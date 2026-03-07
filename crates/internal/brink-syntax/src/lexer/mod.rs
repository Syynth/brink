mod ident;
mod punctuation;
#[cfg(test)]
mod tests;

use crate::SyntaxKind;
pub use ident::{classify_keyword, is_ident_char, is_ink_ident_codepoint, scan_ident};
pub use punctuation::lex_punctuation;

/// Lex `source` into a sequence of `(SyntaxKind, slice)` pairs.
///
/// Every byte of `source` appears in exactly one token (lossless).
/// The only mutable state is a `string_depth: u32` tracking nested string
/// interpolation. When `string_depth > 0`, we are inside a string and lex
/// `STRING_TEXT`/`STRING_ESCAPE`/`QUOTE`/`L_BRACE`. `{` inside a string
/// increments depth and exits string mode; `}` outside a string with
/// `depth > 0` decrements depth and re-enters string mode.
pub fn lex(source: &str) -> Vec<(SyntaxKind, &str)> {
    Lexer::new(source).run()
}

struct Lexer<'src> {
    source: &'src str,
    bytes: &'src [u8],
    pos: usize,
    /// Nesting depth of string interpolations.
    /// 0 = outside any string. 1 = inside a string. 2 = inside an interpolation
    /// inside a string, etc. Odd = in string mode, even = in code mode (but
    /// with pending string closings to track).
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

        // Closing quote — pop one level of string nesting
        if b == b'"' {
            self.pos += 1;
            self.string_depth -= 1;
            self.emit(SyntaxKind::QUOTE, start);
            return;
        }

        // Escape sequence
        if b == b'\\' && self.pos + 1 < self.bytes.len() {
            let next = self.bytes[self.pos + 1];
            if matches!(next, b'n' | b't' | b'\\' | b'"') {
                self.pos += 2;
                self.emit(SyntaxKind::STRING_ESCAPE, start);
                return;
            }
        }

        // Opening brace — enter interpolation (push depth)
        if b == b'{' {
            self.pos += 1;
            self.string_depth += 1;
            self.emit(SyntaxKind::L_BRACE, start);
            return;
        }

        // Brackets — emit as L_BRACKET/R_BRACKET even in string mode so the
        // parser can find choice bracket boundaries regardless of context.
        // In ink, `[` and `]` in choice content are always bracket delimiters
        // even when they appear inside quoted text like `"tired[."]`.
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

        // Newline terminates an unterminated string
        if b == b'\n' || b == b'\r' {
            self.pos += 1;
            if b == b'\r' && self.pos < self.bytes.len() && self.bytes[self.pos] == b'\n' {
                self.pos += 1;
            }
            self.string_depth -= 1;
            self.emit(SyntaxKind::NEWLINE, start);
            return;
        }

        // `STRING_TEXT`: run of non-special chars
        self.pos += 1;
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b'"' | b'\\' | b'{' | b'\n' | b'\r' | b'[' | b']' => break,
                _ => self.pos += 1,
            }
        }
        self.emit(SyntaxKind::STRING_TEXT, start);
    }

    // ── Code-mode lexing ────────────────────────────────────────

    fn lex_code_token(&mut self) {
        let start = self.pos;
        let b = self.bytes[self.pos];

        // Newlines
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

        // UTF-8 BOM (U+FEFF) — treat as whitespace trivia for lossless roundtrip.
        if b == 0xEF
            && self.pos + 2 < self.bytes.len()
            && self.bytes[self.pos + 1] == 0xBB
            && self.bytes[self.pos + 2] == 0xBF
        {
            self.pos += 3;
            self.emit(SyntaxKind::WHITESPACE, start);
            return;
        }

        // Whitespace (spaces + tabs only)
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

        // Comments (before punctuation, since `/` is also SLASH)
        if b == b'/'
            && let Some(kind) = self.try_lex_comment()
        {
            self.emit(kind, start);
            return;
        }

        // Closing brace — if `string_depth > 0`, re-enter string mode
        if b == b'}' && self.string_depth > 0 {
            self.pos += 1;
            self.string_depth -= 1;
            self.emit(SyntaxKind::R_BRACE, start);
            return;
        }

        // Multi-char punctuation (greedy, longest-first)
        if let Some((kind, advance)) = lex_punctuation(self.bytes, self.pos) {
            self.pos += advance;
            if kind == SyntaxKind::QUOTE {
                self.string_depth += 1;
            }
            self.emit(kind, start);
            return;
        }

        // Digits — could be INTEGER, FLOAT, or digit-start IDENT
        if b.is_ascii_digit() {
            self.lex_number_or_ident();
            return;
        }

        // Identifiers (and keywords)
        if is_ident_char(self.bytes, self.pos) {
            let end = scan_ident(self.bytes, self.pos + char_len_utf8(self.bytes, self.pos));
            let text = &self.source[start..end];
            let kind = classify_keyword(text);
            self.pos = end;
            self.tokens.push((kind, text));
            return;
        }

        // Anything else is an error token (one char at a time)
        self.pos += char_len_utf8(self.bytes, self.pos);
        self.emit(SyntaxKind::ERROR_TOKEN, start);
    }

    /// Try to lex a comment starting at current position (which is `/`).
    /// Returns `Some(kind)` and advances `self.pos` if successful, `None` otherwise.
    fn try_lex_comment(&mut self) -> Option<SyntaxKind> {
        if self.pos + 1 >= self.bytes.len() {
            return None;
        }
        match self.bytes[self.pos + 1] {
            b'/' => {
                // Line comment — consume through end of line (not including newline)
                self.pos += 2;
                while self.pos < self.bytes.len()
                    && self.bytes[self.pos] != b'\n'
                    && self.bytes[self.pos] != b'\r'
                {
                    self.pos += 1;
                }
                Some(SyntaxKind::LINE_COMMENT)
            }
            b'*' => {
                // Block comment — consume through `*/`
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
                        break; // unterminated
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

        // Consume leading digits
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
            self.pos += 1;
        }

        // Check for digit-start identifier: digits followed by an identifier character
        if self.pos < self.bytes.len() && is_ident_char(self.bytes, self.pos) {
            self.pos = scan_ident(self.bytes, self.pos + char_len_utf8(self.bytes, self.pos));
            self.emit(SyntaxKind::IDENT, start);
            return;
        }

        // Check for float: digits.digits (NOT followed by an identifier character)
        if self.pos < self.bytes.len()
            && self.bytes[self.pos] == b'.'
            && self.pos + 1 < self.bytes.len()
            && self.bytes[self.pos + 1].is_ascii_digit()
        {
            self.pos += 1; // skip the dot
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            // If followed by an identifier character, the whole thing is an ident
            if self.pos < self.bytes.len() && is_ident_char(self.bytes, self.pos) {
                self.pos = scan_ident(self.bytes, self.pos + char_len_utf8(self.bytes, self.pos));
                self.emit(SyntaxKind::IDENT, start);
                return;
            }
            self.emit(SyntaxKind::FLOAT, start);
            return;
        }

        // Plain integer
        self.emit(SyntaxKind::INTEGER, start);
    }
}

/// Length of the UTF-8 character starting at `pos` (1–4 bytes).
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
