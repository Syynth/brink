use wasm_bindgen::prelude::*;

use super::{EditorSession, ViewContext};

#[wasm_bindgen]
impl EditorSession {
    /// Convert a line element for a document handle. Returns JSON text edit or "null".
    ///
    /// Target values: `"narrative"`, `"choice"`, `"sticky_choice"`, `"gather"`, `"choice_body"`.
    pub fn convert_element_doc(&self, doc: u32, offset: u32, target: &str) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        crate::perf::time("ide.convertElement", || {
            self.convert_element_impl(&d.path, d.view.as_ref(), offset, target)
        })
    }

    /// Convert a line element to a different type. Returns JSON text edit or "null".
    ///
    /// Target values: `"narrative"`, `"choice"`, `"sticky_choice"`, `"gather"`, `"choice_body"`.
    pub fn convert_element(&self, offset: u32, target: &str) -> String {
        crate::perf::time("ide.convertElement", || {
            self.convert_element_impl(&self.active_path, self.view.as_ref(), offset, target)
        })
    }

    /// Format a document handle's file (sort knots). Returns the formatted
    /// source as a JSON string.
    pub fn format_document_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "\"\"".to_owned();
        };
        crate::perf::time("ide.formatDocument", || self.format_document_impl(&d.path))
    }

    /// Format the document (sort knots). Returns the formatted source as a JSON string.
    pub fn format_document(&self) -> String {
        crate::perf::time("ide.formatDocument", || {
            self.format_document_impl(&self.active_path)
        })
    }
}

impl EditorSession {
    fn convert_element_impl(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        offset: u32,
        target: &str,
    ) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "null".to_owned();
        };
        // #2291: `line_convert::convert_element` isn't just reading the
        // wrong CST here (`syntax_root` always runs ink's parser regardless
        // of extension — `IdeSession::syntax_root`'s doc comment, #2280's
        // failure mode) — the *feature itself* doesn't apply to native.
        // It detects/rewrites bare-line `*`/`+`/`-` choice and gather
        // sigils, but the native grammar has no such thing: choices only
        // exist inside an explicit `{? ... }` choice point
        // (`brink-syntax-native/src/parser/choice.rs`'s own doc: "there is
        // no bare knot-level `*`/`+` anymore — that ambiguity died with the
        // gather"). Converting a native line by inserting an ink sigil
        // would write syntactically invalid `.brink` source, not just
        // compute from the wrong tree — a native file gets no conversion
        // rather than a corrupting one.
        if self.session.is_native(file_id) {
            return "null".to_owned();
        }
        let (Some(hir), Some(source), Some(root)) = (
            self.session.hir(file_id),
            self.session.source(file_id),
            self.session.syntax_root(file_id),
        ) else {
            return "null".to_owned();
        };

        let convert_target = match target {
            "narrative" => brink_ide::line_convert::ConvertTarget::Narrative,
            "choice" => brink_ide::line_convert::ConvertTarget::Choice { sticky: false },
            "sticky_choice" => brink_ide::line_convert::ConvertTarget::Choice { sticky: true },
            "gather" => brink_ide::line_convert::ConvertTarget::Gather,
            "choice_body" => brink_ide::line_convert::ConvertTarget::ChoiceBody,
            _ => return "null".to_owned(),
        };

        let abs_offset = self.to_absolute(path, view, offset);
        match brink_ide::line_convert::convert_element(
            source,
            hir,
            &root,
            abs_offset,
            convert_target,
        ) {
            Some(edit) => match (
                self.to_relative(path, view, edit.from),
                self.to_relative(path, view, edit.to),
            ) {
                (Some(from), Some(to)) => {
                    let adjusted = brink_ide::line_convert::TextEdit {
                        from,
                        to,
                        insert: edit.insert,
                    };
                    serde_json::to_string(&adjusted).unwrap_or_default()
                }
                _ => "null".to_owned(),
            },
            None => "null".to_owned(),
        }
    }

    fn format_document_impl(&self, path: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "\"\"".to_owned();
        };
        let Some(source) = self.session.source(file_id) else {
            return "\"\"".to_owned();
        };

        // #2291: `sort_knots_in_source` unconditionally calls
        // `brink_syntax::parse` and looks for ink `Knot` nodes (`=== name
        // ===`) — a `.brink` file has no such header, so this is a no-op
        // today (native has no `=== ===` for ink's parser to ever match
        // >= 2 of), but it is still the always-ink-parse pattern
        // `IdeSession::syntax_root`'s doc comment warns against, and no
        // native knot-sort exists yet to route to instead. Gate explicitly
        // rather than rely on the coincidental no-op.
        if self.session.is_native(file_id) {
            return serde_json::to_string(source).unwrap_or_default();
        }

        let formatted = brink_ide::sort_knots_in_source(source);
        serde_json::to_string(&formatted).unwrap_or_default()
    }
}
