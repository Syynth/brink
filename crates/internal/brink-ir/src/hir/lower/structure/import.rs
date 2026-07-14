//! Import lowering: `lower_import` (M-2, docs/modules-spec.md §2).

use brink_syntax::ast::{self, AstNode};

use crate::{Import, ImportItem};

/// Lower one `IMPORT` statement AST node to its HIR form.
///
/// Both spellings collapse to the same shape: the qualified form
/// (`IMPORT mod`) has an empty `items` list and `bare == false`; the bare
/// form (`IMPORT { a, b AS c } FROM mod`) carries the items and `bare ==
/// true`. Well-formedness (unresolved / duplicate / self import) is checked
/// downstream in the analyzer, where the whole-project module view exists.
pub(super) fn lower_import(stmt: &ast::ImportStmt) -> Import {
    let range = stmt.syntax().text_range();
    let module_node = stmt.module();
    let module = module_node
        .as_ref()
        .and_then(ast::ImportModule::name)
        .unwrap_or_default();
    let module_range = module_node
        .as_ref()
        .map_or(range, |m| m.syntax().text_range());

    let (items, bare) = match stmt.list() {
        Some(list) => {
            let items = list
                .items()
                .filter_map(|item| {
                    item.name().map(|name| ImportItem {
                        name,
                        alias: item.alias(),
                        range: item.syntax().text_range(),
                    })
                })
                .collect();
            (items, true)
        }
        None => (Vec::new(), false),
    };

    Import {
        module,
        module_range,
        items,
        bare,
        range,
    }
}
