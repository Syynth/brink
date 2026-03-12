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
    /// Pre-computed non-trivia token indices. `non_trivia[k]` is the raw
    /// token index of the k-th non-trivia token. Enables O(1) `nth(n)`.
    non_trivia: Vec<usize>,
    builder: rowan::GreenNodeBuilder<'c>,
    errors: Vec<ParseError>,
}

impl<'t> Parser<'t, 'static> {
    fn new(tokens: &'t [(SyntaxKind, &'t str)]) -> Self {
        let brace_scan = Self::build_brace_scan(tokens);
        let non_trivia = Self::build_non_trivia(tokens);
        Self {
            tokens,
            pos: 0,
            depth: 0,
            brace_scan,
            non_trivia,
            builder: rowan::GreenNodeBuilder::new(),
            errors: Vec::new(),
        }
    }
}

impl<'t, 'c> Parser<'t, 'c> {
    fn with_cache(tokens: &'t [(SyntaxKind, &'t str)], cache: &'c mut rowan::NodeCache) -> Self {
        let brace_scan = Self::build_brace_scan(tokens);
        let non_trivia = Self::build_non_trivia(tokens);
        Self {
            tokens,
            pos: 0,
            depth: 0,
            brace_scan,
            non_trivia,
            builder: rowan::GreenNodeBuilder::with_cache(cache),
            errors: Vec::new(),
        }
    }

    /// O(n) pre-pass: collect the raw indices of all non-trivia tokens.
    /// Enables O(1) `nth(n)` lookup during parsing.
    fn build_non_trivia(tokens: &[(SyntaxKind, &str)]) -> Vec<usize> {
        tokens
            .iter()
            .enumerate()
            .filter(|(_, (k, _))| !k.is_trivia())
            .map(|(i, _)| i)
            .collect()
    }

    /// O(n) pre-pass: for each `L_BRACE`, classify the brace pair as `COLON`
    /// (conditional), `PIPE` (sequence), or `EOF` (bare expression).
    ///
    /// Classification rules (`||`-aware):
    ///  1. If a **single** `|` (not part of `||`) appears at depth-0 →
    ///     sequence (`PIPE`), regardless of any COLON.
    ///  2. Else if `COLON` appears at depth-0 → conditional (`COLON`).
    ///  3. Else if `||` appears (no single `|`, no COLON) → sequence (`PIPE`),
    ///     since `||` without a conditional colon means two separators.
    ///  4. Neither → bare expression (`EOF`).
    ///
    /// Examples:
    ///  - `{a|b:c}` — single `|` → sequence (rule 1)
    ///  - `{x || y: body}` — no single `|`, has COLON → conditional (rule 2)
    ///  - `{a||b}` — `||` only, no COLON → sequence (rule 3)
    ///  - `{x}` — neither → bare expression (rule 4)
    fn build_brace_scan(tokens: &[(SyntaxKind, &str)]) -> Vec<SyntaxKind> {
        // Stack entries track what we've seen at depth-0 inside each brace pair.
        // `single_pipe_before_colon` is the key signal: a lone `|` that appears
        // before any `:` means this brace pair is a sequence, not a conditional
        // (the `|` is a separator, not part of a conditional body like `{x: a|b}`).
        struct Entry {
            brace_pos: usize,
            has_colon: bool,
            has_pipe: bool,
            single_pipe_before_colon: bool,
        }

        fn classify(e: &Entry) -> SyntaxKind {
            if e.single_pipe_before_colon {
                PIPE // rule 1: single `|` before `:` → sequence
            } else if e.has_colon {
                COLON // rule 2: colon (with only `||` or no pipe before it) → conditional
            } else if e.has_pipe {
                PIPE // rule 3: `||` without colon → sequence separators
            } else {
                EOF // rule 4: bare expression
            }
        }

        let n = tokens.len();
        let mut result = vec![EOF; n];

        // Precompute: for each token position, the next non-trivia token index.
        let next_nt = {
            let mut v = vec![n; n];
            let mut last = n;
            for i in (0..n).rev() {
                v[i] = last;
                if !tokens[i].0.is_trivia() {
                    last = i;
                }
            }
            v
        };

        let mut stack: Vec<Entry> = Vec::new();
        let mut prev_nt = EOF;

        for (i, &(kind, _)) in tokens.iter().enumerate() {
            if kind.is_trivia() {
                continue;
            }
            match kind {
                L_BRACE => {
                    stack.push(Entry {
                        brace_pos: i,
                        has_colon: false,
                        has_pipe: false,
                        single_pipe_before_colon: false,
                    });
                    prev_nt = L_BRACE;
                }
                R_BRACE => {
                    if let Some(entry) = stack.pop() {
                        result[entry.brace_pos] = classify(&entry);
                    }
                    prev_nt = R_BRACE;
                }
                COLON => {
                    if let Some(e) = stack.last_mut() {
                        e.has_colon = true;
                    }
                    prev_nt = COLON;
                }
                PIPE => {
                    if let Some(e) = stack.last_mut() {
                        e.has_pipe = true;
                        // Determine if this is a single `|` (not part of `||`).
                        let next_is_pipe = next_nt[i] < n && tokens[next_nt[i]].0 == PIPE;
                        let prev_is_pipe = prev_nt == PIPE;
                        let is_single = !next_is_pipe && !prev_is_pipe;
                        // Only matters if we haven't seen COLON yet.
                        if is_single && !e.has_colon {
                            e.single_pipe_before_colon = true;
                        }
                    }
                    prev_nt = PIPE;
                }
                NEWLINE => {
                    while let Some(entry) = stack.pop() {
                        result[entry.brace_pos] = classify(&entry);
                    }
                    prev_nt = NEWLINE;
                }
                _ => {
                    prev_nt = kind;
                }
            }
        }

        for entry in stack {
            result[entry.brace_pos] = classify(&entry);
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
    ///
    /// Uses the pre-computed `non_trivia` index for O(log n + 1) lookup
    /// (binary search to find our position, then constant-time indexing).
    fn nth(&self, n: usize) -> SyntaxKind {
        // Find the first non-trivia index >= self.pos via binary search.
        let start = self.non_trivia.partition_point(|&idx| idx < self.pos);
        let target = start + n;
        if target < self.non_trivia.len() {
            self.tokens[self.non_trivia[target]].0
        } else {
            EOF
        }
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
