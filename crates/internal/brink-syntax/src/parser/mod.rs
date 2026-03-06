mod choice;
mod content;
mod declaration;
mod divert;
mod expression;
mod gather;
mod inline;
mod knot;
mod logic;
mod story;
mod tag;

use crate::SyntaxKind::{self, COLON, EOF, ERROR, IDENT, L_BRACE, NEWLINE, PIPE, R_BRACE};
use crate::lexer;
use rowan::GreenNode;

/// Result of parsing an Ink source file.
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

/// A parse error with a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
}

/// Parse an Ink source string into a lossless CST.
#[must_use]
pub fn parse(source: &str) -> Parse {
    let raw_tokens = lexer::lex(source);
    let mut p = Parser::new(&raw_tokens);
    story::source_file(&mut p);
    let green = p.builder.finish();
    Parse {
        green,
        errors: p.errors,
    }
}

/// Parse with a shared [`rowan::NodeCache`] for green-node interning.
///
/// Re-parsing the same source through the same cache produces structurally
/// identical subtrees that share the same `Arc` allocation, enabling O(1)
/// pointer-equality checks via `GreenNode::eq`.
pub fn parse_with_cache(source: &str, cache: &mut rowan::NodeCache) -> Parse {
    let raw_tokens = lexer::lex(source);
    let mut p = Parser::with_cache(&raw_tokens, cache);
    story::source_file(&mut p);
    let green = p.builder.finish();
    Parse {
        green,
        errors: p.errors,
    }
}

// ── Parser internals ────────────────────────────────────────────────

/// Maximum nesting depth for recursive grammar rules (inline logic, expressions,
/// parenthesized groups). Prevents stack overflow and superlinear parse time on
/// pathological input. 256 matches Rust's default `recursion_limit`.
const MAX_DEPTH: u32 = 256;

/// The parser. Holds a token stream and a `GreenNodeBuilder`.
pub(crate) struct Parser<'t, 'c> {
    tokens: &'t [(SyntaxKind, &'t str)],
    pos: usize,
    depth: u32,
    /// Pre-computed scan results for each `{` token. Indexed by raw token
    /// position. For positions that are not `L_BRACE`, the value is meaningless.
    /// For `L_BRACE` positions, stores `PIPE`, `COLON`, or `EOF` indicating
    /// which delimiter appears first at depth-0 inside that brace pair.
    brace_scan: Vec<SyntaxKind>,
    builder: rowan::GreenNodeBuilder<'c>,
    errors: Vec<ParseError>,
}

impl<'t> Parser<'t, 'static> {
    fn new(tokens: &'t [(SyntaxKind, &'t str)]) -> Self {
        let brace_scan = Self::build_brace_scan(tokens);
        Self {
            tokens,
            pos: 0,
            depth: 0,
            brace_scan,
            builder: rowan::GreenNodeBuilder::new(),
            errors: Vec::new(),
        }
    }
}

impl<'t, 'c> Parser<'t, 'c> {
    fn with_cache(tokens: &'t [(SyntaxKind, &'t str)], cache: &'c mut rowan::NodeCache) -> Self {
        let brace_scan = Self::build_brace_scan(tokens);
        Self {
            tokens,
            pos: 0,
            depth: 0,
            brace_scan,
            builder: rowan::GreenNodeBuilder::with_cache(cache),
            errors: Vec::new(),
        }
    }

    /// One O(n) pre-pass over the token stream. For each `{`, determines whether
    /// `PIPE` or `COLON` appears first at depth-0 inside that brace pair.
    /// Stores the result so `scan_for_pipe_or_colon` becomes an O(1) lookup.
    fn build_brace_scan(tokens: &[(SyntaxKind, &str)]) -> Vec<SyntaxKind> {
        let n = tokens.len();
        let mut result = vec![EOF; n];

        // Stack of (brace_raw_pos, first_delimiter_seen).
        // `first_delimiter_seen` is PIPE, COLON, or EOF (neither yet).
        let mut stack: Vec<(usize, SyntaxKind)> = Vec::new();

        for (i, &(kind, _)) in tokens.iter().enumerate() {
            if kind.is_trivia() {
                continue;
            }
            match kind {
                L_BRACE => {
                    stack.push((i, EOF));
                }
                R_BRACE => {
                    if let Some((brace_pos, delim)) = stack.pop() {
                        result[brace_pos] = delim;
                    }
                    // Unmatched `}` — ignore
                }
                PIPE => {
                    // Only affects the innermost open brace (depth-0 relative to it)
                    if let Some((_, delim)) = stack.last_mut()
                        && *delim == EOF
                    {
                        *delim = PIPE;
                    }
                }
                COLON => {
                    if let Some((_, delim)) = stack.last_mut()
                        && *delim == EOF
                    {
                        *delim = COLON;
                    }
                }
                NEWLINE => {
                    // Newline terminates unclosed braces on the current line.
                    // Pop all open braces — they're unclosed within this line.
                    while let Some((brace_pos, delim)) = stack.pop() {
                        result[brace_pos] = delim;
                    }
                }
                _ => {}
            }
        }

        // Anything left on the stack is unclosed at EOF
        for (brace_pos, delim) in stack {
            result[brace_pos] = delim;
        }

        result
    }

    /// Returns `true` if the nesting depth limit has been reached.
    fn at_depth_limit(&self) -> bool {
        self.depth >= MAX_DEPTH
    }

    /// Look up the pre-computed scan result for a `{` token at the given raw
    /// position. Returns `PIPE`, `COLON`, or `EOF`.
    fn brace_scan_at(&self, raw_pos: usize) -> SyntaxKind {
        self.brace_scan.get(raw_pos).copied().unwrap_or(EOF)
    }

    // ── Lookahead ───────────────────────────────────────────────

    /// The kind of the current token (or `EOF` if past the end).
    fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    /// Lookahead by `n` tokens, skipping trivia (WHITESPACE, comments).
    /// `nth(0)` returns the current non-trivia token.
    fn nth(&self, n: usize) -> SyntaxKind {
        let mut i = self.pos;
        let mut remaining = n;
        while i < self.tokens.len() {
            let kind = self.tokens[i].0;
            if !kind.is_trivia() {
                if remaining == 0 {
                    return kind;
                }
                remaining -= 1;
            }
            i += 1;
        }
        EOF
    }

    /// Lookahead by `n` tokens WITHOUT skipping trivia.
    fn nth_raw(&self, n: usize) -> SyntaxKind {
        self.tokens.get(self.pos + n).map_or(EOF, |&(k, _)| k)
    }

    /// Returns `true` if the current non-trivia token matches `kind`.
    fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    /// Returns `true` if we're at end-of-file.
    fn at_eof(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    /// Current position in the token stream (for loop-progress checks).
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

    /// Bump the current token, asserting its kind matches `kind`.
    fn bump_assert(&mut self, kind: SyntaxKind) {
        debug_assert_eq!(self.nth_raw(0), kind);
        self.bump();
    }

    /// If the current non-trivia token matches `kind`, eat trivia then bump it.
    /// Returns `true` if consumed.
    fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.current() == kind {
            self.skip_ws();
            self.bump();
            true
        } else {
            false
        }
    }

    /// Expect the current non-trivia token to be `kind`. If it is, eat trivia
    /// and bump. Otherwise, emit an error.
    fn expect(&mut self, kind: SyntaxKind) {
        if !self.eat(kind) {
            self.error(format!("expected {kind:?}"));
        }
    }

    /// Returns `true` if the current non-trivia token is `IDENT` or a keyword.
    ///
    /// Ink keywords are contextual — they may appear as identifiers in some
    /// positions (e.g. list member names like `or`, `and`, `not`).
    fn at_ident_or_keyword(&self) -> bool {
        self.current() == IDENT || self.current().is_keyword()
    }

    /// If the current non-trivia token is `IDENT` or a keyword, eat trivia
    /// then bump it. Returns `true` if consumed.
    fn eat_ident_or_keyword(&mut self) -> bool {
        if self.at_ident_or_keyword() {
            self.skip_ws();
            self.bump();
            true
        } else {
            false
        }
    }

    /// Expect the current non-trivia token to be `IDENT` or a keyword.
    /// If not, emit an error.
    fn expect_ident_or_keyword(&mut self) {
        if !self.eat_ident_or_keyword() {
            self.error("expected IDENT".into());
        }
    }

    /// Consume all trivia (`WHITESPACE`, `LINE_COMMENT`, `BLOCK_COMMENT`).
    fn skip_ws(&mut self) {
        while self.pos < self.tokens.len() && self.tokens[self.pos].0.is_trivia() {
            self.bump();
        }
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

    /// Record a parse error at the current position.
    fn error(&mut self, message: String) {
        self.errors.push(ParseError { message });
    }

    /// Wrap the current token in an `ERROR` node and advance.
    ///
    /// Used by grammar rules that need to recover from unexpected tokens
    /// without losing the rest of the input.
    fn error_recover(&mut self, message: &str) {
        self.error(message.to_owned());
        self.start_node(ERROR);
        self.bump();
        self.finish_node();
    }
}

#[cfg(test)]
mod tests;
