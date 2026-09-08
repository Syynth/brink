use wasm_bindgen::prelude::*;

use super::{EditorSession, ViewContext, byte_to_utf16, utf16_to_byte};
use crate::editor_dto::{CodeActionJs, FileEditJs, FixCaretJs, FixJs, code_action_kind_str};
use crate::editor_refactor::{error_json, gated_move_json, move_result_json_simple};

#[wasm_bindgen]
impl EditorSession {
    /// Compute code actions for a document handle. Returns JSON array.
    pub fn code_actions_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        crate::perf::time("ide.codeActions", || {
            self.code_actions_impl(&d.path, d.view.as_ref(), offset)
        })
    }

    /// Compute code actions. Returns JSON array.
    pub fn code_actions(&self, offset: u32) -> String {
        crate::perf::time("ide.codeActions", || {
            self.code_actions_impl(&self.active_path, self.view.as_ref(), offset)
        })
    }

    /// Apply a code action selected from [`code_actions`](Self::code_actions).
    ///
    /// `data_json` is the `data` field of a `CodeAction` (the self-describing,
    /// internally-tagged discriminator). `offset` is the cursor position the
    /// action was offered at — unused for the source-level actions (format /
    /// sort / structural move) but accepted for parity with the other queries
    /// and so future cursor-scoped actions need no signature change.
    ///
    /// Returns `StructuralResult`-shaped JSON: `new_source` for the primary file plus
    /// any `cross_file_edits` for structural moves, or `ok: false` with an
    /// `error` when the data is malformed or the action is a no-op.
    pub fn resolve_code_action(&self, data_json: &str, offset: u32) -> String {
        crate::perf::time("ide.resolveCodeAction", || {
            self.resolve_code_action_impl(&self.active_path, self.view.as_ref(), data_json, offset)
        })
    }

    /// Document-handle variant of [`resolve_code_action`](Self::resolve_code_action).
    pub fn resolve_code_action_doc(&self, doc: u32, data_json: &str, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return error_json("unknown document handle");
        };
        crate::perf::time("ide.resolveCodeAction", || {
            self.resolve_code_action_impl(&d.path, d.view.as_ref(), data_json, offset)
        })
    }

    /// The auto-fixes offered for the diagnostics under `offset`
    /// (`docs/autofix-spec.md` §7). Returns a JSON `FixJs[]`.
    ///
    /// Distinct from [`code_actions`](Self::code_actions), which offers
    /// structural refactors keyed off the *syntax* at the cursor: a fix is
    /// keyed off a *diagnostic* and carries its own minimal edits, which may
    /// land in other files (§4).
    pub fn fixes_at(&self, offset: u32) -> String {
        crate::perf::time("ide.fixesAt", || {
            self.fixes_at_impl(&self.active_path, self.view.as_ref(), offset)
        })
    }

    /// Document-handle variant of [`fixes_at`](Self::fixes_at).
    pub fn fixes_at_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        crate::perf::time("ide.fixesAt", || {
            self.fixes_at_impl(&d.path, d.view.as_ref(), offset)
        })
    }

    /// Turn a chosen fix (a `FixJs` from [`fixes_at`](Self::fixes_at), passed
    /// back verbatim) into the sources to write, as `StructuralResult`-shaped
    /// JSON: `new_source` for `path` plus a `cross_file_edits` entry per other
    /// file the fix touches.
    ///
    /// Side-effect-free — the caller applies through its own apply seam.
    pub fn apply_fix(&self, fix_json: &str) -> String {
        crate::perf::time("ide.applyFix", || {
            self.apply_fix_impl(&self.active_path, fix_json)
        })
    }

    /// [`apply_fix`](Self::apply_fix) for a named file rather than the active
    /// one — the Problems panel's per-row road, where the row's diagnostic
    /// names its own file and that file is the one the result should call
    /// `path` (`docs/autofix-spec.md` §7). Applying a row's fix through
    /// `apply_fix` instead would report the fix's edits against whichever
    /// file happens to be open.
    pub fn apply_fix_at_path(&self, path: &str, fix_json: &str) -> String {
        crate::perf::time("ide.applyFix", || self.apply_fix_impl(path, fix_json))
    }

    /// Document-handle variant of [`apply_fix`](Self::apply_fix).
    pub fn apply_fix_doc(&self, doc: u32, fix_json: &str) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return error_json("unknown document handle");
        };
        crate::perf::time("ide.applyFix", || self.apply_fix_impl(&d.path, fix_json))
    }
}

impl EditorSession {
    fn code_actions_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let Some(source) = self.session.source(file_id) else {
            return "[]".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        // Diagnostic-keyed quick-fixes are NOT merged here (#3377): they are
        // `Fix`es with their own `Vec<FileEdit>` currency, pulled through
        // `fixes_at` / applied through `apply_fix`. This road stays the
        // structural-refactor road (`docs/autofix-spec.md` §2).
        let actions = brink_ide::code_actions::code_actions(source, abs_offset as usize);

        let items: Vec<CodeActionJs> = actions
            .iter()
            .map(|a| CodeActionJs {
                title: a.title.clone(),
                kind: code_action_kind_str(&a.kind).to_owned(),
                data: serde_json::to_value(&a.data).unwrap_or(serde_json::Value::Null),
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    fn resolve_code_action_impl(
        &self,
        path: &str,
        _view: Option<&ViewContext>,
        data_json: &str,
        _offset: u32,
    ) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(source) = self.session.source(file_id) else {
            return error_json("no source");
        };

        let data: brink_ide::code_actions::CodeActionData = match serde_json::from_str(data_json) {
            Ok(d) => d,
            Err(e) => return error_json(&format!("invalid code-action data: {e}")),
        };

        // Structural moves (move / promote / demote) need analysis context;
        // everything else (format / sort / reorder) is a pure source rewrite.
        if let Some(analysis) = self.session.analysis()
            && let Some(result) =
                brink_ide::code_actions::resolve_structural_action(source, analysis, file_id, &data)
        {
            return gated_move_json(&self.session, result, path);
        }

        match brink_ide::code_actions::resolve_code_action(source, &data) {
            Some(new_source) => move_result_json_simple(new_source, path),
            None => error_json("code action produced no change"),
        }
    }

    /// [`fixes_at_impl`](Self::fixes_at_impl) for a named file rather than
    /// the active one, with no view context: the Problems panel's per-row
    /// road (`docs/autofix-spec.md` §7), where the row names its own file
    /// and the offset is already whole-file absolute.
    pub(super) fn fixes_at_path_impl(&self, path: &str, offset: u32) -> String {
        crate::perf::time("ide.fixesAt", || self.fixes_at_impl(path, None, offset))
    }

    fn fixes_at_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let abs_offset = self.to_absolute(path, view, offset);
        let cx = brink_ide::fix::FixCx::new(self.session.db());
        // Resolve every edit fallibly, and drop the WHOLE fix if any edit
        // names a file with no loaded source/path — cross-file edits are the
        // reason `Fix::edits` exists (docs/autofix-spec.md §4), so a partial
        // resolution here would offer a fix that `apply_fix` can only apply
        // incompletely (review finding on #3384).
        // The two suppression channels §5 names both apply here now
        // (review finding on #3459 — this used to align on only one):
        // `brink_ide::fix::fixes_at` itself applies inline
        // `// brink-disable`/`@[allow(…)]` suppressions before matching a
        // diagnostic to the cursor, the same way `fix_offers_impl` does; the
        // filter below withdraws the other channel, a `[lints]` `"allow"`
        // code, which has no Problems row and no squiggle and so must not
        // offer a fix here either.
        let suppressed = self.suppressed_codes();
        let items: Vec<FixJs> = brink_ide::fix::fixes_at(&cx, file_id, abs_offset)
            .iter()
            .filter(|fix| !suppressed.contains(&fix.code))
            .filter_map(|fix| self.fix_to_js(fix))
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    /// Resolve one [`brink_ide::fix::Fix`] to its `FixJs` wire shape, or
    /// `None` if any of its edits names a file with no loaded source/path.
    /// Split out of [`fixes_at_impl`](Self::fixes_at_impl) so the
    /// drop-the-whole-fix behavior is unit-testable without a fixer that
    /// actually produces a cross-file edit (none does today).
    pub(super) fn fix_to_js(&self, fix: &brink_ide::fix::Fix) -> Option<FixJs> {
        let edits: Vec<FileEditJs> = fix
            .edits
            .iter()
            .map(|e| {
                let src = self.session.source(e.file)?;
                Some(FileEditJs {
                    path: self.session.file_path(e.file)?.to_owned(),
                    start: byte_to_utf16(src, e.range.start().into()),
                    end: byte_to_utf16(src, e.range.end().into()),
                    new_text: e.new_text.clone(),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(FixJs {
            code: fix.code.as_str().to_owned(),
            title: fix.title.clone(),
            applicability: fix.applicability.as_str().to_owned(),
            edits,
            caret: fix.caret.and_then(|(file, at)| {
                let src = self.session.source(file)?;
                Some(FixCaretJs {
                    path: self.session.file_path(file)?.to_owned(),
                    offset: byte_to_utf16(src, at.into()),
                })
            }),
        })
    }

    fn apply_fix_impl(&self, path: &str, fix_json: &str) -> String {
        let fix: FixJs = match serde_json::from_str(fix_json) {
            Ok(f) => f,
            Err(e) => return error_json(&format!("invalid fix: {e}")),
        };
        if fix.edits.is_empty() {
            return error_json("fix carries no edits");
        }

        // Back to brink-ide's currency: file-absolute *byte* ranges.
        let mut edits: Vec<brink_ide::rename::FileEdit> = Vec::with_capacity(fix.edits.len());
        for e in &fix.edits {
            let Some(file) = self.session.file_id(&e.path) else {
                return error_json("fix names a file that is not loaded");
            };
            let Some(src) = self.session.source(file) else {
                return error_json("fix names a file that is not loaded");
            };
            let start = utf16_to_byte(src, e.start);
            let end = utf16_to_byte(src, e.end);
            // `TextRange::new` asserts `start <= end` and panics otherwise —
            // `apply_fix`/`apply_fix_doc` are `#[wasm_bindgen]` entry points
            // taking arbitrary caller JSON, so an inverted edit must be
            // refused here rather than reaching that assert (review finding
            // on #3384).
            if start > end {
                return error_json("fix has an inverted edit range");
            }
            edits.push(brink_ide::rename::FileEdit {
                file,
                range: rowan::TextRange::new(start.into(), end.into()),
                new_text: e.new_text.clone(),
            });
        }

        let Some(primary) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(source) = self.session.source(primary) else {
            return error_json("no source");
        };
        let primary_edits: Vec<(usize, usize, String)> = edits
            .iter()
            .filter(|e| e.file == primary)
            .map(|e| {
                (
                    usize::from(e.range.start()),
                    usize::from(e.range.end()),
                    e.new_text.clone(),
                )
            })
            .collect();

        // A fix is offered only where the analyzer already reports a problem
        // and is pinned by §3's discharge obligation, so it applies directly:
        // `safe` with an empty breakage report, exactly as the three
        // diagnostic-keyed quick-fixes behaved before #3377.
        let result = brink_ide::structural_result::StructuralResult {
            new_source: Some(crate::editor_refactor::apply_edits(source, primary_edits)),
            cross_file_edits: edits,
            safe: true,
            introduced: Vec::new(),
        };
        crate::editor_refactor::structural_result_json(&self.session, &result, path)
    }
}

#[cfg(test)]
mod tests {
    use super::EditorSession;

    /// The definition side: a `pub` flow in its own native module.
    const BARTER: &str = "\
pub flow haggle() {
  You haggle over the price.
}
";

    /// The reference side, with **no** `use` — the `E025` import-required
    /// shape the `ImportFixer` discharges.
    const MAIN: &str = "\
flow start() {
  The market is busy.
  -> haggle
}
";

    fn session() -> EditorSession {
        let mut session = EditorSession::new();
        session.update_file("market/barter.brink", BARTER);
        session.update_file("main.brink", MAIN);
        assert!(session.set_active_file("main.brink"));
        session
    }

    fn haggle_reference_offset() -> u32 {
        let at = MAIN.find("haggle\n}");
        assert!(at.is_some(), "fixture must carry the divert target");
        u32::try_from(at.expect("just asserted above")).expect("offset")
    }

    /// The `@brink-lang/web` fix road end to end: a cursor on the `E025`
    /// squiggle offers the `FixJs` the studio menu renders, carrying its own
    /// minimal edit rather than a `resolveCodeAction` payload.
    #[test]
    fn fixes_at_offers_the_import_fix_over_the_wasm_boundary() {
        let session = session();
        let json = session.fixes_at(haggle_reference_offset());
        let fixes: serde_json::Value = serde_json::from_str(&json).expect("fixes JSON");
        let fixes = fixes.as_array().expect("array");
        assert_eq!(fixes.len(), 1, "{json}");
        assert_eq!(fixes[0]["code"], "E025");
        assert_eq!(fixes[0]["applicability"], "suggested");
        assert_eq!(
            fixes[0]["title"],
            "Import `haggle` from `story::market::barter`"
        );
        let edits = fixes[0]["edits"].as_array().expect("edits");
        assert_eq!(edits.len(), 1, "{json}");
        assert_eq!(edits[0]["path"], "main.brink");
        assert_eq!(edits[0]["new_text"], "use story::market::barter::haggle;\n");
    }

    /// Handing the chosen fix straight back produces the sources to write —
    /// the `StructuralResult` shape the studio's existing apply seam takes.
    #[test]
    fn apply_fix_returns_the_new_source_for_the_chosen_fix() {
        let session = session();
        let json = session.fixes_at(haggle_reference_offset());
        let fixes: serde_json::Value = serde_json::from_str(&json).expect("fixes JSON");
        let chosen = serde_json::to_string(&fixes[0]).expect("re-serialize");

        let applied = session.apply_fix(&chosen);
        let result: serde_json::Value = serde_json::from_str(&applied).expect("result JSON");
        assert_eq!(result["ok"], true, "{applied}");
        assert_eq!(result["safe"], true, "{applied}");
        assert_eq!(result["path"], "main.brink");
        assert_eq!(
            result["new_source"],
            format!("use story::market::barter::haggle;\n{MAIN}")
        );
        assert_eq!(
            result["cross_file_edits"].as_array().expect("array").len(),
            0,
            "a single-file fix touches no other file: {applied}"
        );
    }

    /// The structural-refactor road no longer carries diagnostic-keyed
    /// quick-fixes (#3377): `code_actions` offers only the syntax-keyed
    /// entries, and the import fix is reachable through `fixes_at` alone.
    #[test]
    fn code_actions_no_longer_merge_the_import_quick_fix() {
        let session = session();
        let json = session.code_actions(haggle_reference_offset());
        assert!(
            !json.contains("Import `haggle`"),
            "code_actions must not carry the E025 fix: {json}"
        );
    }

    /// Review finding on #3384: `apply_fix`/`apply_fix_doc` are
    /// `#[wasm_bindgen]` entry points taking arbitrary caller JSON. A `FixJs`
    /// with `end < start` used to reach `rowan::TextRange::new`, which
    /// panics on an inverted range — this must refuse instead of aborting the
    /// wasm session, exactly like every other malformed-input branch here.
    #[test]
    fn apply_fix_refuses_an_inverted_edit_range() {
        let session = session();
        let malformed = serde_json::json!({
            "code": "E025",
            "title": "malformed",
            "applicability": "suggested",
            "edits": [{ "path": "main.brink", "start": 10, "end": 0, "new_text": "" }],
        });
        let result = session.apply_fix(&malformed.to_string());
        let parsed: serde_json::Value = serde_json::from_str(&result).expect("result JSON");
        assert_eq!(parsed["ok"], false, "{result}");
        assert_eq!(
            parsed["error"], "fix has an inverted edit range",
            "{result}"
        );
    }

    /// Review finding on #3384: an edit naming a file with no loaded
    /// source/path used to be silently dropped from `fixes_at`'s DTO while
    /// the rest of the fix was still offered, so `apply_fix` would apply a
    /// partial rewrite. `fix_to_js` — the exact mapping `fixes_at_impl` calls
    /// — must drop the whole fix instead. No fixer produces a genuinely
    /// unresolvable cross-file edit today (§4's currency is unreachable that
    /// way until a multi-file fixer lands), so this constructs the `Fix` by
    /// hand and calls the production mapping function directly.
    #[test]
    fn fix_to_js_drops_the_whole_fix_when_an_edit_names_an_unloaded_file() {
        let session = session();
        let fix = brink_ide::fix::Fix {
            code: brink_ir::DiagnosticCode::E025,
            title: "hand-built cross-file fix".to_owned(),
            applicability: brink_ide::fix::Applicability::Suggested,
            edits: vec![brink_ide::rename::FileEdit {
                // A `FileId` this session never loaded a source for.
                file: brink_ir::FileId(999_999),
                range: rowan::TextRange::new(0.into(), 0.into()),
                new_text: String::new(),
            }],
            caret: None,
        };
        assert!(
            session.fix_to_js(&fix).is_none(),
            "a fix with an unresolvable edit must be dropped entirely, not offered partially"
        );
    }
}
