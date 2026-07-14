use crate::hir;
use crate::symbols::SymbolKind;

use super::context::LowerCtx;
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
    }
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
/// §6). Every initializer is lowered — and therefore evaluated — exactly
/// once, in **source** order (the order the author wrote them), regardless
/// of which path below is taken.
///
/// - **Well-formed** (every declared field has exactly one initializer, no
///   extra names): reorders the *already-lowered* expression trees into the
///   shape's *declaration* order for `RecordNew` (the VM's required push
///   order). **Evaluation-order caveat**: because each field's `lir::Expr`
///   is placed, not re-evaluated, codegen will evaluate them in shape order
///   at emission time, not source order — when the author's field order
///   differs from the shape's declared order *and* more than one
///   initializer has an observable side effect (a function call, an
///   external, `TURNS_SINCE`, …), those side effects fire in shape order.
///   The LIR has no local-binding expression node (an `Expr::Let`-shaped
///   construct) to stage a source-order-evaluated value ahead of a later
///   shape-order placement without one — introducing one is out of TM-4c's
///   scope (flagged in the PR description's scope notes). Field
///   initializers with observable side effects are expected to be rare in
///   practice; this divergence is deliberate and documented, not a silent
///   miscompile — construction *values* are always correct regardless.
/// - **Mismatched** (a missing declared field, or an initializer for a name
///   the shape doesn't declare): value-model-spec §11c's gradual
///   construction-fault path. Reachable under `types = gradual` (the only
///   policy that compiles this far — under `types = strict` it's already
///   `E069`/`E070`, a compile error, unless that diagnostic was suppressed,
///   in which case this is the non-suppressible runtime backstop). Emits
///   every supplied initializer (source order, still evaluated for its side
///   effects) followed by `RecordNew(CONSTRUCTION_FAULT_SHAPE_ID)` — the VM's
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

    let mut placed: Vec<Option<lir::Expr>> = vec![None; shape.fields.len()];
    let mut source_order: Vec<lir::Expr> = Vec::with_capacity(sl.fields.len());
    let mut has_extra = false;
    for (name, val) in &sl.fields {
        let lowered = lower_expr(val, ctx);
        match shape.field(&name.text) {
            Some((offset, _)) => {
                if let Some(slot) = placed.get_mut(offset as usize) {
                    *slot = Some(lowered.clone());
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
        };
    }

    // `has_missing == false` just proved every slot is `Some` — `unwrap_or`
    // rather than `unwrap` anyway (denied in production code; guarded, not
    // asserted, per the E053-backstop lesson): a future refactor that
    // weakens that proof degrades to a well-formed-but-wrong `Null` field
    // instead of a panic.
    lir::Expr::RecordNew {
        shape_id: shape.id,
        fields: placed
            .into_iter()
            .map(|f| f.unwrap_or(lir::Expr::Null))
            .collect(),
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
/// `push`/`insert`/`remove` (the mutators) are statement-only — recognized
/// and fully lowered by `blocks::try_lower_mutator_stmt` *before* a call
/// expression ever reaches here. Reaching here with one of those three names
/// means the author used it in expression position (`~ x = push(a, v)`),
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
        "push" | "insert" | "remove" => {
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
        "float" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::ConvertFloat(Box::new(lower_expr(&args[0], ctx))))
        }
        "string" => {
            if !arity_ok(ctx, 1) {
                return Some(lir::Expr::Null);
            }
            Some(lir::Expr::ConvertString(Box::new(lower_expr(
                &args[0], ctx,
            ))))
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
            | "int"
            | "float"
            | "string"
            | "call"
            | "bind"
    )
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
                    hir::Expr::Path(path) => {
                        let name = path_to_string(path);
                        if let Some(slot) = ctx.temp_slot(&name) {
                            let name_id = ctx.names.intern(&name);
                            return lir::CallArg::RefTemp(slot, name_id);
                        }
                        if let Some(info) = ctx.resolve_path(path.range) {
                            // A T1b block-scoped temp (`~ { … }`) passed by
                            // `ref` after its own block has closed — same
                            // #680 RCA as `lower_path`'s `SymbolKind::Temp`
                            // arm. Without this check `info.id` (the temp's
                            // `LocalVar`-tagged id, never registered as a
                            // real global) would be emitted as a
                            // `RefGlobal`, faulting at runtime as
                            // `UnresolvedGlobal` with no compile diagnostic.
                            if info.kind == SymbolKind::Temp
                                && ctx.block_scoped_temp_names.contains(&name)
                            {
                                ctx.diagnostics.push(crate::Diagnostic {
                                    file: ctx.file,
                                    range: path.range,
                                    message: format!(
                                        "{}: `{name}` was declared in a `~ {{ … }}` block that \
                                         has already closed — block-scoped temps \
                                         (docs/t1b-surface-spec.md §2) are only visible for \
                                         the rest of their own block",
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
                        lir::CallArg::Value(lower_expr(arg, ctx))
                    }
                    _ => lir::CallArg::Value(lower_expr(arg, ctx)),
                }
            } else {
                lir::CallArg::Value(lower_expr(arg, ctx))
            }
        })
        .collect()
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
