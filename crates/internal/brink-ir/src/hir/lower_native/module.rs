//! File-level `@[was("old::module::path")]` → [`HirFile.module`] for the
//! native surface (issue #1286, `docs/decision-log.md` "Native module
//! identity" 2026-07-22 + the 2026-07-23 `@[was]` follow-up ruling).
//!
//! Native module identity is filesystem-derived — a `.brink` file's module is
//! `native_module_path(root-relative path)` (`brink-db::modules`), folded into
//! every definition's `DefinitionId`. Moving the file (or relocating the
//! `brink.toml` root) changes that path, hence every id, hence breaks saves
//! keyed on the old ids. `@[was("old::path")]` is the migration record: it
//! names the module's *previous* path so the analyzer can emit an
//! `AliasEntry { old, new }` (`brink-analyzer::manifest`) mapping each stale
//! `DefinitionId` to its current one.
//!
//! This is the **same** module-rename feature ink already has (`#@was`,
//! `docs/modules-spec.md` §5); only the spelling differs — native names a full
//! `::`-separated module path (a string literal, since `::` is not annotation
//! grammar), where ink names a bare module name. The read path
//! (`brink-db::queries::module_map_query`) and the alias-table codegen
//! (`brink-analyzer::manifest::insert_symbol`) are already wired for both; the
//! only piece this slice adds is parsing the authored record into
//! `HirFile.module.was` so it stops being silently dropped.
//!
//! The produced [`ModuleDecl`] carries an **empty `name`** deliberately: a
//! native file's current module identity is a project-layer, path-derived fact
//! (`module_map_query` stamps it from the file's location and overrides any
//! name here), not something a single-file lowering can know. This node exists
//! only to carry the authored `was` rename record; its `name` is never read for
//! a native file (`module_map_query` reads only `.was`).

use brink_syntax_native::SyntaxKind as N;
use brink_syntax_native::ast::{self, AstNode as _};
use rowan::TextRange;

use crate::hir::FileId;
use crate::{Diagnostic, DiagnosticCode, ModuleDecl};

use super::SyntaxNode;
use super::expr::unescape_string_token;

/// The annotation-line name (`@[was(…)]`) that declares a native module's
/// rename. A bare identifier in the annotation grammar, not a lexer keyword —
/// so no keyword-list bookkeeping is involved (contrast `#@was`, an ink
/// directive tag).
const WAS: &str = "was";

/// `true` if `node` is a file-level `@[was(…)]` annotation line — the one
/// annotation [`super::walk_top_level`] must *not* diagnose as unlowered
/// (`E129`), because [`lower_file_module`] consumes it instead.
pub(super) fn is_was_annotation(node: &SyntaxNode) -> bool {
    node.kind() == N::ANNOTATION_LINE
        && ast::AnnotationLine::cast(node.clone())
            .and_then(|l| l.name_token())
            .is_some_and(|t| t.text() == WAS)
}

/// Scan a native file's top level for a `@[was("old::path")]` annotation and,
/// if one is present, produce the [`ModuleDecl`] carrying its rename record.
///
/// First-one-wins if a file (mistakenly) carries several — the same
/// "first declaration wins" discipline `brink-db::modules::resolve_modules`
/// already applies to a multi-file module's aggregated `was`. A `@[was]` with
/// no quoted old path (empty or a non-string argument) is a malformed
/// migration directive: it is **not** silently dropped (`CLAUDE.md` "Flag
/// silent data drops") but diagnosed `E132` and skipped.
pub(super) fn lower_file_module(
    file_id: FileId,
    source_file: &SyntaxNode,
    diags: &mut Vec<Diagnostic>,
) -> Option<ModuleDecl> {
    let mut found: Option<(String, TextRange)> = None;
    for child in source_file.children() {
        let Some(line) = ast::AnnotationLine::cast(child) else {
            continue;
        };
        if line.name_token().is_none_or(|t| t.text() != WAS) {
            continue;
        }
        let range = line.syntax().text_range();
        match was_old_path(&line) {
            Some(old) if !old.is_empty() => {
                // First wins; a redundant second `@[was]` is left recognized
                // (so `walk_top_level` does not re-diagnose it `E129`) but
                // otherwise ignored.
                found.get_or_insert((old, range));
            }
            _ => diags.push(diag(file_id, range, DiagnosticCode::E132)),
        }
    }
    found.map(|(old, range)| ModuleDecl {
        // Path-derived, project-layer fact — stamped by `module_map_query`,
        // never read from here for a native file (see this module's doc).
        name: String::new(),
        range,
        was: Some((old, range)),
    })
}

/// Extract the old module path from a `@[was("…")]` line: its first argument
/// must be a string literal (`::` path spellings are not annotation-argument
/// grammar, so the path travels as a quoted string). Returns `None` for a
/// missing or non-string argument, which the caller diagnoses `E132`.
fn was_old_path(line: &ast::AnnotationLine) -> Option<String> {
    let arg = line.args()?.args().next()?;
    let string_lit = arg
        .syntax()
        .children()
        .find(|n| n.kind() == N::STRING_LIT)?;
    let mut out = String::new();
    for el in string_lit.children_with_tokens() {
        if let rowan::NodeOrToken::Token(t) = el {
            match t.kind() {
                N::STRING_TEXT => out.push_str(t.text()),
                N::STRING_ESCAPE => out.push_str(unescape_string_token(t.text())),
                // An interpolation (`{expr}`) inside a rename path is
                // meaningless; ignoring it yields an empty/partial path the
                // caller rejects as malformed.
                _ => {}
            }
        }
    }
    Some(out)
}

fn diag(file: FileId, range: TextRange, code: DiagnosticCode) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: code.title().to_string(),
        code,
    }
}
