use crate::{Block, Content, ContentPart, Stmt, Tag};

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
    parts: Vec<ContentPart>,
    last_pushed_was_content: bool,
}

impl<B: BodyBackend> ContentAccumulator<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            parts: Vec::new(),
            last_pushed_was_content: false,
        }
    }

    // ── Content part buffering ──────────────────────────────────

    pub fn push_text(&mut self, text: String) {
        if !text.is_empty() {
            self.parts.push(ContentPart::Text(text));
        }
    }

    pub fn push_glue(&mut self) {
        self.parts.push(ContentPart::Glue);
    }

    pub fn push_escape(&mut self, text: &str) {
        if text.len() > 1 {
            self.parts.push(ContentPart::Text(text[1..].to_string()));
        }
    }

    pub fn has_buffered_parts(&self) -> bool {
        !self.parts.is_empty()
    }

    pub fn ends_with_glue(&self) -> bool {
        content_ends_with_glue(&self.parts)
    }

    // ── Flushing ────────────────────────────────────────────────

    /// Flush buffered content parts as a `Stmt::Content`.
    pub fn flush(&mut self) {
        if !self.parts.is_empty() {
            self.backend.push_stmt(Stmt::Content(Content {
                ptr: None,
                parts: std::mem::take(&mut self.parts),
                tags: Vec::new(),
            }));
            self.last_pushed_was_content = true;
        }
    }

    /// Flush with tags.
    pub fn flush_with_tags(&mut self, tags: Vec<Tag>) {
        if !self.parts.is_empty() || !tags.is_empty() {
            self.backend.push_stmt(Stmt::Content(Content {
                ptr: None,
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
        self.backend.push_stmt(Stmt::Content(Content {
            ptr: None,
            parts: Vec::new(),
            tags: output.tags,
        }));
        self.last_pushed_was_content = true;
        self.push_eol();
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
