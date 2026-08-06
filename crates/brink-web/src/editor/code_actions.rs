use wasm_bindgen::prelude::*;

use super::{EditorSession, ViewContext};
use crate::editor_dto::{CodeActionJs, code_action_kind_str};
use crate::editor_refactor::{error_json, gated_move_json, move_result_json_simple};

#[wasm_bindgen]
impl EditorSession {
    /// Compute code actions for a document handle. Returns JSON array.
    pub fn code_actions_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.code_actions_impl(&d.path, d.view.as_ref(), offset)
    }

    /// Compute code actions. Returns JSON array.
    pub fn code_actions(&self, offset: u32) -> String {
        self.code_actions_impl(&self.active_path, self.view.as_ref(), offset)
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
        self.resolve_code_action_impl(&self.active_path, self.view.as_ref(), data_json, offset)
    }

    /// Document-handle variant of [`resolve_code_action`](Self::resolve_code_action).
    pub fn resolve_code_action_doc(&self, doc: u32, data_json: &str, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return error_json("unknown document handle");
        };
        self.resolve_code_action_impl(&d.path, d.view.as_ref(), data_json, offset)
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
        let mut actions = brink_ide::code_actions::code_actions(source, abs_offset as usize);

        // Auto-import quick-fix (M-4, modules-spec §2/§9): a cursor on an
        // out-of-scope module reference (`E025`) offers an `AddImport` action.
        // Session-aware (needs the whole-project module view), so it is merged
        // here rather than in the source-only `code_actions` path; it resolves
        // through the same `resolve_code_action` seam as a pure source rewrite.
        actions.extend(brink_ide::import_fix::import_actions(
            self.session.db(),
            file_id,
            abs_offset,
        ));

        // T1c creation-site + call()/bind() strict quick-fixes (issue #744):
        // same session-aware merge posture as the auto-import offer above.
        actions.extend(brink_ide::creation_site_fix::fn_value_actions(
            self.session.db(),
            file_id,
            abs_offset,
        ));
        actions.extend(brink_ide::value_call_fix::value_call_actions(
            self.session.db(),
            file_id,
            abs_offset,
        ));

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
}
