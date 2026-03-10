use brink_format::DefinitionId;

use crate::FileId;
use crate::hir;
use crate::symbols::{SymbolIndex, SymbolKind};

use super::context::{NameTable, ResolutionLookup};
use super::lir;

/// Collect global variable/constant definitions from HIR files.
///
/// Evaluates constants first so that variable initializers like `VAR x = c`
/// can resolve constant references to their values.
pub fn collect_globals(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    names: &mut NameTable,
    resolutions: &ResolutionLookup,
) -> Vec<lir::GlobalDef> {
    use std::collections::HashMap;

    // Pass 1: evaluate all constants and build a value lookup.
    let mut const_values: HashMap<DefinitionId, lir::ConstValue> = HashMap::new();
    let mut globals = Vec::new();

    for &(file_id, hir_file) in files {
        for cst in &hir_file.constants {
            if let Some(id) = lookup_global(index, &cst.name.text, SymbolKind::Constant) {
                let name = names.intern(&cst.name.text);
                let default =
                    eval_const_expr(&cst.value, index, resolutions, file_id, &const_values);
                const_values.insert(id, default.clone());
                globals.push(lir::GlobalDef {
                    id,
                    name,
                    mutable: false,
                    default,
                });
            }
        }
    }

    // Pass 2: evaluate variables (may reference constants).
    for &(file_id, hir_file) in files {
        for var in &hir_file.variables {
            if let Some(id) = lookup_global(index, &var.name.text, SymbolKind::Variable) {
                let name = names.intern(&var.name.text);
                let default =
                    eval_const_expr(&var.value, index, resolutions, file_id, &const_values);
                globals.push(lir::GlobalDef {
                    id,
                    name,
                    mutable: true,
                    default,
                });
            }
        }
    }

    globals
}

/// Collect list definitions and items from HIR files.
pub fn collect_lists(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    names: &mut NameTable,
) -> (Vec<lir::ListDef>, Vec<lir::ListItemDef>) {
    let mut lists = Vec::new();
    let mut items = Vec::new();

    for &(_file_id, hir_file) in files {
        for list_decl in &hir_file.lists {
            let Some(list_id) = lookup_global(index, &list_decl.name.text, SymbolKind::List) else {
                continue;
            };
            let list_name = names.intern(&list_decl.name.text);

            let mut list_items = Vec::new();
            let mut next_ordinal = 1i32;

            for member in &list_decl.members {
                let ordinal = member.value.unwrap_or(next_ordinal);
                next_ordinal = ordinal + 1;

                let qualified = format!("{}.{}", list_decl.name.text, member.name.text);
                let item_name = names.intern(&qualified);

                if let Some(item_id) = lookup_global(index, &qualified, SymbolKind::ListItem) {
                    list_items.push((item_name, ordinal));
                    items.push(lir::ListItemDef {
                        id: item_id,
                        name: item_name,
                        origin: list_id,
                        ordinal,
                    });
                }
            }

            lists.push(lir::ListDef {
                id: list_id,
                name: list_name,
                items: list_items,
            });
        }
    }

    (lists, items)
}

/// Collect external function declarations from HIR files.
pub fn collect_externals(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    names: &mut NameTable,
) -> Vec<lir::ExternalDef> {
    let mut externals = Vec::new();

    for &(_file_id, hir_file) in files {
        for ext in &hir_file.externals {
            if let Some(id) = lookup_global(index, &ext.name.text, SymbolKind::External) {
                let name = names.intern(&ext.name.text);
                externals.push(lir::ExternalDef {
                    id,
                    name,
                    arg_count: ext.param_count,
                    fallback: None,
                });
            }
        }
    }

    externals
}

fn lookup_global(index: &SymbolIndex, name: &str, kind: SymbolKind) -> Option<DefinitionId> {
    index.by_name.get(name).and_then(|ids| {
        ids.iter()
            .find(|&&id| index.symbols.get(&id).is_some_and(|info| info.kind == kind))
            .copied()
    })
}

/// Evaluate a compile-time constant expression.
#[expect(
    clippy::cast_possible_truncation,
    reason = "f64→f32 is intentional per ink spec"
)]
pub fn eval_const_expr(
    expr: &hir::Expr,
    index: &SymbolIndex,
    resolutions: &ResolutionLookup,
    file: FileId,
    const_values: &std::collections::HashMap<DefinitionId, lir::ConstValue>,
) -> lir::ConstValue {
    match expr {
        hir::Expr::Int(n) => lir::ConstValue::Int(*n),
        hir::Expr::Float(bits) => lir::ConstValue::Float(bits.to_f64() as f32),
        hir::Expr::Bool(b) => lir::ConstValue::Bool(*b),
        hir::Expr::String(s) => {
            let text: String = s
                .parts
                .iter()
                .filter_map(|p| match p {
                    hir::StringPart::Literal(t) => Some(t.as_str()),
                    hir::StringPart::Interpolation(_) => None,
                })
                .collect();
            lir::ConstValue::String(text)
        }
        hir::Expr::Prefix(hir::PrefixOp::Negate, inner) => {
            match eval_const_expr(inner, index, resolutions, file, const_values) {
                lir::ConstValue::Int(n) => lir::ConstValue::Int(-n),
                lir::ConstValue::Float(f) => lir::ConstValue::Float(-f),
                _ => lir::ConstValue::Null,
            }
        }
        hir::Expr::Path(path) => {
            if let Some(id) = resolutions.resolve(file, path.range) {
                if let Some(info) = index.symbols.get(&id) {
                    match info.kind {
                        SymbolKind::ListItem => lir::ConstValue::List {
                            items: vec![id],
                            origins: vec![],
                        },
                        SymbolKind::Constant => const_values
                            .get(&id)
                            .cloned()
                            .unwrap_or(lir::ConstValue::Null),
                        SymbolKind::Variable => lir::ConstValue::Null,
                        _ => lir::ConstValue::DivertTarget(id),
                    }
                } else {
                    lir::ConstValue::Null
                }
            } else {
                lir::ConstValue::Null
            }
        }
        hir::Expr::DivertTarget(path) => {
            if let Some(id) = resolutions.resolve(file, path.range) {
                lir::ConstValue::DivertTarget(id)
            } else {
                lir::ConstValue::Null
            }
        }
        hir::Expr::ListLiteral(paths) => {
            let mut items = Vec::new();
            let mut origins = Vec::new();
            for path in paths {
                if let Some(id) = resolutions.resolve(file, path.range)
                    && let Some(info) = index.symbols.get(&id)
                {
                    if info.kind == SymbolKind::ListItem {
                        items.push(id);
                    } else if info.kind == SymbolKind::List {
                        origins.push(id);
                    }
                }
            }
            lir::ConstValue::List { items, origins }
        }
        _ => lir::ConstValue::Null,
    }
}
