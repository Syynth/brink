//! Auto-import quick-fix for out-of-scope module references (M-4,
//! docs/modules-spec.md §2/§9).
//!
//! When a file references a public definition that lives in another *declared*
//! module without importing it, the analyzer raises `E025` (import-required).
//! [`ImportFixer`] turns that diagnostic into a [`Fix`] whose one edit inserts
//! `IMPORT { name } FROM module` (ink) or `use module::name;` (native) into
//! the referring file.
//!
//! The offer is session-aware (it needs the whole-project module view to know
//! *which* module exports the name), so it reads the compilation
//! ([`FixCx::db`]) rather than the source alone.
//!
//! **Dialect** (issue #1590 companion finding): the diagnostic that gates this
//! offer is dialect-blind (`brink-analyzer` never tags a `.brink` file — see
//! `brink-db`'s `file_language` doc, "no dialect tag near HIR"), so which
//! syntax to *render* is decided here, the presentation layer, from
//! [`brink_db::ProjectDb::is_native`] — the same sanctioned per-file signal
//! `compilation_closure_files`/`per_file_diagnostics_query` already use for
//! this exact frontend question.
//!
//! [`import_edit`] computes the minimal insertion: it rides the same
//! leading-block insertion machinery as the INCLUDE auto-import
//! ([`crate::auto_import`]), placing the new import after any existing
//! `IMPORT`/`use` block, else after the `INCLUDE` block, else at the top of
//! the file below any leading comment / `#@module` header.

use brink_ir::{Diagnostic, DiagnosticCode, FileId};
use rowan::{TextRange, TextSize};

use crate::fix::{Applicability, Fix, FixCx, Fixer};
use crate::import_block::import_block_span;
use crate::include_block::include_block_span;
use crate::rename::FileEdit;

/// The `E025` import-required fixer (`docs/autofix-spec.md` §9, "Migrated,
/// unchanged in meaning").
///
/// Reads the **module-qualified** db surface ([`brink_db::ProjectDb::diagnostics`] /
/// [`brink_db::ProjectDb::symbol_index`] / [`brink_db::ProjectDb::resolve`]) — the same one that
/// produces the editor's live `E025` squiggle. The whole-project
/// `IdeSession::analysis` snapshot hashes names bare (no module
/// qualification), so it never carries `E025`; keying off it would leave this
/// quick-fix permanently dead.
pub struct ImportFixer;

impl Fixer for ImportFixer {
    fn code(&self) -> DiagnosticCode {
        DiagnosticCode::E025
    }

    /// Adding an import brings a name into scope — mechanical, but it changes
    /// what the file resolves to, so it is not `Safe` under §3's
    /// observable-equivalence bar.
    fn max_applicability(&self) -> Applicability {
        Applicability::Suggested
    }

    fn fixes(&self, cx: &FixCx<'_>, d: &Diagnostic) -> Vec<Fix> {
        let db = cx.db;
        let file_id = d.file;
        let at = d.range.start();

        // The reference's resolution supplies the target's module + name
        // structurally (never by parsing the diagnostic message). Pick the
        // tightest reference range covering the diagnostic's anchor, so a
        // nested reference wins over an enclosing one.
        let Some((resolutions, _)) = db.resolve(file_id) else {
            return Vec::new();
        };
        let index = db.symbol_index();
        let target = resolutions
            .iter()
            .filter(|r| r.range.contains_inclusive(at))
            .min_by_key(|r| r.range.len())
            .and_then(|r| index.symbols.get(&r.target));

        let Some(info) = target else {
            return Vec::new();
        };
        let Some(module) = info.module.clone() else {
            return Vec::new();
        };
        let Some(source) = db.source(file_id) else {
            return Vec::new();
        };
        let Some((offset, new_text)) =
            import_edit(source, &module, &info.name, db.is_native(file_id))
        else {
            return Vec::new();
        };

        vec![Fix {
            code: DiagnosticCode::E025,
            title: format!("Import `{}` from `{module}`", info.name),
            applicability: Applicability::Suggested,
            edits: vec![FileEdit {
                file: file_id,
                range: TextRange::empty(offset),
                new_text,
            }],
            caret: None,
        }]
    }
}

/// The minimal edit that brings `name` in from `module`: the insertion point
/// and the text to insert there. `None` when the exact bare import already
/// exists (an idempotent no-op).
///
/// `native` selects both which frontend parses `source` for the idempotence
/// check and which syntax gets rendered — the two must agree, since parsing
/// a native `use` block with the ink frontend (or vice versa) would silently
/// fail to recognize any existing import (issue #1590 companion finding).
#[must_use]
pub fn import_edit(
    source: &str,
    module: &str,
    name: &str,
    native: bool,
) -> Option<(TextSize, String)> {
    let hir = if native {
        let parsed = brink_syntax_native::parse(source);
        brink_ir::hir::lower_native::lower(FileId(0), &parsed.tree()).0
    } else {
        let parsed = brink_syntax::parse(source);
        brink_ir::hir::lower(FileId(0), &parsed.tree()).0
    };

    // Idempotence: if a bare import already brings this exact name in from
    // this exact module, there is nothing to do.
    let already = hir
        .imports
        .iter()
        .any(|imp| imp.bare && imp.module == module && imp.items.iter().any(|it| it.name == name));
    if already {
        return None;
    }

    let line = if native {
        format!("use {module}::{name};")
    } else {
        format!("IMPORT {{ {name} }} FROM {module}")
    };
    let byte = insertion_byte(&hir, source);
    // The insertion byte is normally the start of a line, so a trailing `\n`
    // makes the IMPORT its own line. When it lands mid-line (a file with no
    // trailing newline whose insertion point is the end of the file), prepend
    // a `\n` so the new IMPORT is not concatenated onto the preceding line.
    let needs_leading_newline = byte > 0 && source.as_bytes().get(byte - 1) != Some(&b'\n');
    let insert = if needs_leading_newline {
        format!("\n{line}\n")
    } else {
        format!("{line}\n")
    };

    Some((
        TextSize::from(u32::try_from(byte).unwrap_or(u32::MAX)),
        insert,
    ))
}

/// The byte offset (at the start of a line) at which to insert the new
/// `IMPORT` line: after an existing `IMPORT` block, else after the `INCLUDE`
/// block, else at the top below any leading comment / `#@module` header.
fn insertion_byte(hir: &brink_ir::HirFile, source: &str) -> usize {
    if let Some(span) = import_block_span(hir, source) {
        line_start_byte(source, span.end_line + 1)
    } else if let Some(span) = include_block_span(hir, source) {
        line_start_byte(source, span.end_line + 1)
    } else {
        line_start_byte(source, leading_header_block_end(source))
    }
}

/// The 0-based line index of the first line that is **not** part of a leading
/// `//` / `///` comment / `#@module` directive / blank-line block.
fn leading_header_block_end(source: &str) -> u32 {
    let mut last_header_plus_one = 0u32;
    for (line, raw) in source.lines().enumerate() {
        let line = u32::try_from(line).unwrap_or(u32::MAX);
        let t = raw.trim_start();
        if t.starts_with("//") || t.starts_with("#@module") {
            last_header_plus_one = line + 1;
        } else if !t.is_empty() {
            break;
        }
    }
    last_header_plus_one
}

/// Byte offset of the start of 0-based `line`. Lines past the end clamp to the
/// end of `source`.
fn line_start_byte(source: &str, line: u32) -> usize {
    if line == 0 {
        return 0;
    }
    let mut seen = 0u32;
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            seen += 1;
            if seen == line {
                return i + 1;
            }
        }
    }
    source.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fix::fixes_at;
    use crate::session::IdeSession;

    fn session_with(files: &[(&str, &str)]) -> IdeSession {
        let mut session = IdeSession::new();
        for (path, src) in files {
            session.update_source(path, (*src).to_string());
        }
        for (path, src) in files {
            session.update_and_analyze(path, (*src).to_string());
        }
        session
    }

    /// Apply [`import_edit`]'s minimal insertion to `source`. The placement
    /// logic under test is `import_edit`'s; this only splices.
    fn applied(source: &str, module: &str, name: &str, native: bool) -> Option<String> {
        let (offset, text) = import_edit(source, module, name, native)?;
        let byte = usize::from(offset);
        let mut out = String::with_capacity(source.len() + text.len());
        out.push_str(&source[..byte]);
        out.push_str(&text);
        out.push_str(&source[byte..]);
        Some(out)
    }

    // ── import_edit (the minimal insertion) ─────────────────────────

    #[test]
    fn inserts_below_existing_import_block() {
        let src = "IMPORT quest_1\nIMPORT quest_2\n== hub ==\n";
        let out = applied(src, "quest_3", "ambush", false).expect("edit");
        assert_eq!(
            out,
            "IMPORT quest_1\nIMPORT quest_2\nIMPORT { ambush } FROM quest_3\n== hub ==\n"
        );
    }

    #[test]
    fn inserts_below_include_block_when_no_imports() {
        let src = "INCLUDE a.ink\nINCLUDE b.ink\n== hub ==\n";
        let out = applied(src, "quest_3", "ambush", false).expect("edit");
        assert_eq!(
            out,
            "INCLUDE a.ink\nINCLUDE b.ink\nIMPORT { ambush } FROM quest_3\n== hub ==\n"
        );
    }

    #[test]
    fn inserts_below_module_header_when_no_blocks() {
        let src = "#@module(town)\n// notes\n== hub ==\n";
        let out = applied(src, "quest_3", "ambush", false).expect("edit");
        assert_eq!(
            out,
            "#@module(town)\n// notes\nIMPORT { ambush } FROM quest_3\n== hub ==\n"
        );
    }

    #[test]
    fn inserts_at_top_when_bare_file() {
        let src = "== hub ==\ntext\n";
        let out = applied(src, "quest_3", "ambush", false).expect("edit");
        assert_eq!(out, "IMPORT { ambush } FROM quest_3\n== hub ==\ntext\n");
    }

    #[test]
    fn idempotent_when_name_already_imported() {
        let src = "IMPORT { ambush, gt } FROM quest_3\n== hub ==\n";
        assert_eq!(import_edit(src, "quest_3", "ambush", false), None);
    }

    #[test]
    fn distinct_name_from_same_module_adds_a_line() {
        // A different name from an already-imported module still needs an edit
        // (a second IMPORT line is legal; merging into the brace is a future
        // refinement).
        let src = "IMPORT { ambush } FROM quest_3\n== hub ==\n";
        let out = applied(src, "quest_3", "guard_talk", false).expect("edit");
        assert_eq!(
            out,
            "IMPORT { ambush } FROM quest_3\nIMPORT { guard_talk } FROM quest_3\n== hub ==\n"
        );
    }

    #[test]
    fn insertion_without_trailing_newline_stays_on_its_own_line() {
        let src = "== hub ==";
        let out = applied(src, "quest_3", "ambush", false).expect("edit");
        assert_eq!(out, "IMPORT { ambush } FROM quest_3\n== hub ==");
    }

    // ── import_edit: native dialect (issue #1590 companion finding) ──

    /// `native: true` renders `use module::name;`, not ink's `IMPORT { … }
    /// FROM …` — the exact gap the companion finding calls out ("do not
    /// leave it rendering ink syntax to native authors").
    #[test]
    fn native_insert_renders_use_syntax() {
        let src = "flow start() {\n  Hi\n}\n";
        let out = applied(src, "story::market::barter", "haggle", true).expect("edit");
        assert_eq!(
            out,
            "use story::market::barter::haggle;\nflow start() {\n  Hi\n}\n"
        );
    }

    /// `native: true` must parse `source` with the native frontend for the
    /// idempotence check — parsing a native `use` with the ink frontend would
    /// recognize no imports at all and duplicate the line every time.
    #[test]
    fn native_insert_is_idempotent_against_native_syntax() {
        let src = "use story::market::barter::haggle;\nflow start() {\n  Hi\n}\n";
        assert_eq!(
            import_edit(src, "story::market::barter", "haggle", true),
            None
        );
    }

    /// A native-flagged insert for a *different* module than an existing
    /// `use` line is not coverage (the idempotence check only skips when the
    /// exact module/name pair is already imported) — the new `use` line is
    /// appended below the existing one, not before it or in place of it.
    #[test]
    fn native_insert_below_existing_use_block() {
        let src = "use story::market::barter::haggle;\nflow start() {\n  Hi\n}\n";
        let out = applied(src, "story::docks::barter", "haggle", true).expect("edit");
        assert_eq!(
            out,
            "use story::market::barter::haggle;\nuse story::docks::barter::haggle;\nflow start() {\n  Hi\n}\n"
        );
    }

    // ── ImportFixer (session-aware detection, through `fixes_at`) ────

    /// Two declared modules; `town` references `quest`'s public `ambush`
    /// without importing it → `E025` → the fixer offers an insertion.
    #[test]
    fn offers_add_import_for_out_of_scope_reference() {
        let session = session_with(&[
            (
                "quest.ink",
                "#@module(quest)\n== ambush ==\n#@public\nGotcha!\n-> DONE\n",
            ),
            ("town.ink", "#@module(town)\n== square ==\nHi\n-> ambush\n"),
        ]);
        let town = session.file_id("town.ink").expect("town id");
        let src = session.source(town).expect("src");
        let off = u32::try_from(src.find("ambush").expect("ref")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, town, off);
        assert_eq!(fixes.len(), 1, "one add-import offer");
        assert_eq!(fixes[0].title, "Import `ambush` from `quest`");
        assert_eq!(fixes[0].code, DiagnosticCode::E025);
        assert_eq!(fixes[0].applicability, Applicability::Suggested);
        assert_eq!(fixes[0].edits.len(), 1);
        assert_eq!(fixes[0].edits[0].file, town);
        assert!(
            fixes[0].edits[0].range.is_empty(),
            "an insertion replaces nothing: {:?}",
            fixes[0].edits[0].range
        );
        assert_eq!(fixes[0].edits[0].new_text, "IMPORT { ambush } FROM quest\n");
    }

    /// The fixer reads `db.is_native` per referring file (issue #1590
    /// companion finding) — a `.brink` referrer must get `use` syntax, not
    /// `IMPORT`.
    #[test]
    fn offers_add_import_with_use_syntax_for_native_referrer() {
        let session = session_with(&[
            (
                "quest.ink",
                "#@module(quest)\n== ambush ==\n#@public\nGotcha!\n-> DONE\n",
            ),
            ("market/barter.brink", "flow start() {\n  -> ambush\n}\n"),
        ]);
        let file = session.file_id("market/barter.brink").expect("file id");
        let src = session.source(file).expect("src");
        let off = u32::try_from(src.find("ambush").expect("ref")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(fixes.len(), 1, "one add-import offer");
        assert_eq!(fixes[0].edits[0].new_text, "use quest::ambush;\n");
    }

    #[test]
    fn no_offer_where_there_is_no_diagnostic() {
        let session = session_with(&[("town.ink", "#@module(town)\n== hub ==\nHi\n-> hub\n")]);
        let town = session.file_id("town.ink").expect("town id");
        let src = session.source(town).expect("src");
        let off = u32::try_from(src.find("hub").expect("ref")).expect("fits");
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, town, off).is_empty());
    }
}
