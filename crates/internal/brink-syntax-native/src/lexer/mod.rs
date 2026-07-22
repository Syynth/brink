mod ident;
mod punctuation;
#[cfg(test)]
mod tests;

use crate::SyntaxKind;
pub use ident::classify_keyword;
pub use punctuation::lex_punctuation;

/// Lex `source` into a sequence of `(SyntaxKind, slice)` pairs.
///
/// Every byte of `source` appears in exactly one token — this is the
/// lossless-roundtrip invariant the CST and its proptests depend on. The
/// only mutable state is a `string_depth: u32` tracking nested string /
/// interpolation regions, mirroring `brink-syntax`'s lexer: a `"` toggles
/// string-scanning mode regardless of whether the surrounding context turns
/// out to be an expression string literal or literal dialogue-quote prose
/// text — the *parser*, not the lexer, decides which node shape a quoted
/// run gets, from its structural position (same one-token-stream,
/// position-decides-shape pattern `brace_scan` uses in the ink parser).
pub fn lex(source: &str) -> Vec<(SyntaxKind, &str)> {
    Lexer::new(source).run()
}

struct Lexer<'src> {
    source: &'src str,
    bytes: &'src [u8],
    pos: usize,
    /// Nesting depth of string interpolations. 0 = outside any string.
    /// Odd = in string mode, even = in code mode with pending string
    /// closings to track (a `{` inside a string increments depth and
    /// re-enters code mode for the interpolation; the matching `}`
    /// decrements depth and re-enters string mode).
    string_depth: u32,
    tokens: Vec<(SyntaxKind, &'src str)>,
}

impl<'src> Lexer<'src> {
    fn new(source: &'src str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            string_depth: 0,
            tokens: Vec::new(),
        }
    }

    fn in_string(&self) -> bool {
        self.string_depth % 2 == 1
    }

    fn run(mut self) -> Vec<(SyntaxKind, &'src str)> {
        while self.pos < self.bytes.len() {
            if self.in_string() {
                self.lex_string_token();
            } else {
                self.lex_code_token();
            }
        }
        self.tokens
    }

    fn emit(&mut self, kind: SyntaxKind, start: usize) {
        self.tokens.push((kind, &self.source[start..self.pos]));
    }

    // ── String-mode lexing ──────────────────────────────────────

    fn lex_string_token(&mut self) {
        let start = self.pos;
        let b = self.bytes[self.pos];

        // Closing quote — pop one level of string nesting.
        if b == b'"' {
            self.pos += 1;
            self.string_depth -= 1;
            self.emit(SyntaxKind::QUOTE, start);
            return;
        }

        // Escape sequence.
        if b == b'\\' && self.pos + 1 < self.bytes.len() {
            let next = self.bytes[self.pos + 1];
            if matches!(next, b'n' | b't' | b'\\' | b'"') {
                self.pos += 2;
                self.emit(SyntaxKind::STRING_ESCAPE, start);
                return;
            }
        }

        // Opening brace — enter interpolation (push depth). Lets
        // `{expr}` interpolation appear inside dialogue-quoted prose text.
        if b == b'{' {
            self.pos += 1;
            self.string_depth += 1;
            self.emit(SyntaxKind::L_BRACE, start);
            return;
        }

        // Brackets — emit as L_BRACKET/R_BRACKET even in string mode so
        // the parser can find choice-bracket boundaries (charter §5's
        // `[]` display-split) regardless of context, mirroring ink.
        if b == b'[' {
            self.pos += 1;
            self.emit(SyntaxKind::L_BRACKET, start);
            return;
        }
        if b == b']' {
            self.pos += 1;
            self.emit(SyntaxKind::R_BRACKET, start);
            return;
        }

        // Glue `<>` — breaks out of STRING_TEXT so the parser sees it even
        // inside dialogue-quoted text (charter §11: glue kept).
        if b == b'<' && self.pos + 1 < self.bytes.len() && self.bytes[self.pos + 1] == b'>' {
            self.pos += 2;
            self.emit(SyntaxKind::GLUE, start);
            return;
        }

        // Newline terminates an unterminated string — never let a
        // malformed quote swallow the rest of the file.
        if b == b'\n' || b == b'\r' {
            self.pos += 1;
            if b == b'\r' && self.pos < self.bytes.len() && self.bytes[self.pos] == b'\n' {
                self.pos += 1;
            }
            self.string_depth -= 1;
            self.emit(SyntaxKind::NEWLINE, start);
            return;
        }

        // `STRING_TEXT`: run of non-special chars (byte-stepped; codepoint
        // boundaries never split because every multi-byte UTF-8 continuation
        // byte is >= 0x80 and none of the ASCII break bytes below collide
        // with continuation bytes).
        self.pos += 1;
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b'"' | b'\\' | b'{' | b'\n' | b'\r' | b'[' | b']' => break,
                b'<' if self.pos + 1 < self.bytes.len() && self.bytes[self.pos + 1] == b'>' => {
                    break;
                }
                _ => self.pos += 1,
            }
        }
        self.emit(SyntaxKind::STRING_TEXT, start);
    }

    // ── Code-mode lexing ────────────────────────────────────────

    fn lex_code_token(&mut self) {
        let start = self.pos;
        let b = self.bytes[self.pos];

        // Newlines.
        if b == b'\n' {
            self.pos += 1;
            self.emit(SyntaxKind::NEWLINE, start);
            return;
        }
        if b == b'\r' {
            self.pos += 1;
            if self.pos < self.bytes.len() && self.bytes[self.pos] == b'\n' {
                self.pos += 1;
            }
            self.emit(SyntaxKind::NEWLINE, start);
            return;
        }

        // UTF-8 BOM (U+FEFF) — treat as whitespace trivia for lossless
        // roundtrip (adversarial-input requirement).
        if b == 0xEF
            && self.pos + 2 < self.bytes.len()
            && self.bytes[self.pos + 1] == 0xBB
            && self.bytes[self.pos + 2] == 0xBF
        {
            self.pos += 3;
            self.emit(SyntaxKind::WHITESPACE, start);
            return;
        }

        // Whitespace (spaces + tabs only).
        if b == b' ' || b == b'\t' {
            self.pos += 1;
            while self.pos < self.bytes.len()
                && (self.bytes[self.pos] == b' ' || self.bytes[self.pos] == b'\t')
            {
                self.pos += 1;
            }
            self.emit(SyntaxKind::WHITESPACE, start);
            return;
        }

        // Comments (checked before punctuation, since `/` is also SLASH).
        if b == b'/'
            && let Some(kind) = self.try_lex_comment()
        {
            self.emit(kind, start);
            return;
        }

        // Closing brace — if `string_depth > 0`, re-enter string mode.
        if b == b'}' && self.string_depth > 0 {
            self.pos += 1;
            self.string_depth -= 1;
            self.emit(SyntaxKind::R_BRACE, start);
            return;
        }

        // Multi-char punctuation (greedy, longest-first).
        if let Some((kind, advance)) = lex_punctuation(self.bytes, self.pos) {
            self.pos += advance;
            if kind == SyntaxKind::QUOTE {
                self.string_depth += 1;
            }
            self.emit(kind, start);
            return;
        }

        // Digits — could be INTEGER, FLOAT, or digit-start IDENT.
        if b.is_ascii_digit() {
            self.lex_number_or_ident();
            return;
        }

        // Identifiers (and keywords) — ASCII-only (Finding #2).
        if ident::is_ident_start_byte(b) {
            let end = ident::scan_ident(self.bytes, self.pos + 1);
            let text = &self.source[start..end];
            let kind = classify_keyword(text);
            self.pos = end;
            self.tokens.push((kind, text));
            return;
        }

        // Anything else is an error token (one codepoint at a time, never
        // splitting a multi-byte UTF-8 sequence — required for lossless
        // roundtrip on arbitrary prose/unicode input).
        self.pos += char_len_utf8(self.bytes, self.pos);
        self.emit(SyntaxKind::ERROR_TOKEN, start);
    }

    /// Try to lex a comment starting at current position (which is `/`).
    /// Returns `Some(kind)` and advances `self.pos` if successful, `None`
    /// otherwise.
    fn try_lex_comment(&mut self) -> Option<SyntaxKind> {
        if self.pos + 1 >= self.bytes.len() {
            return None;
        }
        match self.bytes[self.pos + 1] {
            b'/' => {
                // B0.6b: classify by the third/fourth byte before consuming
                // to end-of-line — `///` (exactly three slashes) is
                // DOC_COMMENT_OUTER, `//!` is DOC_COMMENT_INNER, everything
                // else (`//`, `////+`) stays a plain LINE_COMMENT (Rust
                // precedent for both rulings).
                let kind = match self.bytes.get(self.pos + 2) {
                    Some(b'/') if self.bytes.get(self.pos + 3) != Some(&b'/') => {
                        SyntaxKind::DOC_COMMENT_OUTER
                    }
                    Some(b'!') => SyntaxKind::DOC_COMMENT_INNER,
                    _ => SyntaxKind::LINE_COMMENT,
                };
                self.pos += 2;
                while self.pos < self.bytes.len()
                    && self.bytes[self.pos] != b'\n'
                    && self.bytes[self.pos] != b'\r'
                {
                    self.pos += 1;
                }
                Some(kind)
            }
            b'*' => {
                self.pos += 2;
                loop {
                    if self.pos + 1 < self.bytes.len()
                        && self.bytes[self.pos] == b'*'
                        && self.bytes[self.pos + 1] == b'/'
                    {
                        self.pos += 2;
                        break;
                    }
                    if self.pos >= self.bytes.len() {
                        break; // unterminated — runs to EOF, still lossless
                    }
                    self.pos += 1;
                }
                Some(SyntaxKind::BLOCK_COMMENT)
            }
            _ => None,
        }
    }

    /// Lex a sequence starting with a digit. Could be:
    /// - `INTEGER` (digits, NOT followed by an identifier character)
    /// - `FLOAT` (digits.digits, NOT followed by an identifier character)
    /// - digit-start `IDENT` (digits followed by an identifier character)
    fn lex_number_or_ident(&mut self) {
        let start = self.pos;

        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
            self.pos += 1;
        }

        if self.pos < self.bytes.len() && ident::is_ident_continue_byte(self.bytes[self.pos]) {
            self.pos = ident::scan_ident(self.bytes, self.pos);
            self.emit(SyntaxKind::IDENT, start);
            return;
        }

        // Float: digits.digits (NOT followed by an identifier character;
        // and NOT `..`/`.method()`-shaped — a lone `.` not followed by a
        // digit stays a separate DOT token, so `1.method()`-style postfix
        // stays parseable later without the lexer pre-deciding it).
        if self.pos < self.bytes.len()
            && self.bytes[self.pos] == b'.'
            && self.pos + 1 < self.bytes.len()
            && self.bytes[self.pos + 1].is_ascii_digit()
        {
            self.pos += 1;
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            if self.pos < self.bytes.len() && ident::is_ident_continue_byte(self.bytes[self.pos]) {
                self.pos = ident::scan_ident(self.bytes, self.pos);
                self.emit(SyntaxKind::IDENT, start);
                return;
            }
            self.emit(SyntaxKind::FLOAT, start);
            return;
        }

        self.emit(SyntaxKind::INTEGER, start);
    }
}

/// Length of the UTF-8 character starting at `pos` (1-4 bytes). Used only
/// for stepping over unrecognized bytes one codepoint at a time.
pub(crate) fn char_len_utf8(bytes: &[u8], pos: usize) -> usize {
    let b = bytes[pos];
    if b < 0x80 {
        1
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}
