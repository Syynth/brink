//! Auto-import quick-fix for out-of-scope module references (M-4,
//! docs/modules-spec.md §2/§9).
//!
//! When a file references a public definition that lives in another *declared*
//! module without importing it, the analyzer raises `E025` (import-required).
//! [`import_actions`] turns that diagnostic into a quick-fix: an
//! [`AddImport`](crate::code_actions::CodeActionData::AddImport) code action
//! that inserts `IMPORT { name } FROM module` (ink) or `use module::name;`
//! (native) into the referring file.
//!
//! The offer is session-aware (it needs the whole-project module view to know
//! *which* module exports the name), so it is computed here rather than in the
//! source-only [`code_actions`](crate::code_actions::code_actions) path — the
//! wasm layer merges it into the same code-action menu.
//!
//! **Dialect** (issue #1590 companion finding): the diagnostic that gates this
//! offer is dialect-blind (`brink-analyzer` never tags a `.brink` file — see
//! `brink-db`'s `file_language` doc, "no dialect tag near HIR"), so which
//! syntax to *render* is decided here, the presentation layer, from
//! [`ProjectDb::is_native`] — the same sanctioned per-file signal
//! `compilation_closure_files`/`per_file_diagnostics_query` already use for
//! this exact frontend question.
//!
//! Resolution ([`insert_import`]) is a pure source rewrite: it rides the same
//! leading-block insertion machinery as the INCLUDE auto-import
//! ([`crate::auto_import`]), placing the new import after any existing
//! `IMPORT`/`use` block, else after the `INCLUDE` block, else at the top of
//! the file below any leading comment / `#@module` header.

use brink_db::ProjectDb;
use brink_ir::{DiagnosticCode, FileId};

use crate::code_actions::{CodeAction, CodeActionData, CodeActionKind};
use crate::import_block::import_block_span;
use crate::include_block::include_block_span;

/// Collect auto-import quick-fixes applicable at `offset` in `file_id`.
///
/// Returns an [`AddImport`](CodeActionData::AddImport) action for the
/// `(module, name)` an `E025` at `offset` calls for. Empty when there is no
/// import-required diagnostic at the cursor (the common case), so the caller
/// can unconditionally merge the result into its menu.
///
/// Takes the [`ProjectDb`] directly (not an `IdeSession`) so both the wasm
/// editor (`IdeSession::db`) and the LSP (its own locked db) can call it.
///
/// The gate reads the **module-qualified** db surface
/// ([`ProjectDb::diagnostics`] / [`ProjectDb::symbol_index`] /
/// [`ProjectDb::resolve`]) — the same one that produces the editor's live
/// `E025` squiggle. The whole-project `IdeSession::analysis` snapshot hashes
/// names bare (no module qualification), so it never carries `E025`; gating on
/// it would leave this quick-fix permanently dead.
#[must_use]
pub fn import_actions(db: &ProjectDb, file_id: FileId, offset: u32) -> Vec<CodeAction> {
    let at = rowan::TextSize::from(offset);

    // Gate the offer on the analyzer's own import-required diagnostic — it,
    // not this function, owns the module-membership + import-coverage rules.
    // `diagnostics(file_id)` is already scoped to this file.
    let has_import_required = db.diagnostics(file_id).is_some_and(|diags| {
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::E025 && d.range.contains_inclusive(at))
    });
    if !has_import_required {
        return Vec::new();
    }

    // The reference's resolution supplies the target's module + name
    // structurally (never by parsing the diagnostic message). Pick the
    // tightest reference range covering the cursor, so a nested reference wins
    // over an enclosing one.
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

    vec![CodeAction {
        title: format!("Import `{}` from `{module}`", info.name),
        kind: CodeActionKind::QuickFix,
        data: CodeActionData::AddImport {
            module,
            name: info.name.clone(),
            native: db.is_native(file_id),
        },
    }]
}

/// Insert `IMPORT { name } FROM module` (ink) or `use module::name;`
/// (`native: true`) into `source`, returning the new source. Returns `None`
/// when the exact bare import already exists (an idempotent no-op).
///
/// `native` selects both which frontend parses `source` for the idempotence
/// check and which syntax gets rendered — the two must agree, since parsing
/// a native `use` block with the ink frontend (or vice versa) would silently
/// fail to recognize any existing import (issue #1590 companion finding).
#[must_use]
pub fn insert_import(source: &str, module: &str, name: &str, native: bool) -> Option<String> {
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

    let mut out = String::with_capacity(source.len() + insert.len());
    out.push_str(&source[..byte]);
    out.push_str(&insert);
    out.push_str(&source[byte..]);
    Some(out)
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

    // ── insert_import (pure source rewrite) ─────────────────────────

    #[test]
    fn inserts_below_existing_import_block() {
        let src = "IMPORT quest_1\nIMPORT quest_2\n== hub ==\n";
        let out = insert_import(src, "quest_3", "ambush", false).expect("edit");
        assert_eq!(
            out,
            "IMPORT quest_1\nIMPORT quest_2\nIMPORT { ambush } FROM quest_3\n== hub ==\n"
        );
    }

    #[test]
    fn inserts_below_include_block_when_no_imports() {
        let src = "INCLUDE a.ink\nINCLUDE b.ink\n== hub ==\n";
        let out = insert_import(src, "quest_3", "ambush", false).expect("edit");
        assert_eq!(
            out,
            "INCLUDE a.ink\nINCLUDE b.ink\nIMPORT { ambush } FROM quest_3\n== hub ==\n"
        );
    }

    #[test]
    fn inserts_below_module_header_when_no_blocks() {
        let src = "#@module(town)\n// notes\n== hub ==\n";
        let out = insert_import(src, "quest_3", "ambush", false).expect("edit");
        assert_eq!(
            out,
            "#@module(town)\n// notes\nIMPORT { ambush } FROM quest_3\n== hub ==\n"
        );
    }

    #[test]
    fn inserts_at_top_when_bare_file() {
        let src = "== hub ==\ntext\n";
        let out = insert_import(src, "quest_3", "ambush", false).expect("edit");
        assert_eq!(out, "IMPORT { ambush } FROM quest_3\n== hub ==\ntext\n");
    }

    #[test]
    fn idempotent_when_name_already_imported() {
        let src = "IMPORT { ambush, gt } FROM quest_3\n== hub ==\n";
        assert_eq!(insert_import(src, "quest_3", "ambush", false), None);
    }

    #[test]
    fn distinct_name_from_same_module_adds_a_line() {
        // A different name from an already-imported module still needs an edit
        // (a second IMPORT line is legal; merging into the brace is a future
        // refinement).
        let src = "IMPORT { ambush } FROM quest_3\n== hub ==\n";
        let out = insert_import(src, "quest_3", "guard_talk", false).expect("edit");
        assert_eq!(
            out,
            "IMPORT { ambush } FROM quest_3\nIMPORT { guard_talk } FROM quest_3\n== hub ==\n"
        );
    }

    #[test]
    fn insertion_without_trailing_newline_stays_on_its_own_line() {
        let src = "== hub ==";
        let out = insert_import(src, "quest_3", "ambush", false).expect("edit");
        assert_eq!(out, "IMPORT { ambush } FROM quest_3\n== hub ==");
    }

    // ── insert_import: native dialect (issue #1590 companion finding) ──

    /// `native: true` renders `use module::name;`, not ink's `IMPORT { … }
    /// FROM …` — the exact gap the companion finding calls out ("do not
    /// leave it rendering ink syntax to native authors").
    #[test]
    fn native_insert_renders_use_syntax() {
        let src = "flow start() {\n  Hi\n}\n";
        let out = insert_import(src, "story::market::barter", "haggle", true).expect("edit");
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
            insert_import(src, "story::market::barter", "haggle", true),
            None
        );
    }

    /// The converse of the idempotence test: an ink-syntax existing import is
    /// invisible to the native frontend, so a native-flagged insert must not
    /// mistake it for coverage and skip the edit.
    #[test]
    fn native_insert_below_existing_use_block() {
        let src = "use story::market::barter::haggle;\nflow start() {\n  Hi\n}\n";
        let out = insert_import(src, "story::docks::barter", "haggle", true).expect("edit");
        assert_eq!(
            out,
            "use story::market::barter::haggle;\nuse story::docks::barter::haggle;\nflow start() {\n  Hi\n}\n"
        );
    }

    // ── import_actions (session-aware detection) ────────────────────

    /// Two declared modules; `town` references `quest`'s public `ambush`
    /// without importing it → `E025` → an `AddImport` quick-fix is offered.
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
        let actions = import_actions(session.db(), town, off);
        assert_eq!(actions.len(), 1, "one add-import offer");
        assert_eq!(actions[0].title, "Import `ambush` from `quest`");
        assert!(
            matches!(
                &actions[0].data,
                CodeActionData::AddImport { module, name, native }
                    if module == "quest" && name == "ambush" && !native
            ),
            "expected AddImport {{ quest, ambush, native: false }}, got {:?}",
            actions[0].data
        );
    }

    /// `import_actions` reads `db.is_native` per referring file (issue #1590
    /// companion finding) — a `.brink` referrer must get `native: true` on the
    /// offer so `resolve_code_action` renders `use`, not `IMPORT`.
    #[test]
    fn offers_add_import_with_native_flag_for_native_referrer() {
        let session = session_with(&[
            (
                "quest.ink",
                "#@module(quest)\n== ambush ==\n#@public\nGotcha!\n-> DONE\n",
            ),
            ("market/barter.brink", "flow start() {\n  -> ambush\n}\n"),
        ]);
        let town = session.file_id("market/barter.brink").expect("file id");
        let src = session.source(town).expect("src");
        let off = u32::try_from(src.find("ambush").expect("ref")).expect("fits");
        let actions = import_actions(session.db(), town, off);
        assert_eq!(actions.len(), 1, "one add-import offer");
        assert!(
            matches!(
                &actions[0].data,
                CodeActionData::AddImport { native, .. } if *native
            ),
            "expected native: true for a .brink referrer, got {:?}",
            actions[0].data
        );
    }

    /// The `AddImport` payload rides the wasm code-action `data` seam
    /// (`resolve_code_action_impl` round-trips it through JSON). Prove the
    /// tagged form survives serialize → deserialize and still resolves.
    #[test]
    fn add_import_data_round_trips_through_json() {
        let data = CodeActionData::AddImport {
            module: "quest".to_owned(),
            name: "ambush".to_owned(),
            native: false,
        };
        let json = serde_json::to_string(&data).expect("serialize");
        assert_eq!(
            json,
            r#"{"action":"AddImport","module":"quest","name":"ambush","native":false}"#
        );
        let back: CodeActionData = serde_json::from_str(&json).expect("deserialize");
        let out = crate::code_actions::resolve_code_action("== hub ==\n", &back).expect("resolve");
        assert_eq!(out, "IMPORT { ambush } FROM quest\n== hub ==\n");
    }

    #[test]
    fn no_offer_where_there_is_no_diagnostic() {
        let session = session_with(&[("town.ink", "#@module(town)\n== hub ==\nHi\n-> hub\n")]);
        let town = session.file_id("town.ink").expect("town id");
        let src = session.source(town).expect("src");
        let off = u32::try_from(src.find("hub").expect("ref")).expect("fits");
        assert!(import_actions(session.db(), town, off).is_empty());
    }
}
