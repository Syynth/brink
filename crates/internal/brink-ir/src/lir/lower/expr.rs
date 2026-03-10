use crate::hir;
use crate::symbols::SymbolKind;

use super::context::LowerCtx;
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
        match info.kind {
            SymbolKind::Variable | SymbolKind::Constant | SymbolKind::List => {
                lir::Expr::GetGlobal(info.id)
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
            // Params/temps should already be caught by temp_slot above;
            // externals used as values are meaningless — fall back to null.
            SymbolKind::External | SymbolKind::Param | SymbolKind::Temp => lir::Expr::Null,
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

    // Resolve via resolution map
    if let Some(info) = ctx.resolve_path(path.range) {
        let call_args = lower_call_args(args, &info.params, ctx);

        match info.kind {
            SymbolKind::External => lir::Expr::CallExternal {
                target: info.id,
                args: call_args,
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "ink externals have <=255 params"
                )]
                arg_count: info.params.len() as u8,
            },
            _ => lir::Expr::Call {
                target: info.id,
                args: call_args,
            },
        }
    } else {
        lir::Expr::Null
    }
}

fn lower_call_args(
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
                        if let Some(id) = ctx.resolve_id(path.range) {
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
}
