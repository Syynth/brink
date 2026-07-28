//! `use`/`import` → `HirFile.imports: Vec<Import>` (`docs/b0-sequencing.md`
//! §B0.6: "module/import/use → the module skeleton … here just lower the
//! decl nodes correctly").
//!
//! ink's `Import` shape (`IMPORT { a, b AS c } FROM mod` / `IMPORT mod`,
//! `docs/modules-spec.md` §2) is a flat (module, items, bare) triple with
//! no recursive grouping and no module-level alias. Native's Rust-lifted
//! `use path::{a, b as c};` grammar is structurally closer (a `::`-path
//! plus a flat name/alias item list) but allows one shape ink's `Import`
//! cannot represent at all: recursive nested groups (`use a::{b::{c}}`).
//! That is a judgment call, flagged in the `lower_native` module doc — this
//! slice lowers the shapes that DO fit and emits E129 (loud, not silent) for
//! the ones that don't, rather than guessing a lossy mapping.
//!
//! # What `Import.module` must be (issue #1581)
//!
//! `Import.module` is matched **by string equality** against a real module
//! name — `ImportScope::qualified_modules` / `bare_imports` in resolution and
//! `modules::import_covers` for the `E025` import-required gate. A native
//! module's name is `brink_db::modules::native_module_path` of the file's
//! root-relative path: `market/barter.brink` → `story::market::barter`,
//! `::`-joined. Two properties follow, and both were wrong before #1581 —
//! so no `use` could ever match any module and every native cross-file
//! reference fell through to `lookup_by_name`'s bare-name fallback:
//!
//! - the path is joined with `::`, never `.`; and
//! - the **leaf** segment of `use story::market::barter::haggle;` is the
//!   imported *item*, not part of the module — it belongs in `items`, with
//!   `story::market::barter` (the prefix) as `module`.
//!
//! That leaf-is-an-item reading is the ruled one: "referencing it from
//! another file requires an explicit `use story::…::name`" (decision-log
//! 2026-07-23, "Native visibility: top-level flows default to Private"), and
//! charter §13.2's "imports are naming only". It also makes the previously
//! rejected `use a::b as c;` shape representable — it is an item import with
//! an alias, exactly ink's `IMPORT { b AS c } FROM a`.
//!
//! A **single-segment** `use story;` has no prefix to be the module, so its
//! one segment can only name a module: it stays the qualified form
//! (`bare: false`), matching `import story;`. Aliasing that (`use a as m;`)
//! is a module-level alias, which ink's `Import` has no field for — still
//! `E129`.
//!
//! # Dual-reading is resolved downstream, not here (issue #1592)
//!
//! "Leaf-is-an-item" above is only the *provisional* parse this module
//! produces — it does not decide whether `use story::market::barter;`'s
//! `barter` is really an item of `story::market` or is itself the
//! submodule `story::market::barter`. That question needs whole-project
//! module knowledge this per-file lowering pass never has, so it is left
//! to the analyzer: `brink_analyzer::resolve::import_coverage_for_file`
//! licenses *both* readings unconditionally (harmless when one is
//! phantom), and `brink_analyzer::modules::check`'s `E088` is what
//! actually validates the two readings against real project data and
//! diagnoses when the trailing segment names neither.

use brink_syntax_native::ast::{self, AstNode as _};
use rowan::TextRange;

use crate::hir::FileId;
use crate::{Diagnostic, DiagnosticCode, Import, ImportItem};

use super::SyntaxToken;
use super::decl::joined_path_text;
use super::expr::lower_path;

fn diag(file: FileId, range: TextRange, code: DiagnosticCode) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: code.title().to_string(),
        code,
    }
}

/// Join path-segment tokens into a native module name — `::`-separated, the
/// spelling `brink_db::modules::native_module_path` mints and every module
/// match in the analyzer compares against (see the module doc).
fn module_name(segments: &[SyntaxToken]) -> String {
    segments
        .iter()
        .map(rowan::SyntaxToken::text)
        .collect::<Vec<_>>()
        .join("::")
}

/// The source span covering `segments` (non-empty), for a diagnostic that
/// points at the module path rather than the whole `use` statement.
fn span_of(segments: &[SyntaxToken]) -> Option<TextRange> {
    let first = segments.first()?;
    let last = segments.last()?;
    Some(TextRange::new(
        first.text_range().start(),
        last.text_range().end(),
    ))
}

/// Lower a `use path::{a, b as c};` / `use path::item;` / `use path::item as
/// alias;` / `use module;` declaration. Returns `None` (with a diagnostic
/// already pushed) for the shapes ink's flat `Import` can't represent — see
/// the module doc.
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

    let segments: Vec<SyntaxToken> = tree.path_segments().collect();
    let Some(module_range) = span_of(&segments) else {
        diags.push(diag(file_id, range, DiagnosticCode::E012));
        return None;
    };

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
        // The whole path is the module here — the `{ … }` group holds the
        // items, so no segment is a leaf item.
        return Some(Import {
            module: module_name(&segments),
            module_range,
            items,
            bare: true,
            range,
        });
    }

    // No `{ … }` group: the final segment is the imported *item* and the
    // prefix is its module (issue #1581 — the module must be a real,
    // `::`-joined module name for `ImportScope`/`import_covers` to match).
    let Some((leaf, prefix)) = segments.split_last() else {
        // Unreachable — `span_of` above already rejected an empty path — but
        // a diagnostic beats a silent `None` if that ever changes.
        diags.push(diag(file_id, range, DiagnosticCode::E012));
        return None;
    };
    if prefix.is_empty() {
        if tree.alias_token().is_some() {
            // Module-level aliasing (`use a as m;`) — no field on ink's
            // `Import` to carry it. Loud, not silently dropped.
            diags.push(diag(file_id, range, DiagnosticCode::E129));
            return None;
        }
        // A lone segment has no prefix to be the module, so it names the
        // module itself: the qualified form, same as `import a;`.
        return Some(Import {
            module: leaf.text().to_string(),
            module_range,
            items: Vec::new(),
            bare: false,
            range,
        });
    }

    let alias_token = tree.alias_token();
    let item_range = alias_token.as_ref().map_or_else(
        || leaf.text_range(),
        |alias| TextRange::new(leaf.text_range().start(), alias.text_range().end()),
    );
    Some(Import {
        module: module_name(prefix),
        module_range: span_of(prefix).unwrap_or(module_range),
        items: vec![ImportItem {
            name: leaf.text().to_string(),
            alias: alias_token.map(|t| t.text().to_string()),
            range: item_range,
        }],
        bare: true,
        range,
    })
}

/// Lower an `import path;` declaration. Finding (per `parser/decl.rs`'s own
/// note: "Real semantics (whole-module import vs name-import) are B0.6's
/// call") — B0.6 decides: `import` is the qualified-module form (brings only
/// the module name into scope, licensing any of its public exports). The
/// whole path is the module name, `::`-joined — unlike `use a::b;`, whose
/// leaf is an *item* of module `a` (see [`lower_use_decl`] and the module
/// doc), `import a::b;` is how a native file names the module `a::b` itself.
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
