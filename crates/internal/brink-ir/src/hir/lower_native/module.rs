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
//! `docs/modules-spec.md` §5); only the spelling differs — native names a
//! full `::`-separated module path, where ink names a bare module name.
//! The read path (`brink-db::queries::module_map_query`) and the
//! alias-table codegen (`brink-analyzer::manifest::insert_symbol`) are
//! already wired for both; the only piece this slice adds is parsing the
//! authored record into `HirFile.module.was` so it stops being silently
//! dropped.
//!
//! The path travels in **either** of two spellings and [`was_old_path`]
//! accepts both: the original quoted string (`@[was("old::path")]`) and the
//! unquoted `::`-path form `brink-syntax-native`'s annotation-arg grammar
//! gained in issue #1349 (`@[was(story::old::path)]`, `AnnotationArg::path`).
//! Wiring the unquoted shape in (issue #1355) is what makes the #1349
//! grammar addition actually usable — until then it parsed cleanly but still
//! diagnosed `E132` here.
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

/// Scan a native file's top level for a `@[was(…)]` annotation — either the
/// quoted `@[was("old::path")]` or unquoted `@[was(old::path)]` spelling —
/// and, if one is present, produce the [`ModuleDecl`] carrying its rename
/// record.
///
/// First-one-wins if a file (mistakenly) carries several — the same
/// "first declaration wins" discipline `brink-db::modules::resolve_modules`
/// already applies to a multi-file module's aggregated `was`. A `@[was]`
/// with no recognizable old path (empty, or an argument that is neither a
/// string literal nor a `::`-path) is a malformed migration directive: it
/// is **not** silently dropped (`CLAUDE.md` "Flag silent data drops") but
/// diagnosed `E132` and skipped.
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
                // (`annotation::handle_line` treats every file-level `@[was]`
                // as a consumed placement) but otherwise ignored.
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

/// Extract the old module path from a `@[was(…)]` line: its first argument
/// must be either a quoted string literal (`@[was("old::path")]`) or the
/// unquoted `::`-path form (`@[was(old::path)]`, issue #1349). Returns `None`
/// for a missing or otherwise-shaped argument, which the caller diagnoses
/// `E132`.
fn was_old_path(line: &ast::AnnotationLine) -> Option<String> {
    let arg = line.args()?.args().next()?;
    if let Some(string_lit) = arg.syntax().children().find(|n| n.kind() == N::STRING_LIT) {
        return Some(string_lit_text(&string_lit));
    }
    path_arg_text(&arg)
}

/// Unescape a `STRING_LIT` node's contents (the quoted `@[was("…")]` form).
fn string_lit_text(string_lit: &SyntaxNode) -> String {
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
    out
}

/// Join an unquoted `::`-path arg's segments (the `@[was(old::path)]` form,
/// issue #1349's `AnnotationArg::path`) back into the same `"::"`-separated
/// spelling the quoted form produces, so both spellings feed
/// [`lower_file_module`] identically. `None` if the arg isn't this shape at
/// all (e.g. a bare-ident arg with no `::`, or a nested-args clause).
fn path_arg_text(arg: &ast::AnnotationArg) -> Option<String> {
    let path = arg.path()?;
    let segments: Vec<String> = path.segments().map(|t| t.text().to_string()).collect();
    if segments.is_empty() {
        return None;
    }
    Some(segments.join("::"))
}

fn diag(file: FileId, range: TextRange, code: DiagnosticCode) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: code.title().to_string(),
        code,
    }
}
