//! `ClassifierSession` — the capability-stripped main-thread session
//! (`docs/editor-worker-spec.md` §4, W3 of the worker architecture).
//!
//! When the full project session moves to a Web Worker (W4), this type is
//! what stays on the main thread: exactly enough to style newly typed
//! text in the same frame with the real lexer — the open document's
//! segment substrate (per-segment lex/parse/lower, all
//! project-independent by construction) plus the config surface that
//! affects classification (dialect, language dialect). Nothing else.
//!
//! **The exported surface IS the capability boundary** (spec §4.2):
//! internally this wraps the full [`EditorSession`] (one engine, one set
//! of query roads — slice/assembly parity with the project session comes
//! by construction, pinned by the tests below), but no project method —
//! analysis, diagnostics, compile, refactors, symbol queries — is
//! exported, and the write paths deliberately never trigger an analysis
//! pull (`write_source_no_analysis`; the delta ingress is update-only by
//! design, #3100). If a keystroke-path consumer ever needs more than
//! this surface, that is a design smell to surface, not a method to add
//! here.
//!
//! One classifier serves ONE document (`open` replaces it): the TS side
//! creates one per mounted view slot, mirroring that view's edits.

use wasm_bindgen::prelude::*;

use crate::editor::EditorSession;

#[wasm_bindgen]
pub struct ClassifierSession {
    inner: EditorSession,
    /// The one open document's handle id — `0` until `open` succeeds.
    doc: u32,
    path: String,
}

impl Default for ClassifierSession {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl ClassifierSession {
    #[wasm_bindgen(constructor)]
    pub fn new() -> ClassifierSession {
        ClassifierSession {
            inner: EditorSession::new(),
            doc: 0,
            path: String::new(),
        }
    }

    /// Open (or replace) THE document this classifier serves. The path's
    /// extension picks the surface (`.ink` vs `.brink`), same as the full
    /// session. Returns false if the document could not be opened.
    pub fn open(&mut self, path: &str, source: &str) -> bool {
        self.inner.write_source_no_analysis(path, source);
        let doc = self.inner.open_document(path);
        if doc == 0 {
            return false;
        }
        if self.doc != 0 {
            self.inner.close_document(self.doc);
        }
        self.doc = doc;
        path.clone_into(&mut self.path);
        true
    }

    /// Full-text push (the delta path's fallback). Never analyzes.
    pub fn update_source(&mut self, source: &str) -> bool {
        if self.doc == 0 {
            return false;
        }
        let path = self.path.clone();
        self.inner.write_source_no_analysis(&path, source);
        true
    }

    /// Bounded edit list (same wire shape as `apply_edits_document`,
    /// #3064 C1) against the open document. Never analyzes.
    pub fn apply_edits(&mut self, edits_json: &str) -> bool {
        if self.doc == 0 {
            return false;
        }
        self.inner.apply_edits_document(self.doc, edits_json)
    }

    /// The segment manifest for the open document — same wire shape as
    /// `segment_manifest_doc`, so the TS slice cache is shared code.
    pub fn segment_manifest(&self) -> String {
        self.inner.segment_manifest_doc(self.doc)
    }

    /// One segment's owned line contexts (segment-relative lines).
    pub fn segment_line_contexts(&self, key: &str) -> String {
        self.inner.segment_line_contexts_doc(self.doc, key)
    }

    /// One segment's classifier tokens (project-independent; lines
    /// relative to the segment's owned range).
    pub fn segment_semantic_tokens_fast(&self, key: &str) -> String {
        self.inner.segment_semantic_tokens_fast_doc(self.doc, key)
    }

    /// The dialogue dialect (at-cue presets change line classes) — pushed
    /// by the same TS code path that pushes it to the project session.
    pub fn set_dialect(&mut self, json: &str) -> Result<(), JsError> {
        self.inner.set_dialect(json)
    }

    pub fn clear_dialect(&mut self) {
        self.inner.clear_dialect();
    }

    /// The compiler surface dialect (`"brink"` | `"strict-ink"`).
    pub fn set_language_dialect(&mut self, value: &str) {
        self.inner.set_language_dialect(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INK_SOURCE: &str =
        "=== start ===\nHello there.\n* [A choice] -> DONE\n=== second ===\nMore text.\n-> DONE\n";

    fn full_session_with(path: &str, source: &str) -> (EditorSession, u32) {
        let mut session = EditorSession::new();
        session.update_file(path, source);
        let doc = session.open_document(path);
        assert!(doc != 0, "full session must open the doc");
        (session, doc)
    }

    /// Every classifier output must equal the full project session's for
    /// the same source — slice/assembly parity by construction (one
    /// engine), pinned here so the surfaces cannot drift apart silently.
    ///
    /// Segment KEYS are salsa identities and therefore
    /// database-instance-specific (two sessions are two databases): the
    /// TS cache treats them as opaque, so parity means "same segmentation
    /// (`ownedFrom`, `totalLines`, count) and same per-segment content
    /// when paired positionally", never byte-equal keys.
    #[test]
    fn classifier_outputs_match_the_full_session() {
        assert_parity(INK_SOURCE, None);
    }

    /// Parity must hold across the delta-ingress path too: the same edit
    /// applied to both sessions yields the same segmentation and slices.
    #[test]
    fn parity_survives_a_delta_edit() {
        assert_parity(
            INK_SOURCE,
            Some(r#"[{"from":14,"to":14,"insert":"Well, h"}]"#),
        );
    }

    fn assert_parity(source: &str, edits: Option<&str>) {
        let path = "main.ink";
        let (mut full, full_doc) = full_session_with(path, source);
        let mut classifier = ClassifierSession::new();
        assert!(classifier.open(path, source));

        if let Some(edits) = edits {
            assert!(full.apply_edits_document(full_doc, edits));
            assert!(classifier.apply_edits(edits));
        }

        let ours = classifier.segment_manifest();
        let theirs = full.segment_manifest_doc(full_doc);
        assert!(ours != "null", "manifest must exist for an ink file");
        assert_eq!(
            strip_keys(&ours),
            strip_keys(&theirs),
            "segmentation (ownedFrom/totalLines/count) diverged"
        );

        let our_keys = manifest_keys(&ours);
        let their_keys = manifest_keys(&theirs);
        assert!(!our_keys.is_empty(), "expected at least one segment");
        assert_eq!(our_keys.len(), their_keys.len());
        for (mine, theirs_key) in our_keys.iter().zip(their_keys.iter()) {
            assert_eq!(
                classifier.segment_line_contexts(mine),
                full.segment_line_contexts_doc(full_doc, theirs_key),
                "line contexts diverged for segment pair {mine}/{theirs_key}"
            );
            assert_eq!(
                classifier.segment_semantic_tokens_fast(mine),
                full.segment_semantic_tokens_fast_doc(full_doc, theirs_key),
                "classifier tokens diverged for segment pair {mine}/{theirs_key}"
            );
        }
    }

    /// The manifest with every instance-specific `"key":"i:g"` value
    /// blanked — what remains is the segmentation itself.
    fn strip_keys(manifest: &str) -> String {
        let needle = "\"key\":\"";
        let mut out = String::new();
        let mut rest = manifest;
        while let Some(idx) = rest.find(needle) {
            let after = &rest[idx + needle.len()..];
            let Some(end) = after.find('"') else { break };
            out.push_str(&rest[..idx + needle.len()]);
            out.push('_');
            rest = &after[end..];
        }
        out.push_str(rest);
        out
    }

    /// The full-push fallback keeps serving fresh content (and `open`
    /// re-targets a replacement document cleanly).
    #[test]
    fn full_push_and_reopen_serve_fresh_content() {
        let path = "main.ink";
        let mut classifier = ClassifierSession::new();
        assert!(classifier.open(path, INK_SOURCE));
        let before = classifier.segment_manifest();

        let rewritten = "=== start ===\nEntirely new text.\n-> DONE\n";
        assert!(classifier.update_source(rewritten));
        let after = classifier.segment_manifest();
        assert!(before != after, "content change must move segment identity");

        let (full, full_doc) = full_session_with(path, rewritten);
        assert_eq!(
            strip_keys(&after),
            strip_keys(&full.segment_manifest_doc(full_doc))
        );

        assert!(classifier.open("other.ink", "=== other ===\n-> DONE\n"));
        assert!(classifier.segment_manifest() != "null");
    }

    /// Guardrails: everything refuses cleanly before `open`.
    #[test]
    fn refuses_cleanly_before_open() {
        let mut classifier = ClassifierSession::new();
        assert!(!classifier.update_source("text"));
        assert!(!classifier.apply_edits("[]"));
        assert_eq!(classifier.segment_manifest(), "null");
    }

    fn manifest_keys(manifest: &str) -> Vec<String> {
        // The manifest wire shape: {"segments":[{"key":"i:g",...},...],...}.
        // A tiny extraction keeps this test free of serde structs that
        // would shadow the real wire contract.
        let mut keys = Vec::new();
        let mut rest = manifest;
        while let Some(idx) = rest.find("\"key\":\"") {
            let tail = &rest[idx + 7..];
            let Some(end) = tail.find('"') else { break };
            keys.push(tail[..end].to_owned());
            rest = &tail[end..];
        }
        keys
    }
}
