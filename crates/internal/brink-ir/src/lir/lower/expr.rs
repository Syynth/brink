use crate::hir;
use crate::symbols::SymbolKind;

use super::context::{self, LowerCtx};
use super::decls::list_def_to_global_var;
use super::lir;

/// Lower a HIR expression to LIR.
#[expect(
    clippy::cast_possible_truncation,
    reason = "f64→f32 is intentional per ink spec"
)]
pub fn lower_expr(expr: &hir::Expr, ctx: &mut LowerCtx<'_>) -> lir::Expr {
    match expr {
        hir::Expr::Int(n) => lir::Expr::Int(*n),
        hir::Expr::Float(bits) => lir::Expr::Float(bits.to_f64() as f32),
        hir::Expr::Bool(b) => lir::Expr::Bool(*b),
        hir::Expr::Null => lir::Expr::Null,

        hir::Expr::String(s) => {
            let parts = s
                .parts
                .iter()
                .map(|p| match p {
                    hir::StringPart::Literal(t) => lir::StringPart::Literal(t.clone()),
                    hir::StringPart::Interpolation(e) => {
                        lir::StringPart::Interpolation(Box::new(lower_expr(e, ctx)))
                    }
                })
                .collect();
            lir::Expr::String(lir::StringExpr { parts })
        }

        hir::Expr::Path(path) => lower_path(path, ctx),

        hir::Expr::DivertTarget(path) => {
            if let Some(id) = ctx.resolve_id(path.range) {
                lir::Expr::DivertTarget(id)
            } else {
                lir::Expr::Null
            }
        }

        hir::Expr::ListLiteral(paths) => {
            let mut items = Vec::new();
            let mut origins = Vec::new();
            for path in paths {
                if let Some(id) = ctx.resolve_id(path.range)
                    && let Some(info) = ctx.index.symbols.get(&id)
                {
                    if info.kind == SymbolKind::ListItem {
                        items.push(id);
                        // Derive the origin list from the item's qualified name
                        // (e.g. "list2.a2" → "list2") and add it to origins.
                        if let Some(dot) = info.name.rfind('.') {
                            let list_name = &info.name[..dot];
                            if let Some(list_ids) = ctx.index.by_name.get(list_name) {
                                for &list_id in list_ids {
                                    if ctx
                                        .index
                                        .symbols
                                        .get(&list_id)
                                        .is_some_and(|s| s.kind == SymbolKind::List)
                                        && !origins.contains(&list_id)
                                    {
                                        origins.push(list_id);
                                    }
                                }
                            }
                        }
                    } else if info.kind == SymbolKind::List {
                        origins.push(id);
                    }
                }
            }
            lir::Expr::ListLiteral { items, origins }
        }

        // PrefixOp, InfixOp, PostfixOp are shared types — pass through directly
        hir::Expr::Prefix(op, inner) => lir::Expr::Prefix(*op, Box::new(lower_expr(inner, ctx))),

        hir::Expr::Infix(lhs, op, rhs) => lir::Expr::Infix(
            Box::new(lower_expr(lhs, ctx)),
            *op,
            Box::new(lower_expr(rhs, ctx)),
        ),

        hir::Expr::Postfix(inner, op) => lir::Expr::Postfix(Box::new(lower_expr(inner, ctx)), *op),

        hir::Expr::Call(path, args) => lower_call(path, args, ctx),

        // T1b sigil collection literals + postfix indexing
        // (docs/t1b-surface-spec.md §3-4). A literal lowers to the V4
        // literal pool (`lir::Expr::ConstLiteral`, deduplicated at codegen)
        // when every element/entry is constant-foldable, else to the
        // runtime construction opcodes (`ArrayNew`/`MapNew`).
        hir::Expr::ArrayLiteral(arr) => lower_array_literal(arr, ctx),
        hir::Expr::MapLiteral(map) => lower_map_literal(map, ctx),
        hir::Expr::Index(idx) => lir::Expr::Index {
            base: Box::new(lower_expr(&idx.base, ctx)),
            index: Box::new(lower_expr(&idx.index, ctx)),
        },

        // NS-A5 range literals (docs/stdlib-spec.md §7, F7): both bounds
        // evaluate left-to-right, then `RangeMake{Excl,Incl}` builds the
        // value. Bound-type faults live at the runtime op.
        hir::Expr::Range(r) => lir::Expr::RangeMake {
            start: Box::new(lower_expr(&r.start, ctx)),
            end: Box::new(lower_expr(&r.end, ctx)),
            inclusive: r.inclusive,
        },

        // TM-4c structs (docs/typed-mode-spec.md §6): construction, field
        // reads, and (through the RMW helpers in `blocks`/`stmts`) field
        // writes all lower for real — see `lower_struct_literal`/
        // `lower_field_access`'s own docs.
        hir::Expr::StructLiteral(sl) => lower_struct_literal(sl, ctx),
        hir::Expr::FieldAccess(fa) => lower_field_access(fa, ctx),

        // T1c-2 (docs/t1c-spec.md §2/§11): `#fn(…)` function values lower
        // for real — `PushFnRef` (zero-bound) / `MakeClosure` (bound prefix)
        // via [`lower_fn_literal`]. The T1c-1 E052 fence that stood here is
        // removed exactly where this real lowering replaces it.
        hir::Expr::FnLiteral(fl) => lower_fn_literal(fl, ctx),

        // T1e-1 (docs/t1e-spec.md §8 sequencing item 1, issue #831): a `ref
        // lvalue-path` reaching *general* expression lowering. This is the
        // E052-fence pattern (see `lir::lower::mod`'s backstop doctrine): a
        // bare single-name `ref x` is handled entirely inside
        // `lower_call_args` (identical to today's unmarked ref-argument
        // binding, never reaches here); anything with a real path segment
        // (dotted field / `[…]` index) — or any `RefArg` outside
        // ref-argument position at all, which should already carry
        // `brink-analyzer`'s E097 — funnels through `lower_call_args`'s
        // fallback arm into this one. T1e-2 lands `MakeProjection`; until
        // then this is a clean, targeted stop, not a silent drop.
        hir::Expr::RefArg(ra) => lower_ref_arg_fence(ra, ctx),
    }
}

/// The T1e-1 E052-fence backstop for a `ref lvalue-path` that reached
/// general expression lowering with at least one real path segment (or fell
/// through from an illegal, non-argument position). `E099` names the
/// specific reason (no `MakeProjection`/`ProjRead` support yet, tracking
/// #828) rather than reusing the generic dialect-gate `E052`. The operand
/// is still lowered (for its own nested diagnostics) and discarded — the
/// whole `RefArg` folds to `Null`, matching every other "reported, not
/// silently dropped" fence in this module.
fn lower_ref_arg_fence(ra: &hir::RefArgExpr, ctx: &mut LowerCtx<'_>) -> lir::Expr {
    ctx.diagnostics.push(crate::Diagnostic {
        file: ctx.file,
        range: ra.ptr.text_range(),
        message: format!(
            "{}: path-projection ref-arguments (`ref {}`) have no runtime \
             representation yet — grammar/HIR/analyzer support lands in T1e-1 \
             (this compiler), lowering in T1e-2 (tracking #828)",
            crate::DiagnosticCode::E099.title(),
            crate::display_expr(&ra.operand),
        ),
        code: crate::DiagnosticCode::E099,
    });
    lower_expr(&ra.operand, ctx);
    lir::Expr::Null
}

/// Lower `#fn(target, args…)` to a [`lir::Expr::MakeFnValue`] (T1c-2,
/// docs/t1c-spec.md §2). The target resolves to a function `DefinitionId`;
/// the bound args reuse the ordinary call-argument lowering
/// ([`lower_call_args`]), so a `ref`-position bound arg becomes a
/// [`lir::CallArg::RefGlobal`] (a captured durable cell) and a `val` a
/// snapshot — exactly the creation-site discipline `brink-analyzer` enforced
/// (E079/E080/E081). Codegen splits zero-bound (`PushFnRef`) from bound
/// (`MakeClosure`).
///
/// An unresolved target is left to the analyzer's own diagnostic (E025/E079);
/// the bound args are still lowered so their subexpression diagnostics fire,
/// but the literal folds to `Null` (never a silent drop of a *reported*
/// construct).
fn lower_fn_literal(fl: &hir::FnLiteral, ctx: &mut LowerCtx<'_>) -> lir::Expr {
    if let Some(info) = ctx.resolve_path(fl.target.range) {
        let target = info.id;
        let bound = lower_call_args(&fl.args, &info.params, ctx);
        lir::Expr::MakeFnValue { target, bound }
    } else {
        for arg in &fl.args {
            lower_expr(arg, ctx);
        }
        lir::Expr::Null
    }
}

/// A sentinel `ShapeId` that never names a real declared shape — `Program::
/// struct_shapes` is populated by [`super::structs::build_shape_table`]
/// with dense ids starting at `0`, so no real project comes remotely close
/// to `u32::MAX` shapes. Used only by [`lower_struct_literal`]'s gradual
/// construction-fault path.
const CONSTRUCTION_FAULT_SHAPE_ID: u32 = u32::MAX;

/// `Name#{field: expr, …}` construction (TM-4c, `docs/typed-mode-spec.md`
/// §6). Every supplied initializer is lowered — and, at codegen time,
/// evaluated — exactly once, in **source** order (the order the author
/// wrote them; decision-log "Struct construction literals: source-order
/// evaluation, duplicate field is a compile error" 2026-07-14, issue #676),
/// regardless of which path below is taken.
///
/// - **Well-formed** (every declared field has exactly one initializer, no
///   extra names): stages each initializer's *already-lowered* expression
///   into a fresh synthetic temp slot, in source order (`prelude` —
///   `LowerCtx::alloc_block_slot`, the same T1b synthetic-temp machinery
///   `lower::blocks`' RMW desugaring uses), then builds `fields` as
///   `GetTemp` reads of those slots reordered into the shape's *declaration*
///   order for `RecordNew` (the VM's required push order). Codegen emits
///   `prelude` first — so every value is *computed* in source order — and
///   only then pushes `fields` in shape order, decoupling evaluation order
///   from placement order. A duplicate field name (last-wins on placement)
///   is a compile error under normal operation (`structs::check_duplicates`'
///   `E084`); this loop still stages *every* supplied initializer (not just
///   the winning last one), so a shadowed duplicate's side effect still
///   fires even under `// brink-disable-all` suppression — never a silent
///   drop (issue #675).
/// - **Mismatched** (a missing declared field, or an initializer for a name
///   the shape doesn't declare): value-model-spec §11c's gradual
///   construction-fault path. Reachable under `types = gradual` (the only
///   policy that compiles this far — under `types = strict` it's already
///   `E069`/`E070`, a compile error, unless that diagnostic was suppressed,
///   in which case this is the non-suppressible runtime backstop). Emits
///   every supplied initializer directly (source order, still evaluated for
///   its side effects; no staging needed since nothing gets reordered)
///   followed by `RecordNew(CONSTRUCTION_FAULT_SHAPE_ID)` — the VM's
///   `record_new` looks up the shape *before* popping any values, so an
///   always-invalid `ShapeId` deterministically turn-terminates via the
///   already-existing `RuntimeError::InvalidShapeId` fault (no new opcode or
///   runtime code needed; see `record_ops::record_new`).
fn lower_struct_literal(sl: &hir::StructLiteral, ctx: &mut LowerCtx<'_>) -> lir::Expr {
    let structs = ctx.structs;
    let Some(shape) = structs.shapes.get(&sl.shape.text) else {
        for (_name, val) in &sl.fields {
            lower_expr(val, ctx);
        }
        return reject_unresolved_struct_shape(sl.ptr.text_range(), ctx);
    };

    let mut placed: Vec<Option<u16>> = vec![None; shape.fields.len()];
    let mut prelude: Vec<(u16, brink_format::NameId, lir::Expr)> =
        Vec::with_capacity(sl.fields.len());
    let mut source_order: Vec<lir::Expr> = Vec::with_capacity(sl.fields.len());
    let mut has_extra = false;
    for (name, val) in &sl.fields {
        let lowered = lower_expr(val, ctx);
        match shape.field(&name.text) {
            Some((offset, _)) => {
                // Stage this initializer's value now, at its source
                // position — the fault path below (if this literal turns
                // out mismatched) never sees `prelude`, only `source_order`.
                let slot = ctx.alloc_block_slot();
                let name_id = ctx.names.intern("__field");
                prelude.push((slot, name_id, lowered.clone()));
                if let Some(p) = placed.get_mut(offset as usize) {
                    *p = Some(slot);
                }
            }
            None => has_extra = true,
        }
        source_order.push(lowered);
    }
    let has_missing = placed.iter().any(Option::is_none);

    if has_extra || has_missing {
        return lir::Expr::RecordNew {
            shape_id: CONSTRUCTION_FAULT_SHAPE_ID,
            fields: source_order,
            prelude: Vec::new(),
        };
    }

    // `has_missing == false` just proved every slot is `Some` — `map_or`
    // rather than `unwrap` anyway (denied in production code; guarded, not
    // asserted, per the E053-backstop lesson): a future refactor that
    // weakens that proof degrades to a well-formed-but-wrong `Null` field
    // instead of a panic.
    lir::Expr::RecordNew {
        shape_id: shape.id,
        fields: placed
            .into_iter()
            .map(|slot| {
                slot.map_or(lir::Expr::Null, |s| {
                    lir::Expr::GetTemp(s, ctx.names.intern("__field"))
                })
            })
            .collect(),
        prelude,
    }
}

/// `base.field` (read) — TM-4c. Chainable: `o.inner.v` lowers as nested
/// `FieldAccessExpr`, and [`known_shape`] chases the chain through declared
/// nested-struct field types (no type inference — see that function's doc)
/// to decide `static_offset` eligibility at every hop, not just the first.
fn lower_field_access(fa: &hir::FieldAccessExpr, ctx: &mut LowerCtx<'_>) -> lir::Expr {
    let static_offset = static_offset_for(&fa.base, &fa.field.text, ctx);
    let field = ctx.names.intern(&fa.field.text);
    let base = lower_expr(&fa.base, ctx);
    lir::Expr::RecordGet {
        base: Box::new(base),
        field,
        static_offset,
    }
}

/// TM-4c "compile-time known shape" — see `lower::structs`' module doc for
/// the full soundness argument. Only ever returns `Some` under `types =
/// strict`: a struct-typed annotation is trustworthy *only* because
/// strict-mode's own type checking (E065/E066, `docs/typed-mode-spec.md`
/// §1) rejects a program where the annotation could lie; under `types =
/// gradual` nothing enforces it, so this always returns `None` there and
/// field ops fall back to the always-correct by-name form.
fn static_offset_for(base: &hir::Expr, field_name: &str, ctx: &LowerCtx<'_>) -> Option<u16> {
    if ctx.structs.type_mode != crate::lir::TypeMode::Strict {
        return None;
    }
    let shape_name = known_shape(base, ctx)?;
    let shape = ctx.structs.shapes.get(&shape_name)?;
    shape.field(field_name).map(|(offset, _)| offset)
}

/// Chase `expr` to a compile-time-known struct shape name, if any — the
/// entire "known shape" story is: a construction literal (trivially known),
/// a `Path` naming a struct-typed `VAR`/`CONST`/`temp` (TM-2 annotation,
/// tracked in `structs::GlobalShapeMap`/`LowerCtx::temp_shapes`), or a
/// `FieldAccess` whose base has a known shape *and* whose accessed field is
/// itself declared with a struct-typed annotation (chases through nested
/// struct fields using only the shape table — never type inference, and
/// never anything requiring `brink-analyzer`, which `brink-ir` cannot
/// depend on). Every other expression (a call, an index, a literal-typed
/// value, …) returns `None` — always safe, just misses the optimization.
fn known_shape(expr: &hir::Expr, ctx: &LowerCtx<'_>) -> Option<String> {
    match expr {
        hir::Expr::StructLiteral(sl) => ctx
            .structs
            .shapes
            .get(&sl.shape.text)
            .map(|_| sl.shape.text.clone()),
        hir::Expr::Path(path) => {
            let name = path_to_string(path);
            if let Some(slot) = ctx.temp_slot(&name) {
                ctx.temp_shape(slot).map(str::to_string)
            } else {
                let info = ctx.resolve_path(path.range)?;
                ctx.global_shape(info.id).map(str::to_string)
            }
        }
        hir::Expr::FieldAccess(fa) => {
            let base_shape = known_shape(&fa.base, ctx)?;
            let shape = ctx.structs.shapes.get(&base_shape)?;
            let (_, nested) = shape.field(&fa.field.text)?;
            nested.map(str::to_string)
        }
        _ => None,
    }
}

/// Non-suppressible backstop for a struct construction literal referencing
/// a shape name that doesn't resolve to any declared `STRUCT` (TM-4c,
/// `docs/typed-mode-spec.md` §6) — `RecordNew` needs a real `ShapeId` at
/// compile time, so this can't defer to a runtime path the way a field
/// access can. Mirrors the E053-backstop discipline (#572 review): a real,
/// Error-severity `Diagnostic` pushed into `ctx.diagnostics`, which
/// `brink-db`'s `lir_query` partitions by severity independently of
/// analysis-phase suppression — reaching this from a normal compile means
/// `brink-analyzer`'s `resolve::resolve_struct_ref` diagnostic (`E068`) was
/// suppressed (`// brink-disable-all`), not a compiler bug on its own.
fn reject_unresolved_struct_shape(range: rowan::TextRange, ctx: &mut LowerCtx<'_>) -> lir::Expr {
    ctx.diagnostics.push(crate::Diagnostic {
        file: ctx.file,
        range,
        message: crate::DiagnosticCode::E073.title().to_string(),
        code: crate::DiagnosticCode::E073,
    });
    lir::Expr::Null
}

fn lower_array_literal(arr: &hir::ArrayLiteral, ctx: &mut LowerCtx<'_>) -> lir::Expr {
    let folded: Option<Vec<lir::ConstValue>> = arr.elements.iter().map(try_const_fold).collect();
    if let Some(items) = folded {
        return lir::Expr::ConstLiteral(lir::ConstValue::Array(items));
    }
    lir::Expr::ArrayNew(arr.elements.iter().map(|e| lower_expr(e, ctx)).collect())
}

fn lower_map_literal(map: &hir::MapLiteral, ctx: &mut LowerCtx<'_>) -> lir::Expr {
    let folded: Option<Vec<(lir::ConstMapKey, lir::ConstValue)>> = map
        .entries
        .iter()
        .map(|(k, v)| {
            let key = try_const_fold(k).and_then(const_value_to_map_key)?;
            let value = try_const_fold(v)?;
            Some((key, value))
        })
        .collect();
    if let Some(entries) = folded {
        return lir::Expr::ConstLiteral(lir::ConstValue::Map(entries));
    }
    lir::Expr::MapNew(
        map.entries
            .iter()
            .map(|(k, v)| (lower_expr(k, ctx), lower_expr(v, ctx)))
            .collect(),
    )
}

/// Attempt to fold a HIR expression into a [`lir::ConstValue`] — the T1b
/// literal-pool eligibility test. Only genuinely constant syntax folds
/// (literals, `null`, non-interpolated strings, nested constant array/map
/// literals); any variable reference, call, or operator bails out to the
/// runtime-construction path (`ArrayNew`/`MapNew`).
fn try_const_fold(expr: &hir::Expr) -> Option<lir::ConstValue> {
    match expr {
        hir::Expr::Int(n) => Some(lir::ConstValue::Int(*n)),
        #[expect(
            clippy::cast_possible_truncation,
            reason = "f64->f32 is intentional per ink spec, matches lower_expr's Float arm"
        )]
        hir::Expr::Float(bits) => Some(lir::ConstValue::Float(bits.to_f64() as f32)),
        hir::Expr::Bool(b) => Some(lir::ConstValue::Bool(*b)),
        hir::Expr::Null => Some(lir::ConstValue::Null),
        hir::Expr::String(s) => {
            // Only a single non-interpolated literal part folds; anything
            // with `{expr}` interpolation is not compile-time-constant.
            match s.parts.as_slice() {
                [hir::StringPart::Literal(text)] => Some(lir::ConstValue::String(text.clone())),
                [] => Some(lir::ConstValue::String(String::new())),
                _ => None,
            }
        }
        hir::Expr::ArrayLiteral(arr) => {
            let items: Option<Vec<lir::ConstValue>> =
                arr.elements.iter().map(try_const_fold).collect();
            items.map(lir::ConstValue::Array)
        }
        hir::Expr::MapLiteral(map) => {
            let entries: Option<Vec<(lir::ConstMapKey, lir::ConstValue)>> = map
                .entries
                .iter()
                .map(|(k, v)| {
                    let key = try_const_fold(k).and_then(const_value_to_map_key)?;
                    let value = try_const_fold(v)?;
                    Some((key, value))
                })
                .collect();
            entries.map(lir::ConstValue::Map)
        }
        _ => None,
    }
}

/// Narrow a folded [`lir::ConstValue`] to the ratified map-key domain
/// (int/string/bool). A statically-visible non-key type (float, null,
/// array, map) returns `None` — shared by two callers with different
/// fallbacks for that case: [`lower_map_literal`]'s expression-position path
/// falls back to the `MapNew` runtime path (key-domain validation happens at
/// `MapNew` construction time instead, a turn-terminating fault); `decls`'s
/// `eval_const_expr` (#673) has no runtime construction step for a
/// declaration default to fall back to, so it reports `None` as a real
/// compile error (`E076`) instead.
pub(super) fn const_value_to_map_key(v: lir::ConstValue) -> Option<lir::ConstMapKey> {
    match v {
        lir::ConstValue::Int(n) => Some(lir::ConstMapKey::Int(n)),
        lir::ConstValue::String(s) => Some(lir::ConstMapKey::Str(s)),
        lir::ConstValue::Bool(b) => Some(lir::ConstMapKey::Bool(b)),
        _ => None,
    }
}

/// TM-4c (`docs/typed-mode-spec.md` §6): lower a multi-segment dotted
/// `Path` that `brink-analyzer`'s resolution fallback resolved to a
/// variable/constant/param/temp (the "ambiguous path" case — `p.x` parses
/// as one dotted `Path`, same grammar node as `knot.stitch`, and only the
/// analyzer's resolution disambiguates "field access on a variable" from
/// "static dotted address") — into the equivalent `RecordGet` chain
/// `lower_field_access` would produce for the unambiguous `base.field`
/// grammar. `head_info` is the already-resolved head symbol
/// (`path.segments[0]`); every subsequent segment is a field hop.
fn lower_ambiguous_dotted_path(
    path: &hir::Path,
    head_info: &crate::symbols::SymbolInfo,
    ctx: &mut LowerCtx<'_>,
) -> lir::Expr {
    let Some(head_name) = path.segments.first().map(|n| n.text.clone()) else {
        // Structurally unreachable — the caller already established
        // `path.segments.len() > 1`. Guarded, not asserted, per the
        // E053-backstop lesson: never let a future refactor turn this into
        // a silent corruption.
        return lir::Expr::Null;
    };

    let (mut expr, mut current_shape) = match head_info.kind {
        SymbolKind::Variable | SymbolKind::Constant => (
            lir::Expr::GetGlobal(head_info.id),
            ctx.global_shape(head_info.id).map(str::to_string),
        ),
        SymbolKind::Param | SymbolKind::Temp => {
            let Some(slot) = ctx.temp_slot(&head_name) else {
                return lir::Expr::Null;
            };
            let name_id = ctx.names.intern(&head_name);
            (
                lir::Expr::GetTemp(slot, name_id),
                ctx.temp_shape(slot).map(str::to_string),
            )
        }
        // The caller only reaches here for these four kinds.
        _ => return lir::Expr::Null,
    };

    for seg in &path.segments[1..] {
        let shape_info = current_shape
            .as_deref()
            .and_then(|s| ctx.structs.shapes.get(s));
        let static_offset = if ctx.structs.type_mode == crate::lir::TypeMode::Strict {
            shape_info.and_then(|s| s.field(&seg.text)).map(|(o, _)| o)
        } else {
            None
        };
        let nested_shape = shape_info
            .and_then(|s| s.field(&seg.text))
            .and_then(|(_, nested)| nested.map(str::to_string));
        let field = ctx.names.intern(&seg.text);
        expr = lir::Expr::RecordGet {
            base: Box::new(expr),
            field,
            static_offset,
        };
        current_shape = nested_shape;
    }

    expr
}

fn lower_path(path: &hir::Path, ctx: &mut LowerCtx<'_>) -> lir::Expr {
    // Check temp map first (for shadowing)
    let name = path_to_string(path);
    if let Some(slot) = ctx.temp_slot(&name) {
        let name_id = ctx.names.intern(&name);
        return lir::Expr::GetTemp(slot, name_id);
    }

    // Resolve via resolution map
    if let Some(info) = ctx.resolve_path(path.range) {
        // TM-4b resolution fallback (docs/typed-mode-spec.md §6): a
        // multi-segment `Path` resolving to a variable/constant/param/temp
        // can only mean the analyzer's fallback kicked in (every dotted
        // symbol name this resolution map otherwise produces for those four
        // kinds is single-segment) — i.e. `p.x` field access on `p`, not a
        // static dotted path. TM-4c (#666): lowers for real, as an
        // equivalent `RecordGet` chain — see `lower_ambiguous_dotted_path`.
        if path.segments.len() > 1
            && matches!(
                info.kind,
                SymbolKind::Variable | SymbolKind::Constant | SymbolKind::Param | SymbolKind::Temp
            )
        {
            return lower_ambiguous_dotted_path(path, info, ctx);
        }
        match info.kind {
            SymbolKind::Variable | SymbolKind::Constant => lir::Expr::GetGlobal(info.id),
            SymbolKind::List => {
                // List symbols resolve to ListDef IDs ($03_), but the global
                // variable uses the GlobalVar tag ($02_) with the same hash.
                lir::Expr::GetGlobal(list_def_to_global_var(info.id))
            }
            SymbolKind::ListItem => {
                // A bare list item reference (e.g. `drown`) produces a list
                // value containing just that item, not the raw item value.
                // Find the origin list from the qualified name "list.item".
                let origin = info
                    .name
                    .split_once('.')
                    .and_then(|(list_name, _)| {
                        ctx.index
                            .by_name
                            .get(list_name)
                            .and_then(|ids| {
                                ids.iter().find(|&&id| {
                                    ctx.index
                                        .symbols
                                        .get(&id)
                                        .is_some_and(|s| s.kind == SymbolKind::List)
                                })
                            })
                            .copied()
                    })
                    .into_iter()
                    .collect();
                lir::Expr::ListLiteral {
                    items: vec![info.id],
                    origins: origin,
                }
            }
            SymbolKind::Knot | SymbolKind::Stitch | SymbolKind::Label => {
                lir::Expr::VisitCount(info.id)
            }
            // Temps not caught by temp_slot above are either (a) a classic
            // (non-block) temp used before its declaring statement — a
            // genuine forward reference, matching inklecate's own behavior
            // of emitting a get_global that fails at link time (reproduced
            // below by hashing the name the same way the converter does:
            // DefaultHasher on the name string → GlobalVar tag) — or (b) a
            // T1b block-scoped temp (`~ { … }`) referenced after its
            // `push_block_scope`/`pop_block_scope` bracket has already
            // closed (#680 RCA: this is the actual defect — see E082).
            // `block_scoped_temp_names` (populated by `declare_block_local`,
            // never cleared) distinguishes the two: a name that was ever a
            // block-scoped local can never legitimately reach this fallback
            // any other way, since `temp_slot` already checks every open
            // block scope first.
            SymbolKind::Temp if ctx.block_scoped_temp_names.contains(&name) => {
                ctx.diagnostics.push(crate::Diagnostic {
                    file: ctx.file,
                    range: path.range,
                    message: format!(
                        "{}: `{name}` was declared in a `~ {{ … }}` block that has already \
                         closed — block-scoped temps (docs/t1b-surface-spec.md §2) are only \
                         visible for the rest of their own block",
                        crate::DiagnosticCode::E082.title(),
                    ),
                    code: crate::DiagnosticCode::E082,
                });
                lir::Expr::Null
            }
            SymbolKind::Temp => {
                use brink_format::{DefinitionId, DefinitionTag};
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                name.hash(&mut hasher);
                let global_id = DefinitionId::new(DefinitionTag::GlobalVar, hasher.finish());
                lir::Expr::GetGlobal(global_id)
            }
            // Params should already be caught by temp_slot above; externals
            // used as values are meaningless; a bare `Expr::Path` never
            // resolves to a struct shape name either — a struct construction
            // literal's shape name is registered as `RefKind::Struct`
            // (TM-4b), a disjoint resolution pass from the
            // `RefKind::Variable` one that reaches `lower_path` here (kept
            // only for match exhaustiveness). All three fall back to null.
            SymbolKind::External | SymbolKind::Param | SymbolKind::Struct => lir::Expr::Null,
        }
    } else if path.segments.len() == 1 && path.segments[0].text == "none" {
        // NS-A1 (`docs/stdlib-spec.md` §1.4): an *unresolved* bare `none`
        // is the Option absence literal — the brink-dialect spelling the
        // wire form's `none` variant mirrors. An author symbol of the same
        // name always wins (the resolution branch above, with the E035
        // shadow warning at its declaration site); `strict-ink` rejection
        // is the dialect gate's E051, and a fresh un-annotated
        // `VAR x = none` declaration is the analyzer's E107
        // (bare-`none`-needs-context) — this lowering is the
        // context-is-elsewhere case.
        lir::Expr::OptionNone
    } else {
        lir::Expr::Null
    }
}

fn lower_call(path: &hir::Path, args: &[hir::Expr], ctx: &mut LowerCtx<'_>) -> lir::Expr {
    let name = path_to_string(path);

    // Check builtin table first
    if let Some(builtin) = recognize_builtin(&name) {
        let lir_args: Vec<lir::Expr> = args.iter().map(|a| lower_expr(a, ctx)).collect();
        return lir::Expr::CallBuiltin {
            builtin,
            args: lir_args,
        };
    }

    // Check temp slot first — calling through a temp/param variable holding a divert target.
    if let Some(slot) = ctx.temp_slot(&name) {
        let call_args = lower_call_args(args, &[], ctx);
        let name_id = ctx.names.intern(&name);
        return lir::Expr::CallVariableTemp {
            slot,
            name: name_id,
            args: call_args,
        };
    }

    // Resolve via resolution map
    if let Some(info) = ctx.resolve_path(path.range) {
        // B3a UFCS (issue #1482/#1506): a *multi-segment* callee path
        // resolving to a value is method-call syntax — the resolution
        // record deliberately names the **receiver**, and the real target
        // lives in `brink-analyzer::ufcs`'s verdict side table
        // (`ctx.ufcs`, threaded in as `brink-ir`'s own `UfcsVerdict`
        // mirror — see that type's doc). Falling through would take the
        // `Variable`/`Constant` or catch-all arm below and emit a call
        // against the receiver's own id: a silently wrong program (the
        // pre-#1482 behavior was an `E025` compile refusal, and that
        // refusal must not become a miscompile). `lower_ufcs_call` reads
        // the verdict and lowers each ruled outcome for real; see its own
        // doc for the E144 fallback this branch still refuses with when no
        // verdict was recorded.
        if path.segments.len() > 1
            && matches!(
                info.kind,
                SymbolKind::Param | SymbolKind::Temp | SymbolKind::Variable | SymbolKind::Constant
            )
        {
            return lower_ufcs_call(&name, path, args, ctx);
        }
        match info.kind {
            SymbolKind::List => {
                // list(n) → ListFromInt; list() → empty list with origin.
                if args.is_empty() {
                    lir::Expr::ListLiteral {
                        items: Vec::new(),
                        origins: vec![info.id],
                    }
                } else {
                    let list_name = info
                        .name
                        .split('.')
                        .next()
                        .unwrap_or(&info.name)
                        .to_string();
                    let name_expr = lir::Expr::String(lir::StringExpr {
                        parts: vec![lir::StringPart::Literal(list_name)],
                    });
                    let ordinal_expr = lower_expr(&args[0], ctx);
                    lir::Expr::CallBuiltin {
                        builtin: lir::BuiltinFn::ListFromInt,
                        args: vec![name_expr, ordinal_expr],
                    }
                }
            }
            SymbolKind::External => {
                let call_args = lower_call_args(args, &info.params, ctx);
                lir::Expr::CallExternal {
                    target: info.id,
                    args: call_args,
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "ink externals have <=255 params"
                    )]
                    arg_count: info.params.len() as u8,
                }
            }
            SymbolKind::Variable | SymbolKind::Constant => {
                let call_args = lower_call_args(args, &info.params, ctx);
                lir::Expr::CallVariable {
                    target: info.id,
                    args: call_args,
                }
            }
            _ => {
                let call_args = lower_call_args(args, &info.params, ctx);
                lir::Expr::Call {
                    target: info.id,
                    args: call_args,
                }
            }
        }
    } else if let Some(expr) = lower_t1b_stdlib_call(&name, args, path.range, ctx) {
        expr
    } else {
        tracing::error!(
            "ICE: unresolved call to `{name}` — analyzer marked as builtin but \
             recognize_builtin() returned None and resolution map has no entry"
        );
        lir::Expr::Null
    }
}

/// B3a UFCS lowering (issue #1506): consume `ctx.ufcs`'s verdict (threaded
/// in from `brink-analyzer::ufcs`'s side table — see [`context::UfcsVerdict`]'s
/// doc) to lower a call site that resolved as method-call syntax, for real.
///
/// Reached only for a *resolved* multi-segment callee path whose head is a
/// param/temp/variable/constant — `lower_call`'s caller has already
/// established that. Every such site the analyzer's `ufcs` pass visited
/// carries a verdict (it is, by construction, UFCS-shaped); a project
/// compiled through `brink-db` never reaches the `None` arm below in
/// practice, because a UFCS diagnostic (`E140`–`E143`) is Error-severity and
/// gates `lir_lowering_query` before it ever runs. The `None` fallback stays
/// as a real refusal (the pre-#1506 `E144`, verbatim) rather than a panic or
/// a silent `Null`, for the callers that lower HIR directly without running
/// analysis first (this crate's own tests/benches, `golden_i078.rs`) — see
/// #1482's PR description for the miscompile this guards against.
/// The shared E144 refusal: `name` resolves as method-call syntax that this
/// UFCS lowering cannot turn into a real call, so refuse loudly rather than
/// silently folding to `Null`. Two call sites reach this — [`lower_ufcs_call`]
/// itself, when no verdict was recorded at all (only possible for a caller
/// that lowers HIR directly without running analysis first, e.g. this
/// crate's own tests/benches), and [`lower_ufcs_prelude_desugar`], when a
/// verdict *was* recorded as `PreludeDesugar` but neither this crate's
/// `recognize_builtin` nor its `is_t1b_stdlib_name`/`lower_t1b_stdlib_call`
/// copy recognizes the name — meaning it drifted out of sync with
/// `brink-analyzer`'s own `is_t1b_stdlib_name`/`is_builtin_function` copies
/// (both pairs' own docs say they're hand-synced across the crate boundary
/// with no shared source of truth).
fn push_ufcs_lowering_refusal(
    name: &str,
    range: rowan::TextRange,
    ctx: &mut LowerCtx<'_>,
) -> lir::Expr {
    ctx.diagnostics.push(crate::Diagnostic {
        file: ctx.file,
        range,
        message: format!(
            "{}: `{name}` resolves as method-call syntax, but the compiler cannot \
             lower it yet — spell the call explicitly as a free call for now",
            crate::DiagnosticCode::E144.title(),
        ),
        code: crate::DiagnosticCode::E144,
    });
    lir::Expr::Null
}

fn lower_ufcs_call(
    name: &str,
    path: &hir::Path,
    args: &[hir::Expr],
    ctx: &mut LowerCtx<'_>,
) -> lir::Expr {
    let Some(verdict) = ctx.ufcs.get(ctx.file, path.range).cloned() else {
        return push_ufcs_lowering_refusal(name, path.range, ctx);
    };
    match verdict {
        // Field access wins (`brink-analyzer::ufcs` D1): the whole path,
        // head through the final segment, is an ordinary field-access
        // chain reading the field's (function-typed) value — exactly what
        // `lower_path`'s own TM-4b/4c dotted-path handling
        // (`lower_ambiguous_dotted_path`) already builds for `p.x.y` field
        // reads. Reusing it here means a receiver chain of any depth
        // (`a.b.c()`) lowers through the one RecordGet-chain builder.
        context::UfcsVerdict::FieldCall => {
            let callee = lower_expr(&hir::Expr::Path(path.clone()), ctx);
            let call_args = args.iter().map(|a| lower_expr(a, ctx)).collect();
            lir::Expr::CallValue {
                callee: Box::new(callee),
                args: call_args,
            }
        }
        // A free function in ordinary lexical scope (D4) — `target(recv,
        // args…)`, by value only (D5; a `ref`-first-param target is
        // refused earlier, at analysis, with `E143`).
        context::UfcsVerdict::FreeFnDesugar { target } => {
            lower_ufcs_desugared_call(path, args, target, ctx)
        }
        // A T1b/NS stdlib prelude verb, or a classic ink builtin, with no
        // index symbol of its own (D4's other candidate) — `name(recv,
        // args…)` through the same dispatch an ordinary bare call of that
        // name already reaches.
        context::UfcsVerdict::PreludeDesugar { name } => {
            lower_ufcs_prelude_desugar(path, args, &name, ctx)
        }
    }
}

/// The receiver half of a UFCS desugar: `path` minus its final (method-name)
/// segment, as a synthetic `hir::Path` reusing `path`'s own range — the
/// exact range `brink-analyzer::ufcs` recorded the head's resolution
/// against (`value_receiver_def`), so `lower_path`/`lower_ambiguous_dotted_
/// path`'s existing `ctx.resolve_path`/`ctx.temp_slot` lookups resolve it
/// correctly whether the receiver is one segment (`x`) or a dotted chain
/// (`a.b`).
pub(super) fn ufcs_receiver_path(path: &hir::Path) -> hir::Path {
    let receiver_segs = path.segments.split_last().map_or(&[][..], |(_, rest)| rest);
    hir::Path {
        segments: receiver_segs.to_vec(),
        range: path.range,
    }
}

/// `target(receiver, args…)` — the `UfcsVerdict::FreeFnDesugar` shape.
/// `target` is always a `Knot` or `External` symbol (`brink-analyzer::
/// ufcs::try_free_fn_desugar` only looks those two kinds up); the receiver
/// occupies `target`'s first declared parameter, so the arity/`ref` check
/// for the *rest* of `args` runs against `target`'s params shifted by one.
fn lower_ufcs_desugared_call(
    path: &hir::Path,
    args: &[hir::Expr],
    target: brink_format::DefinitionId,
    ctx: &mut LowerCtx<'_>,
) -> lir::Expr {
    let receiver_expr = lower_expr(&hir::Expr::Path(ufcs_receiver_path(path)), ctx);
    let Some(target_info) = ctx.index.symbols.get(&target) else {
        // Structurally unreachable — `target` came from the analyzer's own
        // `resolve::lookup_by_name` against this same project index. Guard
        // rather than panic, per the E053-backstop lesson.
        return lir::Expr::Null;
    };
    let rest_params = target_info.params.get(1..).unwrap_or(&[]);
    let mut call_args = vec![lir::CallArg::Value(receiver_expr)];
    call_args.extend(lower_call_args(args, rest_params, ctx));

    if target_info.kind == SymbolKind::External {
        lir::Expr::CallExternal {
            target,
            #[expect(
                clippy::cast_possible_truncation,
                reason = "ink externals have <=255 params"
            )]
            arg_count: target_info.params.len() as u8,
            args: call_args,
        }
    } else {
        lir::Expr::Call {
            target,
            args: call_args,
        }
    }
}

/// `name(receiver, args…)` — the `UfcsVerdict::PreludeDesugar` shape.
/// Dispatches through the same two tables an ordinary bare call of `name`
/// already reaches: the classic ink builtin table first
/// ([`recognize_builtin`], `CallBuiltin` — takes already-lowered args), then
/// the T1b/NS stdlib table ([`lower_t1b_stdlib_call`], which lowers its own
/// HIR args internally) — mirroring `lower_call`'s own dispatch order.
fn lower_ufcs_prelude_desugar(
    path: &hir::Path,
    args: &[hir::Expr],
    name: &str,
    ctx: &mut LowerCtx<'_>,
) -> lir::Expr {
    let receiver_path = ufcs_receiver_path(path);

    if let Some(builtin) = recognize_builtin(name) {
        let mut lowered = vec![lower_expr(&hir::Expr::Path(receiver_path), ctx)];
        lowered.extend(args.iter().map(|a| lower_expr(a, ctx)));
        return lir::Expr::CallBuiltin {
            builtin,
            args: lowered,
        };
    }

    let mut desugared_args = Vec::with_capacity(args.len() + 1);
    desugared_args.push(hir::Expr::Path(receiver_path));
    desugared_args.extend(args.iter().cloned());
    // `lower_t1b_stdlib_call` returning `None` here means `name` passed the
    // analyzer's `is_t1b_stdlib_name`/`is_builtin_function` check (that's
    // the only way a `PreludeDesugar` verdict is recorded) but missed both
    // of this crate's own hand-synced copies — a drift bug, not a normal
    // compile outcome. Refuse loudly (E144) instead of silently dropping
    // the call to `Null`.
    lower_t1b_stdlib_call(name, &desugared_args, path.range, ctx)
        .unwrap_or_else(|| push_ufcs_lowering_refusal(name, path.range, ctx))
}

/// Recognize a T1b stdlib call (`docs/t1b-surface-spec.md` §5) reached with
/// no resolved symbol. An author-defined function of the same name always
/// wins — `lower_call`'s `ctx.resolve_path` branch above already handles
/// that (the existing external/knot/list/variable lookup chain resolves a
/// shadowing user symbol exactly like any other call; `brink-analyzer`'s
/// symbol-declaration pass separately warns on the shadow, E035); this is
/// only the "no shadow, genuinely the builtin" fallback.
///
/// Dialect-agnostic at this layer, matching the T1b-2 precedent (every
/// brink-extension construct lowers to a correct program regardless of
/// dialect; `strict-ink` enforcement is a separate analysis diagnostic —
/// `brink-analyzer`'s dialect gate, extended in T1b-3 to flag an unresolved
/// call to one of these names under `strict-ink`).
///
/// `push`/`insert`/`remove`/`remove_at` (the mutators) are statement-only —
/// recognized and fully lowered by `blocks::try_lower_mutator_stmt` *before*
/// a call expression ever reaches here. Reaching here with one of those
/// mutator names means the author used it in expression position (`~ x =
/// push(a, v)`),
/// which is invalid since mutators return nothing (§5) — E056.
#[expect(
    clippy::too_many_lines,
    reason = "one-arm-per-stdlib-name dispatch; splitting would scatter the table"
)]
fn lower_t1b_stdlib_call(
    name: &str,
    args: &[hir::Expr],
    call_range: rowan::TextRange,
    ctx: &mut LowerCtx<'_>,
) -> Option<lir::Expr> {
    if !is_t1b_stdlib_name(name) {
        return None;
    }
    let arity_ok = |ctx: &mut LowerCtx<'_>, expected: usize| -> bool {
        if args.len() == expected {
            true
        } else {
            ctx.diagnostics.push(crate::Diagnostic {
                file: ctx.file,
                range: call_range,
                message: format!(
                    "{}: `{name}` expects {expected} argument(s), got {}",
                    crate::DiagnosticCode::E031.title(),
                    args.len(),
                ),
                code: crate::DiagnosticCode::E031,
            });
            false
        }
    };

    match name {
        "len" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::CollectionLen(Box::new(lower_expr(
                &args[0], ctx,
            ))))
        }
        "keys" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::CollectionKeys(Box::new(lower_expr(
                &args[0], ctx,
            ))))
        }
        "values" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::CollectionValues(Box::new(lower_expr(
                &args[0], ctx,
            ))))
        }
        "contains" => {
            if !arity_ok(ctx, 2) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::CollectionContains {
                container: Box::new(lower_expr(&args[0], ctx)),
                needle: Box::new(lower_expr(&args[1], ctx)),
            })
        }
        // `char_at(s, i)` (T1b stdlib slice 1 completion, issue #857): chars
        // indexing (Unicode scalar values, not bytes — author sanity, per the
        // issue) into a string, single-character `String` result.
        // Turn-terminating fault at the runtime op on an out-of-range `i` or
        // a non-`String`/non-`Int` argument (value-model-spec §11c) — this
        // lowering just recognizes the call shape.
        "char_at" => {
            if !arity_ok(ctx, 2) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::CharAt {
                s: Box::new(lower_expr(&args[0], ctx)),
                index: Box::new(lower_expr(&args[1], ctx)),
            })
        }
        // `clear` (NS-A1, `docs/stdlib-spec.md` §5) joins the statement-only
        // mutators: in-place, returns nothing, so expression position is the
        // same E056 misuse the original three get. NS-A4 adds `sort`/
        // `sort_by` (F0: imperative = in-place, `void` — `sorted`/
        // `sorted_by` are the expression twins below).
        "push" | "insert" | "remove" | "remove_at" | "clear" | "sort" | "sort_by" | "heap_push" => {
            ctx.diagnostics.push(crate::Diagnostic {
                file: ctx.file,
                range: call_range,
                message: format!(
                    "{}: `{name}` mutates its first argument and returns nothing — it can \
                     only be used as a statement, not an expression",
                    crate::DiagnosticCode::E056.title(),
                ),
                code: crate::DiagnosticCode::E056,
            });
            Some(lir::Expr::Null)
        }
        // ── NS-A1 Option verbs (issue #1107, `docs/stdlib-spec.md` §§3-5,
        // §1.4). Pure query verbs lower like `contains`/`char_at` above;
        // runtime fault semantics (wrong container type, unorderable
        // elements) live entirely at the ops — these lowerings just
        // recognize the call shapes. ─────────────────────────────────────
        "some" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::OptionSome(Box::new(lower_expr(&args[0], ctx))))
        }
        "find" => {
            if !arity_ok(ctx, 2) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::StrFind {
                s: Box::new(lower_expr(&args[0], ctx)),
                sub: Box::new(lower_expr(&args[1], ctx)),
            })
        }
        "index_of" => {
            if !arity_ok(ctx, 2) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::SeqIndexOf {
                seq: Box::new(lower_expr(&args[0], ctx)),
                needle: Box::new(lower_expr(&args[1], ctx)),
            })
        }
        // `min`/`max` carry two call shapes since NS-A8: the one-arg NS-A1
        // array extremum (`min(a) → Option[T]`) and the two-arg tower
        // componentwise form (`min(a, b)` over same-kind vectors — the
        // mini-spec's "defined once across the tower"; the scalar width-1
        // floor is Domain 1, unsequenced, deliberately NOT shipped here).
        // Any other arity is the E031 arity diagnostic against the one-arg
        // shape (the pre-A8 message, unchanged).
        "min" => {
            if args.len() == 2 {
                return Some(lower_tower_call(brink_format::TowerOp::Min, args, ctx));
            }
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::SeqMin(Box::new(lower_expr(&args[0], ctx))))
        }
        "max" => {
            if args.len() == 2 {
                return Some(lower_tower_call(brink_format::TowerOp::Max, args, ctx));
            }
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::SeqMax(Box::new(lower_expr(&args[0], ctx))))
        }
        // ── NS-A8 numeric tower (issue #1114, `docs/tower-mini-spec.md`).
        // Constructors take numeric lanes (matrices: column vectors, T3's
        // column-major pin); verbs are pure. Runtime fault semantics
        // (wrong operand kind) live entirely at `tower_ops` — these
        // lowerings just recognize the call shapes. ─────────────────────
        "vec2" => {
            if !arity_ok(ctx, 2) {
                return Some(lir::Expr::Null);
            }
            Some(lower_tower_call(brink_format::TowerOp::MakeVec2, args, ctx))
        }
        "vec3" => {
            if !arity_ok(ctx, 3) {
                return Some(lir::Expr::Null);
            }
            Some(lower_tower_call(brink_format::TowerOp::MakeVec3, args, ctx))
        }
        "vec4" => {
            if !arity_ok(ctx, 4) {
                return Some(lir::Expr::Null);
            }
            Some(lower_tower_call(brink_format::TowerOp::MakeVec4, args, ctx))
        }
        "quat" => {
            if !arity_ok(ctx, 4) {
                return Some(lir::Expr::Null);
            }
            Some(lower_tower_call(brink_format::TowerOp::MakeQuat, args, ctx))
        }
        "mat2" => {
            if !arity_ok(ctx, 2) {
                return Some(lir::Expr::Null);
            }
            Some(lower_tower_call(brink_format::TowerOp::MakeMat2, args, ctx))
        }
        "mat3" => {
            if !arity_ok(ctx, 3) {
                return Some(lir::Expr::Null);
            }
            Some(lower_tower_call(brink_format::TowerOp::MakeMat3, args, ctx))
        }
        "mat4" => {
            if !arity_ok(ctx, 4) {
                return Some(lir::Expr::Null);
            }
            Some(lower_tower_call(brink_format::TowerOp::MakeMat4, args, ctx))
        }
        "dot" => {
            if !arity_ok(ctx, 2) {
                return Some(lir::Expr::Null);
            }
            Some(lower_tower_call(brink_format::TowerOp::Dot, args, ctx))
        }
        "cross" => {
            if !arity_ok(ctx, 2) {
                return Some(lir::Expr::Null);
            }
            Some(lower_tower_call(brink_format::TowerOp::Cross, args, ctx))
        }
        "clamp" => {
            if !arity_ok(ctx, 3) {
                return Some(lir::Expr::Null);
            }
            Some(lower_tower_call(brink_format::TowerOp::Clamp, args, ctx))
        }
        "lerp" => {
            if !arity_ok(ctx, 3) {
                return Some(lir::Expr::Null);
            }
            Some(lower_tower_call(brink_format::TowerOp::Lerp, args, ctx))
        }
        "first" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::SeqFirst(Box::new(lower_expr(&args[0], ctx))))
        }
        "last" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::SeqLast(Box::new(lower_expr(&args[0], ctx))))
        }
        "get" => {
            if !arity_ok(ctx, 2) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::MapGetOpt {
                map: Box::new(lower_expr(&args[0], ctx)),
                key: Box::new(lower_expr(&args[1], ctx)),
            })
        }
        "contains_value" => {
            if !arity_ok(ctx, 2) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::MapContainsValue {
                map: Box::new(lower_expr(&args[0], ctx)),
                value: Box::new(lower_expr(&args[1], ctx)),
            })
        }
        // `pop(a)` (§4): both mutator and expression — mutates its bare
        // lvalue receiver in place and produces `Option[T]`. The receiver
        // must be a bare variable/temp so codegen can bracket the runtime
        // op with take/store against the root cell (the RMW discipline);
        // anything else — an rvalue (`pop(#[1,2])`) or a chained lvalue
        // (`pop(grid[0])`, an A1 scope fence) — is the E055-family
        // "bind it to a variable first" compile error.
        "pop" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            // `lower_assign_target` accepts exactly the bare-`Path` shape
            // (temp slot or resolvable global) — `None` for everything else.
            if let Some(root) = super::stmts::lower_assign_target(&args[0], ctx) {
                return Some(lir::Expr::SeqPop { root });
            }
            ctx.diagnostics.push(crate::Diagnostic {
                file: ctx.file,
                range: call_range,
                message: format!(
                    "{}: `pop` mutates its first argument — bind it to a variable first",
                    crate::DiagnosticCode::E055.title(),
                ),
                code: crate::DiagnosticCode::E055,
            });
            Some(lir::Expr::Null)
        }
        // TM-3 completion (docs/typed-mode-spec.md §4, maintainer ruling
        // 2026-07-13, issue #659): `int(x)`/`float(x)`/`string(x)` pure
        // conversion intrinsics. Fault semantics (parse failure,
        // out-of-domain input) live entirely at the runtime op — this
        // lowering just recognizes the call shape.
        "int" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::ConvertInt(Box::new(lower_expr(&args[0], ctx))))
        }
        // `float` is two verbs disambiguated by arity (NS-A6, F4 resolved
        // in-wave per `docs/stdlib-sequencing.md` §2 A6): nullary
        // `float()` is the `std::rand` uniform-`[0,1)` draw
        // (`docs/stdlib-spec.md` §7 — one RNG-cell write); unary
        // `float(x)` stays the TM-3 pure conversion intrinsic. Any other
        // arity is E031 naming both forms.
        "float" => match args.len() {
            0 => Some(lir::Expr::RandFloat),
            1 => Some(lir::Expr::ConvertFloat(Box::new(lower_expr(&args[0], ctx)))),
            n => {
                ctx.diagnostics.push(crate::Diagnostic {
                    file: ctx.file,
                    range: call_range,
                    message: format!(
                        "{}: `float` expects 0 arguments (random draw in [0,1)) or 1 \
                         argument (numeric conversion), got {n}",
                        crate::DiagnosticCode::E031.title(),
                    ),
                    code: crate::DiagnosticCode::E031,
                });
                Some(lir::Expr::Null)
            }
        },
        "string" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::ConvertString(Box::new(lower_expr(
                &args[0], ctx,
            ))))
        }
        // ── NS-A6 rand verbs (issue #1112, `docs/stdlib-spec.md` §7).
        // Draw semantics (clamping, Option-on-empty, the pinned draw
        // chain) live entirely at the runtime ops — these lowerings just
        // recognize the call shapes. `float()`'s nullary arm is above,
        // merged with the conversion intrinsic (F4 arity split). ─────────
        "chance" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::RandChance(Box::new(lower_expr(&args[0], ctx))))
        }
        "pick" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::RandPick(Box::new(lower_expr(&args[0], ctx))))
        }
        // ── NS-A5: `non_empty(r)` — the inhabited-range validator
        // (`docs/stdlib-spec.md` §7, S2). Pure; `Option[NonEmptyRange]`
        // typing is the analyzer's; this lowering just recognizes the
        // call shape. (`int(range)` needs no arm — the unary `int`
        // conversion arm above already lowers it to `ConvertInt`, whose
        // VM op dispatches on the operand.) ─────────────────────────────
        "non_empty" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::RangeNonEmpty(Box::new(lower_expr(
                &args[0], ctx,
            ))))
        }
        // ── NS-A7 collections+ (issue #1113, `docs/stdlib-spec.md` §8).
        // `weighted(…)` carries the compile-classifiable half of the
        // evidence-by-construction split HERE (E120 — the E055/E056/E058
        // "recognition site owns the shape errors" precedent); `roll` and
        // `heap_peek` are ordinary expression verbs; `heap_pop` is the
        // `pop` shape (mutator AND expression); `heap_push` is
        // statement-only (the E056 arm above + `blocks`'s mutator
        // machinery). Runtime semantics (computed-weight construction
        // fault, the min-heap sift, draw shaping) live at the ops. ───────
        "weighted" => Some(lower_weighted_call(args, call_range, ctx)),
        "roll" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::RandRoll(Box::new(lower_expr(&args[0], ctx))))
        }
        "heap_peek" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::HeapPeek(Box::new(lower_expr(&args[0], ctx))))
        }
        // `heap_pop(a)` (§8): both mutator and expression — the `pop`
        // shape exactly, same bare-receiver restriction (the A1 scope
        // fence) and the same E055 otherwise.
        "heap_pop" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            if let Some(root) = super::stmts::lower_assign_target(&args[0], ctx) {
                return Some(lir::Expr::HeapPop { root });
            }
            ctx.diagnostics.push(crate::Diagnostic {
                file: ctx.file,
                range: call_range,
                message: format!(
                    "{}: `heap_pop` mutates its first argument — bind it to a variable first",
                    crate::DiagnosticCode::E055.title(),
                ),
                code: crate::DiagnosticCode::E055,
            });
            Some(lir::Expr::Null)
        }

        // ── NS-A4 ordering verbs (issue #1110, `docs/stdlib-spec.md`
        // §4b). `sorted(a)`/`sorted_by(a, cmp)` are the functional
        // past-participle twins (F0); the imperative `sort`/`sort_by` are
        // statement-only — recognized by `blocks::try_lower_mutator_stmt`,
        // E056 in expression position (the arm above). Ordering semantics
        // (dev NaN-fault / prod pinned order, the comparator contract)
        // live entirely at the runtime ops. ─────────────────────────────
        "sorted" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::SeqSorted(Box::new(lower_expr(&args[0], ctx))))
        }
        "sorted_by" => {
            if !arity_ok(ctx, 2) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::SeqSortedBy {
                seq: Box::new(lower_expr(&args[0], ctx)),
                cmp: Box::new(lower_expr(&args[1], ctx)),
            })
        }
        // `shuffled(a)` — the functional twin (§4's ruled naming
        // convention): evaluates its argument, returns a new shuffled
        // array; the argument itself is never written back.
        "shuffled" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::RandShuffle(Box::new(lower_expr(&args[0], ctx))))
        }
        // `shuffle(a)` (in-place) and `seed(n)` (writes the RNG cell) are
        // statement-only — recognized and fully lowered by
        // `blocks::try_lower_mutator_stmt` before a call expression ever
        // reaches here; expression position is the same E056 misuse the
        // A1/slice-1 mutators get.
        "shuffle" | "seed" => {
            ctx.diagnostics.push(crate::Diagnostic {
                file: ctx.file,
                range: call_range,
                message: format!(
                    "{}: `{name}` returns nothing — it can only be used as a statement, \
                     not an expression",
                    crate::DiagnosticCode::E056.title(),
                ),
                code: crate::DiagnosticCode::E056,
            });
            Some(lir::Expr::Null)
        }
        // T1c (docs/t1c-spec.md §3): the explicit call form `call(f, args…)` —
        // dispatch through a function value where the callee is itself an
        // expression. `f` is the callee; the remaining args are the supplied
        // (val-only) params. Lowers to `CallValue(argc)`; the runtime op is
        // the gradual-mode backstop (dispatch, arity, type faults). Under
        // `types = strict` this form is now statically checked too (issue
        // #733): `brink_analyzer::infer::body::infer_intrinsic` routes `call`
        // through the same `Ty::Fn` value-call machinery as `#fn(...)`/direct
        // calls (`check_value_call`), so a strict author reaches the runtime
        // fault only through a genuinely `Unknown`/`Conflicted` callee (an
        // escape error, `E065`/`E066`).
        "call" => {
            if args.is_empty() {
                ctx.diagnostics.push(crate::Diagnostic {
                    file: ctx.file,
                    range: call_range,
                    message: format!(
                        "{}: `call` needs at least the callee function value \
                         (`call(f, args…)`)",
                        crate::DiagnosticCode::E031.title(),
                    ),
                    code: crate::DiagnosticCode::E031,
                });
                return Some(lir::Expr::Null);
            }
            let callee = lower_expr(&args[0], ctx);
            let supplied = args[1..].iter().map(|a| lower_expr(a, ctx)).collect();
            Some(lir::Expr::CallValue {
                callee: Box::new(callee),
                args: supplied,
            })
        }
        // T1c-3 (docs/t1c-spec.md §3): `bind(f, args…)` — val-only currying
        // over an existing function value. `f` is the callee; the remaining
        // args are appended to its bound-arg row (consuming the head of its
        // remaining param row) and a new function value is returned. Lowers
        // to `BindValue(argc)`; over-binding and a non-function callee are
        // runtime faults at the op — the gradual-mode backstop. Under
        // `types = strict` this form is now statically checked too (issue
        // #733): `brink_analyzer::infer::body::infer_intrinsic` routes `bind`
        // through `check_bind_value` (the "consume the head of the param
        // row" rule applied to a known `Ty::Fn` callee — over-binding
        // becomes a compile-time `E063`, same code family as the direct-call
        // arg/arity mismatches); `Unknown`/`Conflicted` callees still escape
        // as `E065`/`E066`, same as `call` (see `resolve.rs::
        // is_t1b_stdlib_name`).
        "bind" => {
            if args.is_empty() {
                ctx.diagnostics.push(crate::Diagnostic {
                    file: ctx.file,
                    range: call_range,
                    message: format!(
                        "{}: `bind` needs at least the callee function value \
                         (`bind(f, args…)`)",
                        crate::DiagnosticCode::E031.title(),
                    ),
                    code: crate::DiagnosticCode::E031,
                });
                return Some(lir::Expr::Null);
            }
            let callee = lower_expr(&args[0], ctx);
            let supplied = args[1..].iter().map(|a| lower_expr(a, ctx)).collect();
            Some(lir::Expr::BindValue {
                callee: Box::new(callee),
                args: supplied,
            })
        }
        _ => None,
    }
}

/// NS-A7 (`docs/stdlib-spec.md` §8, issue #1113): lower a `weighted(…)`
/// construction call, firing the **compile-classifiable** half of the
/// evidence-by-construction split as E120 (Error severity — the program is
/// refused, the E055/E056/E058 discipline): an empty pair row, an odd
/// (dangling-weight) argument count, or a **literal** weight that is not a
/// positive int (zero/negative int, float/string/bool literal). Everything
/// else — computed weights, CONST refs, arbitrary expressions — lowers
/// through and carries the construction-fault residual at the runtime op
/// (`WeightedBadWeight`). Weights sit at the even argument positions:
/// `weighted(w1, v1, w2, v2, …)`, the brink-dialect spelling of the
/// chartered `Weighted { w: v, … }` literal (B5 later lowers the native
/// grammar to this same node).
fn lower_weighted_call(
    args: &[hir::Expr],
    call_range: rowan::TextRange,
    ctx: &mut LowerCtx<'_>,
) -> lir::Expr {
    let refuse = |ctx: &mut LowerCtx<'_>, detail: &str| {
        ctx.diagnostics.push(crate::Diagnostic {
            file: ctx.file,
            range: call_range,
            message: format!("{}: {detail}", crate::DiagnosticCode::E120.title()),
            code: crate::DiagnosticCode::E120,
        });
        lir::Expr::Null
    };
    if args.is_empty() {
        return refuse(
            ctx,
            "a weighted table cannot be empty — construction is the validator \
             (`weighted(weight, value, …)`)",
        );
    }
    if !args.len().is_multiple_of(2) {
        return refuse(
            ctx,
            "`weighted` takes weight/value pairs — got a dangling weight \
             (`weighted(weight, value, …)`)",
        );
    }
    // Classify literal weights (even positions). Non-literal weights are
    // deliberately NOT classified here — they are the computed-weight
    // residual the runtime construction fault owns.
    for pair in args.chunks_exact(2) {
        match &pair[0] {
            hir::Expr::Int(w) if *w >= 1 => {}
            hir::Expr::Int(w) => {
                return refuse(
                    ctx,
                    &format!("weight {w} is not positive — weights are positive ints (v1)"),
                );
            }
            // `-3` parses as `Prefix(Negate, Int(3))` — a negated numeric
            // literal is exactly as classifiable as a bare one.
            hir::Expr::Prefix(hir::PrefixOp::Negate, inner)
                if matches!(inner.as_ref(), hir::Expr::Int(_) | hir::Expr::Float(_)) =>
            {
                return refuse(
                    ctx,
                    "a negated literal weight is not positive — weights are positive ints (v1)",
                );
            }
            hir::Expr::Float(_) => {
                return refuse(ctx, "weights are positive ints (v1), got a float literal");
            }
            hir::Expr::Bool(_) => {
                return refuse(ctx, "weights are positive ints (v1), got a bool literal");
            }
            hir::Expr::String(_) => {
                return refuse(ctx, "weights are positive ints (v1), got a string literal");
            }
            _ => {}
        }
    }
    let pairs = args
        .chunks_exact(2)
        .map(|pair| (lower_expr(&pair[0], ctx), lower_expr(&pair[1], ctx)))
        .collect();
    lir::Expr::WeightedNew { pairs }
}

/// The T1b stdlib slice 1 function names (`docs/t1b-surface-spec.md` §5)
/// plus the TM-3-completion pure conversion intrinsics (`docs/
/// typed-mode-spec.md` §4, issue #659) — brink-dialect-gated free functions,
/// all sharing the same resolution-fallback/shadowing/dialect-gate
/// machinery (the #659 ruling: "per the stdlib slice-1 pattern"). Kept in
/// sync by hand with `brink_analyzer::resolve::is_t1b_stdlib_name` — the two
/// crates don't share a dependency edge for this purpose (mirrors the
/// existing `recognize_builtin`/`is_builtin_function` split for the classic
/// uppercase ink intrinsics).
pub(crate) fn is_t1b_stdlib_name(name: &str) -> bool {
    matches!(
        name,
        "len"
            | "keys"
            | "values"
            | "contains"
            | "push"
            | "insert"
            | "remove"
            // `remove_at(a, i)` (issue #1484, `docs/stdlib-spec.md` §4/§10):
            // faulting array-index removal, split off `remove` (now
            // map-only — identity-based, idempotent-total, matching flags
            // `remove`) so one name no longer spans two removal postures.
            | "remove_at"
            | "int"
            | "float"
            | "string"
            | "call"
            | "bind"
            | "char_at"
            // NS-A1 (issue #1107, `docs/stdlib-spec.md` §§3-5 + §1.4): the
            // Option verb flips + the `some(x)` constructor. The bare
            // `none` literal is variable-position (`lower_path`), not a
            // call form, so it is deliberately absent from this list.
            | "find"
            | "index_of"
            | "min"
            | "max"
            | "first"
            | "last"
            | "pop"
            | "get"
            | "contains_value"
            | "clear"
            | "some"
            // NS-A6 (issue #1112, `docs/stdlib-spec.md` §7): the
            // `std::rand` draw verbs. `float` (nullary draw / unary
            // conversion, F4 arity split) and `int` (conversion only —
            // `int(range)` is deferred to A5 with the inhabited-range
            // refinement) are already listed above.
            | "chance"
            | "pick"
            | "shuffle"
            | "shuffled"
            | "seed"
            // NS-A5 (issue #1111, `docs/stdlib-spec.md` §7): the
            // inhabited-range validator. `int(range)` needs no entry —
            // `int` is listed above; the VM dispatches on the operand.
            | "non_empty"
            // NS-A7 (issue #1113, `docs/stdlib-spec.md` §8): `Weighted[T]`
            // construction, the `roll` draw, and the humble heap.
            | "weighted"
            | "roll"
            | "heap_push"
            | "heap_pop"
            | "heap_peek"
            // NS-A4 (issue #1110, `docs/stdlib-spec.md` §4b, F0): the
            // ordering verbs — imperative in-place pair + functional
            // past-participle twins.
            | "sort"
            | "sort_by"
            | "sorted"
            | "sorted_by"
            // NS-A8 (issue #1114, `docs/tower-mini-spec.md`; ruled shape
            // `docs/stdlib-spec.md` §2b): the numeric tower — constructors
            // (`vec2(x, y)` … `mat4(c0, c1, c2, c3)`, matrices from
            // column vectors per T3's column-major pin), `dot`/`cross`,
            // and the tower-wide `clamp`/`lerp` (`min`/`max` are already
            // listed above — their two-arg call shape lowers to the tower
            // componentwise forms, the one-arg shape stays the NS-A1
            // array extremum). Same slice-1 machinery end to end:
            // shadowable with E035, `strict-ink` rejection via the
            // dialect gate. All pure.
            | "vec2"
            | "vec3"
            | "vec4"
            | "quat"
            | "mat2"
            | "mat3"
            | "mat4"
            | "dot"
            | "cross"
            | "clamp"
            | "lerp"
    )
}

/// Lower a tower call's args in order into a `lir::Expr::Tower` (NS-A8 —
/// the caller has already checked arity).
fn lower_tower_call(
    op: brink_format::TowerOp,
    args: &[hir::Expr],
    ctx: &mut LowerCtx<'_>,
) -> lir::Expr {
    lir::Expr::Tower {
        op,
        args: args.iter().map(|a| lower_expr(a, ctx)).collect(),
    }
}

/// The pre-T1e ref-argument binding for a bare (single-segment) path —
/// exactly [`lower_call_args`]'s original `hir::Expr::Path` arm, extracted
/// so the T1e `hir::Expr::RefArg` arm can reuse it verbatim for the
/// zero-segment case (`ref gold` binds exactly like unmarked `gold` always
/// has).
fn lower_ref_path_call_arg(
    path: &hir::Path,
    original: &hir::Expr,
    ctx: &mut LowerCtx<'_>,
) -> lir::CallArg {
    let name = path_to_string(path);
    if let Some(slot) = ctx.temp_slot(&name) {
        // B1b (issue #1475): `ref` must not bypass an `as` binding's
        // immutability. `lower_assign_target` is the write-target choke
        // point for ordinary assignment, but `ref x` never routes through
        // it — it hands the callee a raw pointer to the slot
        // (`Opcode::PushTempPointer`), and a `ref`-param write-through
        // (`Opcode::SetTemp`'s `Value::TempPointer` arm) mutates it
        // directly. Refuse here too so every write path is actually
        // covered, not just the ones that go through assignment lowering.
        if ctx.as_binding_slots.contains(&slot) {
            ctx.diagnostics.push(crate::Diagnostic {
                file: ctx.file,
                range: path.range,
                message: format!(
                    "{}: `{name}` is an `as` binding — it is immutable and cannot be passed \
                     by `ref`",
                    crate::DiagnosticCode::E148.title(),
                ),
                code: crate::DiagnosticCode::E148,
            });
            return lir::CallArg::Value(lir::Expr::Null);
        }
        let name_id = ctx.names.intern(&name);
        return lir::CallArg::RefTemp(slot, name_id);
    }
    if let Some(info) = ctx.resolve_path(path.range) {
        // A T1b block-scoped temp (`~ { … }`) passed by `ref` after its own
        // block has closed — same #680 RCA as `lower_path`'s
        // `SymbolKind::Temp` arm. Without this check `info.id` (the temp's
        // `LocalVar`-tagged id, never registered as a real global) would be
        // emitted as a `RefGlobal`, faulting at runtime as
        // `UnresolvedGlobal` with no compile diagnostic.
        if info.kind == SymbolKind::Temp && ctx.block_scoped_temp_names.contains(&name) {
            ctx.diagnostics.push(crate::Diagnostic {
                file: ctx.file,
                range: path.range,
                message: format!(
                    "{}: `{name}` was declared in a `~ {{ … }}` block that has already \
                     closed — block-scoped temps (docs/t1b-surface-spec.md §2) are only \
                     visible for the rest of their own block",
                    crate::DiagnosticCode::E082.title(),
                ),
                code: crate::DiagnosticCode::E082,
            });
            return lir::CallArg::Value(lir::Expr::Null);
        }
        let id = if info.kind == SymbolKind::List {
            list_def_to_global_var(info.id)
        } else {
            info.id
        };
        return lir::CallArg::RefGlobal(id);
    }
    lir::CallArg::Value(lower_expr(original, ctx))
}

pub(super) fn lower_call_args(
    args: &[hir::Expr],
    params: &[crate::symbols::ParamInfo],
    ctx: &mut LowerCtx<'_>,
) -> Vec<lir::CallArg> {
    args.iter()
        .enumerate()
        .map(|(i, arg)| {
            let is_ref = params.get(i).is_some_and(|p| p.is_ref);
            if is_ref {
                match arg {
                    hir::Expr::Path(path) => lower_ref_path_call_arg(path, arg, ctx),
                    // T1e-2 (docs/t1e-spec.md §2/§3, tracking #828): an
                    // explicit `ref` marking a bare single-name path (`ref
                    // gold`) is not a real path *projection* — zero
                    // segments — so it binds exactly like today's unmarked
                    // form. Anything with a real segment (dotted field,
                    // `[…]` index, or any deeper mix) is a genuine T1e
                    // projection, lowered for real via
                    // `lower_ref_projection_arg` — the T1e-1 `E099` fence
                    // this replaces. The analyzer's own durable-root/shape/
                    // position checks already ran by the time lowering sees
                    // it; `lower_ref_projection_arg` still falls back to the
                    // fence if the root somehow doesn't resolve (defense in
                    // depth, not the expected path).
                    hir::Expr::RefArg(ra) => match ra.operand.as_ref() {
                        hir::Expr::Path(path) if path.segments.len() == 1 => {
                            lower_ref_path_call_arg(path, &ra.operand, ctx)
                        }
                        _ => lower_ref_projection_arg(ra, ctx),
                    },
                    _ => lir::CallArg::Value(lower_expr(arg, ctx)),
                }
            } else {
                lir::CallArg::Value(lower_expr(arg, ctx))
            }
        })
        .collect()
}

/// One path-projection segment source, decomposed from the HIR operand tree
/// (mirrors `brink-analyzer::ref_projection`'s private `decompose`/`Segment`
/// shape — brink-ir can't depend on brink-analyzer, so this is its own
/// small copy of the same walk).
enum ProjSegmentSrc<'a> {
    /// `.field` — a dotted field-access segment.
    Field(&'a hir::Name),
    /// `[index]` — an indexing segment; the subexpression is evaluated once
    /// at `ref` creation (snapshot-at-creation, spec §1(1)).
    Index(&'a hir::Expr),
}

/// Decompose an lvalue-shaped HIR expression into its root `Path` and its
/// ordered segment chain. `None` if `expr` isn't lvalue-shaped at all — the
/// analyzer's own `E080` already rejects that case before lowering ever
/// sees it, so this is a defense-in-depth `None`, not an expected miss.
fn decompose_projection(expr: &hir::Expr) -> Option<(&hir::Path, Vec<ProjSegmentSrc<'_>>)> {
    match expr {
        hir::Expr::Path(p) => {
            // A multi-segment bare `Path` (`npc.hp`) is the TM-4b
            // resolution-fallback shape: the analyzer resolves the *whole*
            // path's range to the root variable, so every segment past the
            // first is a field-access segment (same convention
            // `ref_projection::decompose` documents).
            let segments = p.segments[1..].iter().map(ProjSegmentSrc::Field).collect();
            Some((p, segments))
        }
        hir::Expr::FieldAccess(fa) => {
            let (root, mut segments) = decompose_projection(&fa.base)?;
            segments.push(ProjSegmentSrc::Field(&fa.field));
            Some((root, segments))
        }
        hir::Expr::Index(idx) => {
            let (root, mut segments) = decompose_projection(&idx.base)?;
            segments.push(ProjSegmentSrc::Index(&idx.index));
            Some((root, segments))
        }
        _ => None,
    }
}

/// Lower a real path-projection `ref` argument (≥1 segment) to
/// [`lir::CallArg::RefProjection`] — first real lowering of the T1e
/// projection surface (`docs/t1e-spec.md` §2/§3), replacing the T1e-1
/// `E099` fence for this shape. Field segments lower to a literal string
/// expression (the field name); index segments lower via ordinary
/// [`lower_expr`] — both evaluated once, in source order, at the `ref`
/// creation site (snapshot-at-creation, spec §1(1)); codegen pushes them in
/// *reverse* so `Opcode::MakeProjection` can pop them back into source
/// order.
fn lower_ref_projection_arg(ra: &hir::RefArgExpr, ctx: &mut LowerCtx<'_>) -> lir::CallArg {
    let Some((root, src_segments)) = decompose_projection(&ra.operand) else {
        return lir::CallArg::Value(lower_ref_arg_fence(ra, ctx));
    };
    // B1b (issue #1475): the same `ref`-bypasses-immutability hole as
    // `lower_ref_path_call_arg`, but for a projection's *root* (`ref
    // n.field`, `ref n[0]`) — the root is still the `as` binding's own
    // slot, so a projection off it is exactly as much a write-through as
    // a bare `ref n` would be.
    let root_name = path_to_string(root);
    if let Some(slot) = ctx.temp_slot(&root_name)
        && ctx.as_binding_slots.contains(&slot)
    {
        ctx.diagnostics.push(crate::Diagnostic {
            file: ctx.file,
            range: root.range,
            message: format!(
                "{}: `{root_name}` is an `as` binding — it is immutable and cannot be passed \
                 by `ref`",
                crate::DiagnosticCode::E148.title(),
            ),
            code: crate::DiagnosticCode::E148,
        });
        return lir::CallArg::Value(lir::Expr::Null);
    }
    let Some(info) = ctx.resolve_path(root.range) else {
        return lir::CallArg::Value(lower_ref_arg_fence(ra, ctx));
    };
    let root_id = if info.kind == SymbolKind::List {
        list_def_to_global_var(info.id)
    } else {
        info.id
    };
    let segments = src_segments
        .into_iter()
        .map(|seg| match seg {
            ProjSegmentSrc::Field(name) => lir::Expr::String(lir::StringExpr {
                parts: vec![lir::StringPart::Literal(name.text.clone())],
            }),
            ProjSegmentSrc::Index(index_expr) => lower_expr(index_expr, ctx),
        })
        .collect();
    lir::CallArg::RefProjection {
        root: root_id,
        segments,
    }
}

pub fn path_to_string(path: &hir::Path) -> String {
    path.segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

/// Recognize a built-in function by name (case-sensitive).
fn recognize_builtin(name: &str) -> Option<lir::BuiltinFn> {
    match name {
        "TURNS_SINCE" => Some(lir::BuiltinFn::TurnsSince),
        "READ_COUNT" => Some(lir::BuiltinFn::ReadCount),
        "TURNS" => Some(lir::BuiltinFn::Turns),
        "CHOICE_COUNT" => Some(lir::BuiltinFn::ChoiceCount),
        "RANDOM" => Some(lir::BuiltinFn::Random),
        "SEED_RANDOM" => Some(lir::BuiltinFn::SeedRandom),
        "INT" => Some(lir::BuiltinFn::CastToInt),
        "FLOAT" => Some(lir::BuiltinFn::CastToFloat),
        "FLOOR" => Some(lir::BuiltinFn::Floor),
        "CEILING" => Some(lir::BuiltinFn::Ceiling),
        "POW" => Some(lir::BuiltinFn::Pow),
        "MIN" => Some(lir::BuiltinFn::Min),
        "MAX" => Some(lir::BuiltinFn::Max),
        "LIST_COUNT" => Some(lir::BuiltinFn::ListCount),
        "LIST_MIN" => Some(lir::BuiltinFn::ListMin),
        "LIST_MAX" => Some(lir::BuiltinFn::ListMax),
        "LIST_ALL" => Some(lir::BuiltinFn::ListAll),
        "LIST_INVERT" => Some(lir::BuiltinFn::ListInvert),
        "LIST_RANGE" => Some(lir::BuiltinFn::ListRange),
        "LIST_RANDOM" => Some(lir::BuiltinFn::ListRandom),
        "LIST_VALUE" => Some(lir::BuiltinFn::ListValue),
        "LIST_FROM_INT" => Some(lir::BuiltinFn::ListFromInt),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_recognition() {
        assert_eq!(recognize_builtin("RANDOM"), Some(lir::BuiltinFn::Random));
        assert_eq!(
            recognize_builtin("TURNS_SINCE"),
            Some(lir::BuiltinFn::TurnsSince)
        );
        assert_eq!(
            recognize_builtin("LIST_COUNT"),
            Some(lir::BuiltinFn::ListCount)
        );
        assert_eq!(recognize_builtin("random"), None);
        assert_eq!(recognize_builtin("unknown"), None);
    }

    #[test]
    fn builtin_recognition_turns() {
        // TURNS() is a zero-argument builtin that returns the current turn index.
        // It should be recognized and map to a dedicated BuiltinFn variant.
        assert!(
            recognize_builtin("TURNS").is_some(),
            "TURNS() should be recognized as a built-in function"
        );
    }
}
