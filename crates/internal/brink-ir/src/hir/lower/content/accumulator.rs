use rowan::TextRange;

use crate::{Block, Content, ContentPart, FileId, KindToken, NodeClass, Provenance, Stmt, Tag};

use super::super::context::{LowerScope, LowerSink};
use super::super::helpers::content_ends_with_glue;
use super::content_line::ContentLineOutput;
use super::inline_logic::InlineLogicOutput;
use super::logic_line::LogicLineOutput;
use super::tag_line::TagLineOutput;
use super::{BodyBackend, HandleResult, Integrate, LowerBody};

/// Accumulates content parts and block-level statements, flushing
/// buffered parts when block-level nodes appear.
///
/// Generic over [`BodyBackend`] — the backend determines where results go.
pub struct ContentAccumulator<B: BodyBackend> {
    backend: B,
    file_id: FileId,
    parts: Vec<ContentPart>,
    /// Source range covering every part pushed since the last flush,
    /// tracked only by callers that buffer raw tokens (branch bodies — see
    /// [`Self::note_range`]'s doc). Top-level content lines never call the
    /// range-tracking pushers: their whole-line `ptr` comes from
    /// `ContentLineOutput::Content` instead, so this stays `None` for them
    /// and `flush` produces `ptr: None` exactly as before.
    pending_range: Option<TextRange>,
    last_pushed_was_content: bool,
}

impl<B: BodyBackend> ContentAccumulator<B> {
    pub fn new(backend: B, file_id: FileId) -> Self {
        Self {
            backend,
            file_id,
            parts: Vec::new(),
            pending_range: None,
            last_pushed_was_content: false,
        }
    }

    // ── Content part buffering ──────────────────────────────────

    /// Extend the pending source range for buffered raw-token parts (issue
    /// #981). Branch bodies (`brink_syntax`'s multiline conditional/sequence
    /// arms) have no per-line `CONTENT_LINE` wrapper node to hang a `ptr` off
    /// of — their content is raw `TEXT`/`GLUE_NODE`/`ESCAPE`/`INLINE_LOGIC`
    /// tokens accumulated directly here — so `flush` synthesizes one instead,
    /// covering every token range noted since the last flush. The synthetic
    /// provenance never resolves back to a live syntax node (no single node
    /// spans exactly this union), the same posture
    /// `conditional_with_expr::branchless_first_arm_span` already uses for
    /// a branch's own span — but it carries a real byte range for
    /// span-consuming tools (the HIR projection, folding, diagnostics).
    pub fn note_range(&mut self, range: TextRange) {
        self.pending_range = Some(match self.pending_range.take() {
            Some(r) => r.cover(range),
            None => range,
        });
    }

    pub fn push_text(&mut self, text: String, range: TextRange) {
        if !text.is_empty() {
            self.parts.push(ContentPart::Text(text));
            self.note_range(range);
        }
    }

    pub fn push_glue(&mut self, range: TextRange) {
        self.parts.push(ContentPart::Glue);
        self.note_range(range);
    }

    /// [`Self::push_glue`], preceded by a `Spring` when whitespace separated
    /// the glue from an inline construct (issue #3507) — see
    /// [`super::helpers::push_glue`].
    pub fn push_glue_after(&mut self, range: TextRange, ws_before_glue: bool) {
        super::helpers::push_glue(&mut self.parts, ws_before_glue);
        self.note_range(range);
    }

    pub fn push_escape(&mut self, text: &str, range: TextRange) {
        if text.len() > 1 {
            self.parts.push(ContentPart::Text(text[1..].to_string()));
            self.note_range(range);
        }
    }

    pub fn has_buffered_parts(&self) -> bool {
        !self.parts.is_empty()
    }

    pub fn ends_with_glue(&self) -> bool {
        content_ends_with_glue(&self.parts)
    }

    /// Whether the last buffered part is an inline construct — the only
    /// thing whitespace-before-glue lowers to a `Spring` after (issue
    /// #3507, `super::helpers::push_glue`).
    pub fn last_part_is_inline_construct(&self) -> bool {
        super::helpers::is_inline_construct(self.parts.last())
    }

    // ── Flushing ────────────────────────────────────────────────

    /// Flush buffered content parts as a `Stmt::Content`.
    pub fn flush(&mut self) {
        // Deliberately `Provenance::new` with the REAL file id + a
        // synthetic kind token, not `Provenance::synthetic()` (which
        // stamps `FileId(u32::MAX)`): the range is genuine source text, so
        // range-only consumers (admission's E124 range check, diagnostic
        // anchors, projection spans) must attribute it to the right file;
        // only kind-based CST resolution is meant to decline (the
        // `branchless_first_arm_span` posture, #981).
        let ptr = self.pending_range.take().map(|range| {
            Provenance::new(
                self.file_id,
                range,
                KindToken::synthetic(NodeClass::Content),
            )
        });
        if !self.parts.is_empty() {
            self.backend.push_stmt(Stmt::Content(Content {
                ptr,
                parts: std::mem::take(&mut self.parts),
                tags: Vec::new(),
            }));
            self.last_pushed_was_content = true;
        }
    }

    /// Flush with tags.
    pub fn flush_with_tags(&mut self, tags: Vec<Tag>) {
        let ptr = self.pending_range.take().map(|range| {
            Provenance::new(
                self.file_id,
                range,
                KindToken::synthetic(NodeClass::Content),
            )
        });
        if !self.parts.is_empty() || !tags.is_empty() {
            self.backend.push_stmt(Stmt::Content(Content {
                ptr,
                parts: std::mem::take(&mut self.parts),
                tags,
            }));
            self.last_pushed_was_content = true;
        }
    }

    pub fn push_eol(&mut self) {
        self.backend.push_stmt(Stmt::EndOfLine);
        self.last_pushed_was_content = false;
    }

    pub fn last_was_content(&self) -> bool {
        self.last_pushed_was_content
    }

    // ── Block-level dispatch via traits ─────────────────────────

    /// Lower a node via [`LowerBody`], then integrate its output.
    ///
    /// Returns [`HandleResult`] indicating whether the output was
    /// block-level or inline. Most callers ignore this; branch bodies
    /// use it for whitespace tracking around inline logic.
    pub fn handle<N: LowerBody>(
        &mut self,
        node: &N,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> HandleResult
    where
        Self: Integrate<N::Output>,
    {
        match node.lower_body(scope, sink) {
            Ok(output) => self.integrate(output),
            Err(_) => HandleResult::Inline,
        }
    }

    /// Push a raw statement (bypasses part buffering).
    pub fn push_stmt(&mut self, stmt: Stmt) {
        self.last_pushed_was_content = matches!(&stmt, Stmt::Content(_));
        self.backend.push_stmt(stmt);
    }

    // ── Backend access ───────────────────────────────────────────

    /// Access the backend directly for backend-specific operations
    /// (e.g., `WeaveBackend::push_choice`).
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    // ── Finish ──────────────────────────────────────────────────

    pub fn finish(mut self) -> Block {
        self.flush();
        self.backend.finish()
    }
}

// ─── Integrate impls ────────────────────────────────────────────────

impl<B: BodyBackend> Integrate<ContentLineOutput> for ContentAccumulator<B> {
    fn integrate(&mut self, output: ContentLineOutput) -> HandleResult {
        match output {
            ContentLineOutput::Content {
                content,
                divert,
                ends_with_glue,
            } => {
                self.backend.push_stmt(Stmt::Content(content));
                self.last_pushed_was_content = true;
                if let Some(d) = divert {
                    self.backend.push_stmt(d);
                    self.last_pushed_was_content = false;
                } else if !ends_with_glue {
                    self.push_eol();
                }
                HandleResult::Block
            }
            ContentLineOutput::BareDivert(stmt) => {
                self.backend.push_stmt(stmt);
                self.last_pushed_was_content = false;
                HandleResult::Block
            }
            ContentLineOutput::PromotedBlock {
                stmt,
                trailing_content,
                divert,
                needs_eol,
            } => {
                self.backend.push_stmt(stmt);
                self.last_pushed_was_content = false;
                if let Some(tc) = trailing_content {
                    self.backend.push_stmt(Stmt::Content(tc));
                    self.last_pushed_was_content = true;
                }
                if let Some(d) = divert {
                    self.backend.push_stmt(d);
                    self.last_pushed_was_content = false;
                } else if needs_eol {
                    self.push_eol();
                }
                HandleResult::Block
            }
            ContentLineOutput::Empty => HandleResult::Inline,
        }
    }
}

impl<B: BodyBackend> Integrate<LogicLineOutput> for ContentAccumulator<B> {
    fn integrate(&mut self, output: LogicLineOutput) -> HandleResult {
        self.flush();
        let needs_eol = output.has_call();
        self.backend.push_stmt(output.into_stmt());
        self.last_pushed_was_content = false;
        if needs_eol {
            self.push_eol();
        }
        HandleResult::Block
    }
}

impl<B: BodyBackend> Integrate<Stmt> for ContentAccumulator<B> {
    fn integrate(&mut self, stmt: Stmt) -> HandleResult {
        self.flush();
        self.last_pushed_was_content = false;
        self.backend.push_stmt(stmt);
        HandleResult::Block
    }
}

impl<B: BodyBackend> Integrate<Option<Stmt>> for ContentAccumulator<B> {
    fn integrate(&mut self, output: Option<Stmt>) -> HandleResult {
        if let Some(stmt) = output {
            self.flush();
            self.last_pushed_was_content = false;
            self.backend.push_stmt(stmt);
            HandleResult::Block
        } else {
            HandleResult::Inline
        }
    }
}

impl<B: BodyBackend> Integrate<TagLineOutput> for ContentAccumulator<B> {
    fn integrate(&mut self, output: TagLineOutput) -> HandleResult {
        if output.tags.is_empty() {
            return HandleResult::Inline;
        }
        self.flush();
        // Issue #3534: a tag-only line contributes its tags and NOTHING
        // else — ink's parser appends a line's `"\n"` only when the line
        // is not pure tags (`lineIsPureTag`), so the tags ride the next
        // line's newline. No `EndOfLine` here, and the tags are not
        // "content" for the surrounding block's own newline bookkeeping
        // either (a multi-line block's trailing newline is keyed on
        // `last_was_content`).
        self.backend.push_stmt(Stmt::Content(Content {
            ptr: None,
            parts: Vec::new(),
            tags: output.tags,
        }));
        self.last_pushed_was_content = false;
        HandleResult::Block
    }
}

impl<B: BodyBackend> Integrate<InlineLogicOutput> for ContentAccumulator<B> {
    fn integrate(&mut self, output: InlineLogicOutput) -> HandleResult {
        match output {
            InlineLogicOutput::Block(stmt) => {
                self.flush();
                self.backend.push_stmt(stmt);
                self.last_pushed_was_content = false;
                HandleResult::Block
            }
            InlineLogicOutput::Inline(new_parts) => {
                self.parts.extend(new_parts);
                HandleResult::Inline
            }
        }
    }
}
