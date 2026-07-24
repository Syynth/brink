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
}

impl EditorSession {
    fn line_contexts_impl(&self, path: &str, view: Option<&ViewContext>) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(source), Some(root)) = (
            self.session.source(file_id),
            self.session.syntax_root(file_id),
        ) else {
            return "[]".to_owned();
        };

        let Some(projection) = self.session.projection(file_id) else {
            return "[]".to_owned();
        };
        let contexts = match self.session.dialect() {
            Some(dialect) => brink_ide::line_context::line_contexts_with_dialect(
                source,
                &root,
                &projection,
                dialect,
            ),
            None => brink_ide::line_context::line_contexts(source, &root, &projection),
        };
        if let Some(v) = view {
            let start = v.start_line as usize;
            let end_line = self
                .view_end_line(path, v)
                .map_or(contexts.len(), |l| l as usize);
            let slice = &contexts[start..end_line.min(contexts.len())];
            serde_json::to_string(slice).unwrap_or_default()
        } else {
            serde_json::to_string(&contexts).unwrap_or_default()
        }
    }

    fn semantic_tokens_impl(&self, path: &str, view: Option<&ViewContext>) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(source), Some(root)) = (
            self.session.analysis(),
            self.session.source(file_id),
            self.session.syntax_root(file_id),
        ) else {
            return "[]".to_owned();
        };

        let raw = brink_ide::semantic_tokens::semantic_tokens(source, &root, analysis, file_id);

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
        let (Some(hir), Some(analysis), Some(source)) = (
            self.session.hir(file_id),
            self.session.analysis(),
            self.session.source(file_id),
        ) else {
            return EMPTY.to_owned();
        };

        let _ = (hir, analysis);
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
                Some(HirSpanJs {
                    start_line,
                    start_char,
                    end_line,
                    end_char,
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
