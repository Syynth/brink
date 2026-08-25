use wasm_bindgen::prelude::*;

use super::{EditorSession, ViewContext};
use crate::editor_dto::{HirLineContainerJs, HirProjectionJs, HirSpanJs, TokenJs, span_kind_str};

#[wasm_bindgen]
impl EditorSession {
    /// Compute per-line context for a document handle. Returns JSON array of `LineContext`.
    pub fn line_contexts_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.line_contexts_impl(&d.path, d.view.as_ref())
    }

    /// Compute per-line context from the HIR. Returns JSON array of `LineContext`.
    pub fn line_contexts(&self) -> String {
        self.line_contexts_impl(&self.active_path, self.view.as_ref())
    }

    /// Compute semantic tokens for a document handle. Returns JSON array of tokens.
    pub fn semantic_tokens_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.semantic_tokens_impl(&d.path, d.view.as_ref())
    }

    /// Compute semantic tokens. Returns JSON array of tokens.
    pub fn semantic_tokens(&self) -> String {
        self.semantic_tokens_impl(&self.active_path, self.view.as_ref())
    }

    /// The HIR structural projection for a document handle (#454): a JSON
    /// object `{ "spans": [...], "lines": [[...], ...] }` — nested semantic
    /// spans plus the per-line container stack for rails.
    pub fn hir_spans_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "{\"spans\":[],\"lines\":[]}".to_owned();
        };
        self.hir_spans_impl(&d.path, d.view.as_ref())
    }

    /// The outbound-delta segment manifest for a FILE handle's document
    /// (#3064 option A): `{"totalLines": N, "segments": [{"key", "ownedFrom"}]}`
    /// as JSON, or `"null"` for fragment views, unknown handles, and
    /// non-ink files — the consumer falls back to the whole-document
    /// queries. Keys are salsa identity `index:generation` — stable
    /// across shift edits, changed exactly when a segment's content
    /// changes, ABA-safe.
    pub fn segment_manifest_doc(&self, doc: u32) -> String {
        #[derive(serde::Serialize)]
        struct EntryJs<'a> {
            key: &'a str,
            #[serde(rename = "ownedFrom")]
            owned_from: u32,
        }
        #[derive(serde::Serialize)]
        struct ManifestJs<'a> {
            #[serde(rename = "totalLines")]
            total_lines: u32,
            segments: Vec<EntryJs<'a>>,
        }
        let Some(state) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        if state.view.is_some() {
            return "null".to_owned();
        }
        let Some(file_id) = self.session.file_id(&state.path) else {
            return "null".to_owned();
        };
        let Some((entries, total_lines)) = self.session.segment_manifest(file_id) else {
            return "null".to_owned();
        };
        serde_json::to_string(&ManifestJs {
            total_lines,
            segments: entries
                .iter()
                .map(|(key, owned_from)| EntryJs {
                    key,
                    owned_from: *owned_from,
                })
                .collect(),
        })
        .unwrap_or_else(|_| "null".to_owned())
    }

    /// One manifest segment's owned line-context slice (#3064 option A);
    /// `"null"` for a stale key — re-fetch the manifest.
    pub fn segment_line_contexts_doc(&self, doc: u32, key: &str) -> String {
        let Some(state) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        let Some(file_id) = self.session.file_id(&state.path) else {
            return "null".to_owned();
        };
        match self.session.segment_line_contexts_slice(file_id, key) {
            Some(slice) => serde_json::to_string(&slice).unwrap_or_else(|_| "null".to_owned()),
            None => "null".to_owned(),
        }
    }

    /// The classifier-only sibling of
    /// [`segment_semantic_tokens_doc`](Self::segment_semantic_tokens_doc)
    /// (#3064 micro): identical shape, no index/resolve pull — the
    /// keystroke path's source while the deferred refresh fetches the
    /// refined slice.
    pub fn segment_semantic_tokens_fast_doc(&self, doc: u32, key: &str) -> String {
        let Some(state) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        let Some(file_id) = self.session.file_id(&state.path) else {
            return "null".to_owned();
        };
        let Some(slice) = self
            .session
            .segment_semantic_tokens_slice_fast(file_id, key)
        else {
            return "null".to_owned();
        };
        let tokens: Vec<TokenJs> = slice
            .iter()
            .map(|t| TokenJs {
                line: t.line,
                start_char: t.start_char,
                length: t.length,
                token_type: t.token_type,
                modifiers: t.modifiers,
            })
            .collect();
        serde_json::to_string(&tokens).unwrap_or_else(|_| "null".to_owned())
    }

    /// One manifest segment's owned semantic-token slice, token lines
    /// relative to the segment's owned start (#3064 option A); `"null"`
    /// for a stale key.
    pub fn segment_semantic_tokens_doc(&self, doc: u32, key: &str) -> String {
        let Some(state) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        let Some(file_id) = self.session.file_id(&state.path) else {
            return "null".to_owned();
        };
        let Some(slice) = self.session.segment_semantic_tokens_slice(file_id, key) else {
            return "null".to_owned();
        };
        let tokens: Vec<TokenJs> = slice
            .iter()
            .map(|t| TokenJs {
                line: t.line,
                start_char: t.start_char,
                length: t.length,
                token_type: t.token_type,
                modifiers: t.modifiers,
            })
            .collect();
        serde_json::to_string(&tokens).unwrap_or_else(|_| "null".to_owned())
    }
}

impl EditorSession {
    fn line_contexts_impl(&self, path: &str, view: Option<&ViewContext>) -> String {
        // #3064 B3: the assembled per-segment db query — an edit
        // reclassifies the edited knot's fragment only; native files run
        // whole-file inside the same query. Dialect and frontend dispatch
        // both live db-side now.
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let Some(contexts) = self.session.line_contexts(file_id) else {
            return "[]".to_owned();
        };
        if let Some(v) = view {
            let start = v.start_line as usize;
            let end_line = self
                .view_end_line(path, v)
                .map_or(contexts.len(), |l| l as usize);
            let slice = &contexts[start..end_line.min(contexts.len())];
            serde_json::to_string(slice).unwrap_or_default()
        } else {
            serde_json::to_string(&*contexts).unwrap_or_default()
        }
    }

    fn semantic_tokens_impl(&self, path: &str, view: Option<&ViewContext>) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        // #3064 B4: the assembled per-segment db query — the native/ink
        // frontend dispatch (#2280) and the identity join both live
        // db-side now, and an edit re-tokenizes the edited knot's
        // fragment only.
        let Some(raw) = self.session.semantic_tokens(file_id) else {
            return "[]".to_owned();
        };

        let tokens: Vec<TokenJs> = raw
            .iter()
            .filter_map(|t| {
                let line = Self::to_relative_line(view, t.line)?;
                Some(TokenJs {
                    line,
                    start_char: t.start_char,
                    length: t.length,
                    token_type: t.token_type,
                    modifiers: t.modifiers,
                })
            })
            .collect();

        serde_json::to_string(&tokens).unwrap_or_default()
    }

    /// The HIR structural projection for one file (#454 phase 2): spans with
    /// UTF-16 line/char coordinates plus the per-line container stack, as one
    /// JSON object `{ "spans": [...], "lines": [[...], ...] }`.
    ///
    /// Byte→line/UTF-16 conversion happens here (the producer returns byte
    /// ranges). Under a view, span lines are remapped relative to the view's
    /// start (spans entirely above it are dropped) and the `lines` array is
    /// sliced to the view window — the same conventions as `semantic_tokens_impl`
    /// and `line_contexts_impl`.
    fn hir_spans_impl(&self, path: &str, view: Option<&ViewContext>) -> String {
        const EMPTY: &str = "{\"spans\":[],\"lines\":[]}";
        let Some(file_id) = self.session.file_id(path) else {
            return EMPTY.to_owned();
        };
        // #3064 C1: no eager `session.analysis()` gate — it forced the
        // whole diagnostics bundle per keystroke and then DISCARDED it;
        // the assembled projection reads the cheap resolutions half
        // itself, and `analysis()` has been always-`Some` since option A.
        let Some(source) = self.session.source(file_id) else {
            return EMPTY.to_owned();
        };
        let Some(projection) = self.session.projection(file_id) else {
            return EMPTY.to_owned();
        };
        let idx = brink_ide::LineIndex::new(source);

        let spans: Vec<HirSpanJs> = projection
            .spans
            .iter()
            .filter_map(|s| {
                let (abs_start_line, start_char) = idx.line_col(s.range.start());
                let (abs_end_line, end_char) = idx.line_col(s.range.end());
                // Column-0 end rule (see `build_line_stacks`): a span ending
                // exactly at a line's start ends on the PREVIOUS line — step
                // the end position back one byte so line and char stay
                // consistent (the byte before column 0 is the prior line's
                // terminator).
                let (abs_end_line, end_char) = if end_char == 0 && abs_end_line > abs_start_line {
                    idx.line_col(s.range.end() - rowan::TextSize::from(1))
                } else {
                    (abs_end_line, end_char)
                };
                // Containers additionally carry the TIGHT end (two-range
                // model, issue #3054 review) — the rails/tooltip range.
                let abs_content_end = s.kind.is_container().then(|| {
                    brink_ide::hir_projection::tight_container_end_line(&idx, source, s.range)
                        .max(abs_start_line)
                });
                // Drop spans that end above the view; clamp ones straddling its
                // start so partially-visible containers keep their rails.
                // Non-containers straddling the start are dropped instead —
                // clamping a multi-line inline span (a `{ cond: … }` construct
                // extent, a multi-line content node) to (0, 0) would paint a
                // mark from the view's top-left over unrelated text.
                let end_line = Self::to_relative_line(view, abs_end_line)?;
                let (start_line, start_char) = match Self::to_relative_line(view, abs_start_line) {
                    Some(l) => (l, start_char),
                    None if s.kind.is_container() => (0, 0),
                    None => return None,
                };
                let content_end_line =
                    abs_content_end.and_then(|l| Self::to_relative_line(view, l));
                Some(HirSpanJs {
                    start_line,
                    start_char,
                    end_line,
                    end_char,
                    content_end_line,
                    kind: span_kind_str(s.kind),
                    container: s.kind.is_container(),
                    depth: s.depth,
                    def_id: s.def_id,
                    target_id: s.target_id,
                    handle: s.handle,
                })
            })
            .collect();

        let lines: Vec<Vec<HirLineContainerJs>> = {
            let all = &projection.lines;
            let (start, end) = view.map_or((0, all.len()), |v| {
                let start = v.start_line as usize;
                let end = self
                    .view_end_line(path, v)
                    .map_or(all.len(), |l| l as usize);
                (start.min(all.len()), end.min(all.len()))
            });
            all[start.min(end)..end]
                .iter()
                .map(|stack| {
                    stack
                        .containers
                        .iter()
                        .map(|c| HirLineContainerJs {
                            kind: span_kind_str(c.kind),
                            handle: c.handle,
                            depth: c.depth,
                        })
                        .collect()
                })
                .collect()
        };

        serde_json::to_string(&HirProjectionJs { spans, lines })
            .unwrap_or_else(|_| EMPTY.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::EditorSession;

    /// End-to-end through the actual wasm-bridge entry point (issue #2280):
    /// a native `.brink` file's `struct`/field-name/type-reference tokens
    /// must not all read as `variable` (token type `2`) — the bug this
    /// dispatch exists to fix. `update_file` alone does **not** make a file
    /// active for IDE queries (`semantic_tokens` reads `active_path`) —
    /// `set_active_file` does; skipping it would silently query an empty
    /// path and pass vacuously.
    #[test]
    fn semantic_tokens_on_a_native_file_does_not_collapse_everything_to_variable() {
        const TT_VARIABLE: u64 = 2;
        const TT_STRUCT: u64 = 13;
        const TT_PROPERTY: u64 = 14;

        let mut s = EditorSession::new();
        s.update_file(
            "t.brink",
            "struct Cue {\n    speaker: string,\n}\nflow main() {\n    -> DONE\n}\n",
        );
        assert!(
            s.set_active_file("t.brink"),
            "t.brink must be loaded and selectable"
        );

        let json = s.semantic_tokens();
        let tokens: serde_json::Value =
            serde_json::from_str(&json).expect("semantic_tokens must return valid JSON");
        let tokens = tokens.as_array().expect("a JSON array of tokens");
        assert!(
            !tokens.is_empty(),
            "expected non-empty semantic tokens for a native file"
        );

        // Decode: `struct Cue {` and `    speaker: string,` happen to lex
        // identically under both frontends (`struct`/`Cue`/`speaker`/
        // `string` are all plain `IDENT`/keyword tokens either way), so a
        // token genuinely does land at these exact (line, start_char)
        // coordinates even pre-fix — the bug is not a missing token, it's
        // the wrong *classification*: with the ink classifier misrouted
        // onto native source (this issue's bug), `Cue` and `speaker` both
        // decode to `variable` (no `STRUCT_DECL`/`STRUCT_FIELD` parent
        // exists in ink's `SyntaxKind`). Reviewed 2026-08-01: an earlier
        // version of this comment claimed no token would land here at all,
        // which was false and is corrected here — what actually changes
        // between pre- and post-fix is the `token_type` this test asserts
        // below, not token presence.
        let type_at = |line: u64, col: u64| {
            let found = tokens
                .iter()
                .find(|t| t["line"] == line && t["start_char"] == col);
            assert!(
                found.is_some(),
                "no token at line {line} col {col}: {tokens:?}"
            );
            found.expect("checked above")["token_type"]
                .as_u64()
                .expect("token_type is a number")
        };

        // Line 0: `struct Cue {` — `Cue` starts at column 7.
        assert_eq!(
            type_at(0, 7),
            TT_STRUCT,
            "the struct's own name must not read as `variable`"
        );
        // Line 1: `    speaker: string,` — `speaker` at column 4.
        assert_eq!(
            type_at(1, 4),
            TT_PROPERTY,
            "a struct field name must not read as `variable`"
        );
        assert_ne!(type_at(0, 7), TT_VARIABLE);
        assert_ne!(type_at(1, 4), TT_VARIABLE);
    }

    /// #2291: `line_contexts()` on a native (`.brink`) file must route
    /// through `IdeSession::syntax_root_native` +
    /// `line_context::line_contexts_native`, never `syntax_root`'s
    /// always-ink parse (`IdeSession::syntax_root`'s own doc comment,
    /// #2280's failure mode) — mirrors the reachability proof of
    /// `native_folding_ranges_reach_the_native_cst_path` in
    /// `crate::editor::tests` (`crates/brink-web/src/editor/mod.rs`, NOT
    /// the sibling `folding` submodule), same fixture shape, exercised
    /// through the real `EditorSession::line_contexts()` entry point end to
    /// end on a `.brink` file, the same way a host would.
    ///
    /// Note on this assertion's scope: see the canonical caveat (#2471) on
    /// that `mod.rs` test for the full reasoning; it applies here
    /// unchanged, substituting `line_contexts_native`'s block-comment
    /// classification for the fold-run classification. That classification
    /// is proved end to end against the real native CST in brink-ide's own
    /// `line_context::tests::native_block_comment_uses_the_native_cst`
    /// (red-first-verified there). This test only pins that the
    /// wasm-facing entry point actually reaches that native path and
    /// reports the comment correctly, per #2291's reachability
    /// requirement.
    #[test]
    fn native_line_contexts_reach_the_native_cst_path() {
        let mut s = EditorSession::new();
        s.update_file(
            "main.brink",
            "flow main() {\n/* a\nblock */\nHello -> END\n}\n",
        );
        assert!(s.set_active_file("main.brink"));

        let json = s.line_contexts();
        let ctx: serde_json::Value =
            serde_json::from_str(&json).expect("line_contexts returns valid JSON");
        let array = ctx.as_array().expect("array");
        assert_eq!(
            array[1]["block_comment"],
            serde_json::json!(true),
            "the block comment's first line must be classified as such: {ctx}"
        );
        assert_eq!(
            array[2]["block_comment"],
            serde_json::json!(true),
            "the block comment's second line must be classified as such: {ctx}"
        );
        assert_eq!(
            array[3]["block_comment"],
            serde_json::json!(false),
            "the line after the comment must be untouched by it: {ctx}"
        );
    }
}
