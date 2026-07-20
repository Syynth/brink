//! `use`/`import` → `HirFile.imports: Vec<Import>` (`docs/b0-sequencing.md`
//! §B0.6: "module/import/use → the module skeleton … here just lower the
//! decl nodes correctly").
//!
//! ink's `Import` shape (`IMPORT { a, b AS c } FROM mod` / `IMPORT mod`,
//! `docs/modules-spec.md` §2) is a flat (module, items, bare) triple with
//! no recursive grouping and no module-level alias. Native's Rust-lifted
//! `use path::{a, b as c};` grammar is structurally closer (a dotted path
//! plus a flat name/alias item list) but allows two shapes ink's `Import`
//! cannot represent at all: a module-level alias (`use a::b as m;`, no
//! `{}` list) and recursive nested groups (`use a::{b::{c}}`). Both are
//! judgment calls, flagged in the `lower_native` module doc — this slice
//! lowers the shapes that DO fit and emits E129 (loud, not silent) for the
//! ones that don't, rather than guessing a lossy mapping.

use brink_syntax_native::ast::{self, AstNode as _};

use crate::hir::FileId;
use crate::{Diagnostic, DiagnosticCode, Import, ImportItem};

use super::decl::joined_path_text;
use super::expr::lower_path;

fn diag(file: FileId, range: rowan::TextRange, code: DiagnosticCode) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: code.title().to_string(),
        code,
    }
}

/// Lower a `use path::{a, b as c};` / `use path;` / `use path as alias;`
/// declaration. Returns `None` (with a diagnostic already pushed) for the
/// two shapes ink's flat `Import` can't represent — see the module doc.
pub(super) fn lower_use_decl(
    file_id: FileId,
    node: &ast::UseDecl,
    diags: &mut Vec<Diagnostic>,
) -> Option<Import> {
    let range = node.syntax().text_range();
    let Some(tree) = node.tree() else {
        diags.push(diag(file_id, range, DiagnosticCode::E012));
        return None;
    };

    let module: Vec<String> = tree.path_segments().map(|t| t.text().to_string()).collect();
    if module.is_empty() {
        diags.push(diag(file_id, range, DiagnosticCode::E012));
        return None;
    }
    let module_range = tree.syntax().text_range();
    let module_name = module.join(".");

    if let Some(list) = tree.nested_list() {
        if tree.alias_token().is_some() {
            // `use a::b as m::{…}` isn't reachable from this grammar (the
            // parser only accepts `as` OR a nested list per tree, never
            // both — see `parser/decl.rs::use_tree`), kept as a defensive
            // diagnostic rather than an unreachable!().
            diags.push(diag(file_id, range, DiagnosticCode::E129));
            return None;
        }
        let mut items = Vec::new();
        let mut ok = true;
        for item_tree in list.trees() {
            if item_tree.nested_list().is_some() {
                // Recursive nested groups have no flat `ImportItem` shape.
                diags.push(diag(
                    file_id,
                    item_tree.syntax().text_range(),
                    DiagnosticCode::E129,
                ));
                ok = false;
                continue;
            }
            let segs: Vec<String> = item_tree
                .path_segments()
                .map(|t| t.text().to_string())
                .collect();
            if segs.len() != 1 {
                // A multi-segment path inside a `{ … }` group (`use a::{b::c}`,
                // no nested `{}`) has no `ImportItem` shape either — it names
                // a path, not a single importable item.
                diags.push(diag(
                    file_id,
                    item_tree.syntax().text_range(),
                    DiagnosticCode::E129,
                ));
                ok = false;
                continue;
            }
            let alias = item_tree.alias_token().map(|t| t.text().to_string());
            items.push(ImportItem {
                name: segs[0].clone(),
                alias,
                range: item_tree.syntax().text_range(),
            });
        }
        if !ok && items.is_empty() {
            return None;
        }
        return Some(Import {
            module: module_name,
            module_range,
            items,
            bare: true,
            range,
        });
    }

    if tree.alias_token().is_some() {
        // Module-level aliasing (`use a::b as m;`) — no field on ink's
        // `Import` to carry it. Loud, not silently dropped.
        diags.push(diag(file_id, range, DiagnosticCode::E129));
        return None;
    }

    Some(Import {
        module: module_name,
        module_range,
        items: Vec::new(),
        bare: false,
        range,
    })
}

/// Lower an `import name;` declaration. Finding (per `parser/decl.rs`'s own
/// note: "Real semantics (whole-module import vs name-import) are B0.6's
/// call") — B0.6 decides: `import` is the qualified-module form, matching
/// `use path;` with no item list (brings only the module name into scope).
pub(super) fn lower_import_decl(
    file_id: FileId,
    node: &ast::ImportDecl,
    diags: &mut Vec<Diagnostic>,
) -> Option<Import> {
    let range = node.syntax().text_range();
    let Some(path) = node.path() else {
        diags.push(diag(file_id, range, DiagnosticCode::E012));
        return None;
    };
    let module_name = joined_path_text(&path);
    if module_name.is_empty() {
        diags.push(diag(file_id, range, DiagnosticCode::E012));
        return None;
    }
    let module_range = lower_path(&path).range;
    Some(Import {
        module: module_name,
        module_range,
        items: Vec::new(),
        bare: false,
        range,
    })
}
