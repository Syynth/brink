mod annotation;
mod block;
mod choice;
mod content;
mod decl;
mod divert;
mod doc_comment;
mod expr;
mod family;
mod source_file;
mod stmt;
#[cfg(test)]
mod tests;

use crate::SyntaxKind::{self, ERROR};
use crate::lexer;
use rowan::GreenNode;

/// Result of parsing a `.brink` source file.
///
/// `PartialEq` compares the green tree structurally (rowan `GreenNode`
/// equality is content-based) plus the error list.
#[derive(Clone, PartialEq, Eq)]
pub struct Parse {
    green: GreenNode,
    errors: Vec<ParseError>,
}

impl Parse {
    /// The root green node of the lossless CST.
    #[must_use]
    pub fn green(&self) -> &GreenNode {
        &self.green
    }

    /// The root syntax node (typed wrapper around the green tree).
    #[must_use]
    pub fn syntax(&self) -> crate::SyntaxNode {
        crate::SyntaxNode::new_root(self.green.clone())
    }

    /// Parse errors encountered.
    #[must_use]
    pub fn errors(&self) -> &[ParseError] {
        &self.errors
    }
}

/// A parse diagnostic's severity — whether it blocks compilation.
///
/// This crate has no `brink-ir` dependency (peer-crate rule, `lib.rs`'s
/// doc comment), so this stays a small local enum rather than reusing
/// `brink_ir::Severity` — consumers (`brink-db`'s `lower_native_file`) map
/// it onto the appropriate `DiagnosticCode` (`E037` for `Error`, a
/// dedicated Warning-severity code for `Warning`) at the seam where the two
/// diagnostic vocabularies meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseSeverity {
    /// Malformed source — blocks compilation.
    Error,
    /// Advisory only — surfaced to the user but never blocks compilation
    /// (issue #1263: `<-` outside a choice point *can* be literal dialogue,
    /// so a hard error would be wrong).
    Warning,
}

/// A parse error with a message and the source range it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    /// Byte range in the source that the error points at.
    pub range: rowan::TextRange,
    /// Whether this diagnostic blocks compilation. Defaults to `Error` for
    /// every existing diagnostic (`Parser::error`); only `Parser::warning`
    /// produces `Warning`.
    pub severity: ParseSeverity,
}

/// Parse a `.brink` source string into a lossless CST.
#[must_use]
pub fn parse(source: &str) -> Parse {
    let raw_tokens = lexer::lex(source);
    let mut p = Parser::new(&raw_tokens);
    source_file::source_file(&mut p);
    let green = p.builder.finish();
    Parse {
        green,
        errors: p.errors,
    }
}

/// Parse with a shared [`rowan::NodeCache`] for green-node interning.
pub fn parse_with_cache(source: &str, cache: &mut rowan::NodeCache) -> Parse {
    let raw_tokens = lexer::lex(source);
    let mut p = Parser::with_cache(&raw_tokens, cache);
    source_file::source_file(&mut p);
    let green = p.builder.finish();
    Parse {
        green,
        errors: p.errors,
    }
}

// ── Parser internals ────────────────────────────────────────────────

/// Maximum nesting depth for recursive grammar rules (blocks, expressions,
/// parenthesized groups). Prevents stack overflow and superlinear parse
/// time on pathological/adversarial input. 256 matches Rust's default
/// `recursion_limit`.
const MAX_DEPTH: u32 = 256;

/// The parser. Holds a token stream and a `GreenNodeBuilder`.
pub(crate) struct Parser<'t, 'c> {
    tokens: &'t [(SyntaxKind, &'t str)],
    pos: usize,
    depth: u32,
    /// Pre-computed non-trivia token indices. `non_trivia[k]` is the raw
    /// token index of the k-th non-trivia token. Enables O(1) `nth(n)`
    /// instead of an O(n) rescan per lookahead — this parser calls `nth`
    /// in hot loops (block/content dispatch), so an un-indexed scan would
    /// make parsing a large file superlinear.
    non_trivia: Vec<usize>,
    builder: rowan::GreenNodeBuilder<'c>,
    errors: Vec<ParseError>,
}

impl<'t> Parser<'t, 'static> {
    fn new(tokens: &'t [(SyntaxKind, &'t str)]) -> Self {
        let non_trivia = Self::build_non_trivia(tokens);
        Self {
            tokens,
            pos: 0,
            depth: 0,
            non_trivia,
            builder: rowan::GreenNodeBuilder::new(),
            errors: Vec::new(),
        }
    }
}

impl<'t, 'c> Parser<'t, 'c> {
    fn with_cache(tokens: &'t [(SyntaxKind, &'t str)], cache: &'c mut rowan::NodeCache) -> Self {
        let non_trivia = Self::build_non_trivia(tokens);
        Self {
            tokens,
            pos: 0,
            depth: 0,
            non_trivia,
            builder: rowan::GreenNodeBuilder::with_cache(cache),
            errors: Vec::new(),
        }
    }

    /// O(n) pre-pass: collect the raw indices of all non-trivia tokens.
    fn build_non_trivia(tokens: &[(SyntaxKind, &str)]) -> Vec<usize> {
        tokens
            .iter()
            .enumerate()
            .filter(|(_, (k, _))| !k.is_trivia())
            .map(|(i, _)| i)
            .collect()
    }

    /// Enter one level of recursive-grammar nesting. Returns `false` (and
    /// records an error) if `MAX_DEPTH` would be exceeded — callers must
    /// bail out without recursing further, still consuming forward
    /// progress via `error_recover`. Every mutually-recursive entry point
    /// (blocks, the annotated-brace family, expressions) pairs this with
    /// `exit_depth` so pathological/adversarial nesting can never blow the
    /// stack (CLAUDE.md: "guard against unbounded growth").
    fn enter_depth(&mut self) -> bool {
        if self.depth >= MAX_DEPTH {
            self.error("maximum nesting depth exceeded".into());
            false
        } else {
            self.depth += 1;
            true
        }
    }

    /// Leave one level entered by `enter_depth`.
    fn exit_depth(&mut self) {
        self.depth -= 1;
    }

    // ── Lookahead ───────────────────────────────────────────────

    /// The kind of the current token (or `EOF` if past the end).
    fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    /// Lookahead by `n` tokens, skipping trivia (WHITESPACE, comments).
    /// `nth(0)` returns the current non-trivia token.
    fn nth(&self, n: usize) -> SyntaxKind {
        let start = self.non_trivia.partition_point(|&idx| idx < self.pos);
        let target = start + n;
        if target < self.non_trivia.len() {
            self.tokens[self.non_trivia[target]].0
        } else {
            SyntaxKind::EOF
        }
    }

    /// Lookahead by `n` tokens WITHOUT skipping trivia.
    fn nth_raw(&self, n: usize) -> SyntaxKind {
        self.tokens
            .get(self.pos + n)
            .map_or(SyntaxKind::EOF, |&(k, _)| k)
    }

    /// Returns `true` if the current non-trivia token matches `kind`.
    fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    /// Returns `true` if we're at end-of-file.
    fn at_eof(&self) -> bool {
        self.current() == SyntaxKind::EOF
    }

    /// Current position in the raw token stream (for loop-progress checks).
    fn pos(&self) -> usize {
        self.pos
    }

    // ── Consumption ─────────────────────────────────────────────

    /// Emit the current token to the builder and advance.
    fn bump(&mut self) {
        if self.pos < self.tokens.len() {
            let (kind, text) = self.tokens[self.pos];
            self.builder.token(rowan::SyntaxKind(kind as u16), text);
            self.pos += 1;
        }
    }

    /// If the current non-trivia token matches `kind`, eat trivia then bump it.
    /// Returns `true` if consumed.
    fn eat(&mut self, kind: SyntaxKind) -> bool {
        // Flush leading trivia *unconditionally*, before the check — not
        // only on a match. Two correctness properties depend on this:
        // (1) trailing trivia with nothing meaningful after it (a final
        // comment, trailing whitespace at EOF) would otherwise never get
        // flushed into the tree at all, since every loop-continuation
        // check (`at_eof`, `at(R_BRACE)`, …) trivia-skips to decide
        // "nothing left to do" without ever having called `bump` on the
        // trivia itself — found by `proptest_native`'s
        // `arbitrary_garbage_never_panics` (`"#//"` lost its trailing
        // `//`) and `truncated_input_never_panics_and_roundtrips` (a
        // truncated `flow a_a_() ` lost its trailing space). (2) it makes
        // every `eat`/`expect` call site safe to follow with a raw
        // `bump()` for a *different* token regardless of whether pending
        // trivia sat between them — the class of bug this crate's parser
        // tests caught repeatedly during development (e.g. `annotation_arg`
        // bumping a stray space instead of the next `IDENT`).
        self.skip_ws();
        if self.current() == kind {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Expect the current non-trivia token to be `kind`. If it is, eat
    /// trivia and bump. Otherwise, emit an error (no token consumed —
    /// callers that need forward progress on mismatch should follow up
    /// with `error_recover`).
    fn expect(&mut self, kind: SyntaxKind) {
        if !self.eat(kind) {
            self.error(format!("expected {kind:?}, found {:?}", self.current()));
        }
    }

    /// Consume all trivia (`WHITESPACE`, `LINE_COMMENT`, `BLOCK_COMMENT`).
    fn skip_ws(&mut self) {
        while self.pos < self.tokens.len() && self.tokens[self.pos].0.is_trivia() {
            self.bump();
        }
    }

    /// Consume all trivia **and** `NEWLINE` tokens.
    ///
    /// `NEWLINE` is deliberately not trivia (it terminates content
    /// lines/diverts/etc. at body-item position) — but inside an
    /// explicitly bracket/brace-delimited list (param lists, struct
    /// fields, annotation args, `use`-tree lists, match arms, …), a line
    /// break is pure formatting, exactly the case the charter's "whitespace
    /// never load-bearing" ground rule (§2) describes. Every such list
    /// loop calls this instead of `skip_ws` so multi-line lists parse.
    fn skip_ws_and_newlines(&mut self) {
        while self.pos < self.tokens.len()
            && (self.tokens[self.pos].0.is_trivia()
                || self.tokens[self.pos].0 == SyntaxKind::NEWLINE)
        {
            self.bump();
        }
    }

    /// Look at the next significant token, skipping trivia **and**
    /// `NEWLINE` (read-only — does not move `pos`). The lookahead half of
    /// [`Self::skip_ws_and_newlines`]'s policy, for list loops that need to
    /// check a closing delimiter before deciding whether to recurse.
    fn peek_skip_nl(&self) -> SyntaxKind {
        let mut i = self.pos;
        while i < self.tokens.len()
            && (self.tokens[i].0.is_trivia() || self.tokens[i].0 == SyntaxKind::NEWLINE)
        {
            i += 1;
        }
        self.tokens.get(i).map_or(SyntaxKind::EOF, |&(k, _)| k)
    }

    // ── Nodes ───────────────────────────────────────────────────

    /// Start a new CST node.
    fn start_node(&mut self, kind: SyntaxKind) {
        self.builder.start_node(rowan::SyntaxKind(kind as u16));
    }

    /// Start a new CST node at a previously saved checkpoint.
    fn start_node_at(&mut self, checkpoint: rowan::Checkpoint, kind: SyntaxKind) {
        self.builder
            .start_node_at(checkpoint, rowan::SyntaxKind(kind as u16));
    }

    /// Finish the current CST node.
    fn finish_node(&mut self) {
        self.builder.finish_node();
    }

    /// Save the current position as a checkpoint for `start_node_at`.
    fn checkpoint(&self) -> rowan::Checkpoint {
        self.builder.checkpoint()
    }

    // ── Errors ──────────────────────────────────────────────────

    /// Record a parse diagnostic at the current position with the given
    /// severity. Shared implementation for [`Self::error`]/[`Self::warning`].
    fn push_diagnostic(&mut self, message: String, severity: ParseSeverity) {
        let upto = self.pos.min(self.tokens.len());
        let start: usize = self.tokens[..upto].iter().map(|(_, t)| t.len()).sum();
        let len: usize = self.tokens.get(self.pos).map_or(0, |(_, t)| t.len());
        let start = rowan::TextSize::from(u32::try_from(start).unwrap_or(u32::MAX));
        let len = rowan::TextSize::from(u32::try_from(len).unwrap_or(u32::MAX));
        self.errors.push(ParseError {
            message,
            range: rowan::TextRange::at(start, len),
            severity,
        });
    }

    /// Record a parse error at the current position. Blocks compilation
    /// (`ParseSeverity::Error`).
    fn error(&mut self, message: String) {
        self.push_diagnostic(message, ParseSeverity::Error);
    }

    /// Record a warning-severity diagnostic at the current position.
    /// Advisory only — never blocks compilation (`ParseSeverity::Warning`).
    fn warning(&mut self, message: String) {
        self.push_diagnostic(message, ParseSeverity::Warning);
    }

    /// Wrap the current token in an `ERROR` node and advance.
    ///
    /// Used by grammar rules that need to recover from unexpected tokens
    /// without losing the rest of the input. Guarantees forward progress
    /// even at EOF-adjacent malformed input, as long as at least one raw
    /// token remains — callers at the very top (`source_file`) additionally
    /// guard against a zero-progress spin when even that isn't true.
    fn error_recover(&mut self, message: &str) {
        self.error(message.to_owned());
        self.start_node(ERROR);
        if self.pos < self.tokens.len() {
            self.bump();
        }
        self.finish_node();
    }
}
