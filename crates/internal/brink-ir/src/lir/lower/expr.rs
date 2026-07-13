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

        // TM-4b structs (docs/typed-mode-spec.md §6): grammar+HIR+analyzer
        // land in this slice, codegen with TM-4c (#666) — see
        // `reject_struct_construct`'s doc for the T1b-1/E053-backstop
        // discipline this follows (a real diagnostic, not a silent
        // Null/drop). Still walks children so any nested diagnostics
        // (unresolved refs, etc.) inside a rejected construct still surface.
        hir::Expr::StructLiteral(sl) => {
            for (_name, val) in &sl.fields {
                lower_expr(val, ctx);
            }
            reject_struct_construct(sl.ptr.text_range(), ctx)
        }
        hir::Expr::FieldAccess(fa) => {
            lower_expr(&fa.base, ctx);
            reject_struct_construct(fa.ptr.text_range(), ctx)
        }
    }
}

/// Non-suppressible backstop for a struct construct reaching LIR lowering
/// (TM-4b, docs/typed-mode-spec.md §6) — the T1b-1 discipline (grammar/HIR/
/// analyzer land before codegen; LIR lowering rejects) plus the E053-backstop
/// lesson (#572 review: a `debug_assert!`-guarded stub silently drops/
/// corrupts data in release builds if the analyzer's dialect-gate diagnostic
/// is suppressed via `// brink-disable-all`). This pushes a real,
/// Error-severity `Diagnostic` into `ctx.diagnostics` — `brink-db`'s
/// `lir_query` partitions LIR diagnostics by severity independently of
/// analysis-phase suppression, so this fires unconditionally, in both
/// dialects, suppressed or not.
fn reject_struct_construct(range: rowan::TextRange, ctx: &mut LowerCtx<'_>) -> lir::Expr {
    ctx.diagnostics.push(crate::Diagnostic {
        file: ctx.file,
        range,
        message: crate::DiagnosticCode::E072.title().to_string(),
        code: crate::DiagnosticCode::E072,
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
/// array, map) makes the whole map literal non-constant — it falls back to
/// the `MapNew` runtime path, where key-domain validation happens at
/// `MapNew` construction time (a turn-terminating fault) instead.
fn const_value_to_map_key(v: lir::ConstValue) -> Option<lir::ConstMapKey> {
    match v {
        lir::ConstValue::Int(n) => Some(lir::ConstMapKey::Int(n)),
        lir::ConstValue::String(s) => Some(lir::ConstMapKey::Str(s)),
        lir::ConstValue::Bool(b) => Some(lir::ConstMapKey::Bool(b)),
        _ => None,
    }
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
        // static dotted path. Codegen for this isn't ready (TM-4c) — reject
        // rather than silently loading `p` itself and dropping `.x` (the
        // exact silent-data-drop hazard the E053-backstop lesson warns
        // about; see `reject_struct_construct`).
        if path.segments.len() > 1
            && matches!(
                info.kind,
                SymbolKind::Variable | SymbolKind::Constant | SymbolKind::Param | SymbolKind::Temp
            )
        {
            return reject_struct_construct(path.range, ctx);
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
            // Temps not caught by temp_slot above are forward-referenced
            // (used before their declaration). Inklecate emits a get_global
            // for these, which fails at link time because no such global
            // exists. We reproduce the same behavior so the linker errors.
            // Hash the variable name the same way the converter does
            // (DefaultHasher on the name string → GlobalVar tag).
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
        _ => None,
    }
}

/// The T1b stdlib slice 1 function names (`docs/t1b-surface-spec.md` §5),
/// brink-dialect-gated free functions. Kept in sync by hand with
/// `brink_analyzer::resolve::is_t1b_stdlib_name` — the two crates don't
/// share a dependency edge for this purpose (mirrors the existing
/// `recognize_builtin`/`is_builtin_function` split for the classic uppercase
/// ink intrinsics).
pub(crate) fn is_t1b_stdlib_name(name: &str) -> bool {
    matches!(
        name,
        "len" | "keys" | "values" | "contains" | "push" | "insert" | "remove"
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
