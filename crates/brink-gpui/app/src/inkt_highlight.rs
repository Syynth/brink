//! Syntax highlighting for `.inkt`, the compiler's textual format
//! (INVENTORY §0 item 2, the maintainer's ask 2026-09-06).
//!
//! `.inkt` gets a hand-written lexer rather than a grammar because there is
//! no grammar to have: the kit's tree-sitter list carries no
//! lisp/S-expression entry, and `.inkt` is brink's own format — its in-tree
//! reader is the `pest` grammar in `brink-format`'s `inkt/inkt.pest`, which
//! is a parser for reading, not a highlighter for painting.
//!
//! **A lexer is enough, and a parser would be worse.** Every distinction
//! worth a colour here is lexical: the head word after `(` names the form,
//! `$xx_hex` is a definition id, `"…"` a string, `:name` a type, `key=` an
//! attribute. Nothing needs the nesting to decide its colour, so the cost
//! is one pass over the text with no tree to keep in step with edits — and
//! the dump is replaced wholesale on each compile anyway.
//!
//! The token shapes are taken from `inkt.pest`'s own primitives, not from
//! reading a dump: `def_id = "$" ~ HEX{2} ~ "_" ~ HEX+`, `hex_literal =
//! "0x" ~ HEX+`, `integer = "-"? ~ DIGIT+`, `float` with a `.`, and
//! `string` with `\\`-escapes. Keeping to the grammar's definitions is what
//! stops the painting from drifting away from what the reader accepts.
//!
//! `;`-to-end-of-line is lexed as a comment. The format itself has no
//! comment production — but Compiled Output writes its "no program"
//! explanation in that shape, and painting those lines as comments is the
//! difference between a message and a mystery.

use std::ops::Range;

use gpui::{Context, SharedString, Window};
use gpui_component::input::{EditorState, InputHighlighter, InputHighlighterFactory, Rope};
use std::rc::Rc;

/// The language name Compiled Output's editor asks for.
pub const LANGUAGE: &str = "inkt";

/// Zed's syntax names, which is what the theme's highlight table is keyed
/// by (`shell/src/theme.rs`). Named here rather than inline so the mapping
/// from "what this token is" to "what it paints as" reads in one place.
mod role {
    /// The head word of a form: `story`, `global`, `address`.
    pub const HEAD: &str = "keyword";
    /// `$02_0b1cdcf0793179` — a definition id.
    pub const DEF_ID: &str = "variable";
    pub const STRING: &str = "string";
    pub const NUMBER: &str = "number";
    /// `:int`, `:string` — the declared value type.
    pub const TYPE: &str = "type";
    /// The `argc` of `argc=3`; the `checksum` of `checksum=0x…`.
    pub const ATTRIBUTE: &str = "attribute";
    /// `mutable`, `local`, `true`, `null` — bare words that are not heads.
    pub const CONSTANT: &str = "constant";
    /// `->` and the `+` of an address offset.
    pub const ARROW: &str = "punctuation.special";
    pub const COMMENT: &str = "comment";
}

/// Lex `text` into disjoint, ordered runs. Anything not returned paints as
/// plain text, which is the right default for the parens: the structure of
/// an S-expression reads from its indentation, and colouring every bracket
/// is noise rather than information.
#[must_use]
pub fn lex(text: &str) -> Vec<(Range<usize>, &'static str)> {
    let bytes = text.as_bytes();
    let mut out: Vec<(Range<usize>, &'static str)> = Vec::new();
    let mut i = 0usize;
    // Set by `(`, cleared by the word that follows it: the head of a form
    // is a head only in that position. `(name 0)` and a bare `name=` are
    // different tokens, and this one bit is the whole difference.
    let mut expect_head = false;

    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b' ' | b'\t' | b'\r' | b'\n' => {
                i += 1;
            }
            b'(' => {
                expect_head = true;
                i += 1;
            }
            b')' => {
                expect_head = false;
                i += 1;
            }
            b';' => {
                let start = i;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                out.push((start..i, role::COMMENT));
            }
            b'"' => {
                let start = i;
                i += 1;
                // `\\` escapes the next byte, so a `\"` does not end the
                // string — the grammar's own `escape` production.
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i = (i + 2).min(bytes.len()),
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                out.push((start..i, role::STRING));
                expect_head = false;
            }
            b'$' => {
                let start = i;
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                out.push((start..i, role::DEF_ID));
                expect_head = false;
            }
            b':' => {
                let start = i;
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                out.push((start..i, role::TYPE));
                expect_head = false;
            }
            b'-' if bytes.get(i + 1) == Some(&b'>') => {
                out.push((i..i + 2, role::ARROW));
                i += 2;
                expect_head = false;
            }
            b'+' => {
                // `+0` in an address entry: the sign is the arrow's partner,
                // the digits are a number.
                out.push((i..i + 1, role::ARROW));
                i += 1;
                expect_head = false;
            }
            b'0'..=b'9' => {
                let start = i;
                // `0x…` is a hex literal; everything else runs digits and
                // may carry one `.` for a float.
                if b == b'0' && bytes.get(i + 1) == Some(&b'x') {
                    i += 2;
                    while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                        i += 1;
                    }
                } else {
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    if bytes.get(i) == Some(&b'.') {
                        i += 1;
                        while i < bytes.len() && bytes[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
                out.push((start..i, role::NUMBER));
                expect_head = false;
            }
            b'-' if bytes.get(i + 1).is_some_and(u8::is_ascii_digit) => {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if bytes.get(i) == Some(&b'.') {
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                out.push((start..i, role::NUMBER));
                expect_head = false;
            }
            _ if b.is_ascii_alphanumeric() || b == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                // `argc=` — the word is naming a field, not being one.
                let role = if bytes.get(i) == Some(&b'=') {
                    role::ATTRIBUTE
                } else if expect_head {
                    role::HEAD
                } else {
                    role::CONSTANT
                };
                out.push((start..i, role));
                expect_head = false;
            }
            _ => {
                i += 1;
                expect_head = false;
            }
        }
    }
    out
}

/// The highlighter Compiled Output installs.
pub struct InktHighlighter {
    runs: Vec<(Range<usize>, &'static str)>,
}

impl InktHighlighter {
    #[must_use]
    pub fn new() -> Self {
        Self { runs: Vec::new() }
    }
}

impl Default for InktHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

/// A factory that answers for [`LANGUAGE`] and nothing else — the same
/// shape `document.rs` uses for brink, so the editor's
/// `ensure_highlighter_factory` finds this slot filled and never reaches
/// for the tree-sitter path.
#[must_use]
pub fn factory() -> InputHighlighterFactory {
    Rc::new(|language| {
        (language == LANGUAGE)
            .then(|| Box::new(InktHighlighter::new()) as Box<dyn InputHighlighter>)
    })
}

impl InputHighlighter for InktHighlighter {
    fn language(&self) -> SharedString {
        SharedString::from(LANGUAGE)
    }

    fn update(
        &mut self,
        _edit: Option<gpui_component::input::InputEdit>,
        text: &Rope,
        _folding: bool,
        _window: &mut Window,
        _cx: &mut Context<EditorState>,
    ) {
        self.runs = lex(&text.to_string());
    }

    /// The dump has no folds: it is generated text, replaced wholesale on
    /// each compile, and folding it would hide the structure it exists to
    /// show.
    fn fold_ranges(&self, _text: &Rope) -> Vec<gpui_component::input::FoldRange> {
        Vec::new()
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn gpui_component::input::HighlightStyleResolver,
    ) -> Vec<(Range<usize>, gpui::HighlightStyle)> {
        let mut out: Vec<(Range<usize>, gpui::HighlightStyle)> = Vec::new();
        let mut cursor = range.start;
        let lo = self.runs.partition_point(|(r, _)| r.end <= range.start);
        for (token, role) in &self.runs[lo..] {
            if token.start >= range.end {
                break;
            }
            let start = token.start.max(range.start);
            let end = token.end.min(range.end);
            if start > cursor {
                out.push((cursor..start, gpui::HighlightStyle::default()));
            }
            // The roles here are already Zed's names (see `role`), so they
            // go to the resolver as they are — unlike brink's own roles,
            // which ride `theme::syntax_key` to get there.
            let style = resolver.style(role).unwrap_or_default();
            out.push((start..end, style));
            cursor = end;
        }
        if cursor < range.end {
            out.push((cursor..range.end, gpui::HighlightStyle::default()));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every run, as `(text, role)` — what a reader would see.
    fn painted(text: &str) -> Vec<(&str, &'static str)> {
        lex(text)
            .into_iter()
            .map(|(r, role)| (&text[r], role))
            .collect()
    }

    #[test]
    fn a_head_word_is_a_head_only_after_an_open_paren() {
        // `name` is the head of `(name 0)`; `mutable` is a bare word in the
        // same form and must not read as one.
        assert_eq!(
            painted("(global mutable (name 0))"),
            [
                ("global", role::HEAD),
                ("mutable", role::CONSTANT),
                ("name", role::HEAD),
                ("0", role::NUMBER),
            ]
        );
    }

    #[test]
    fn the_grammars_primitives_each_get_their_own_role() {
        assert_eq!(
            painted("(global $02_0b1cd :int 0 mutable)"),
            [
                ("global", role::HEAD),
                ("$02_0b1cd", role::DEF_ID),
                (":int", role::TYPE),
                ("0", role::NUMBER),
                ("mutable", role::CONSTANT),
            ]
        );
    }

    #[test]
    fn an_address_entry_paints_its_arrow_and_offset() {
        assert_eq!(
            painted("(address $01_406ea -> $01_406ea +0)"),
            [
                ("address", role::HEAD),
                ("$01_406ea", role::DEF_ID),
                ("->", role::ARROW),
                ("$01_406ea", role::DEF_ID),
                ("+", role::ARROW),
                ("0", role::NUMBER),
            ]
        );
    }

    #[test]
    fn a_key_equals_word_is_an_attribute_not_a_head() {
        assert_eq!(
            painted("(story checksum=0x1f2e"),
            [
                ("story", role::HEAD),
                ("checksum", role::ATTRIBUTE),
                ("0x1f2e", role::NUMBER),
            ]
        );
        // Even right after `(`, where a head would otherwise be expected.
        assert_eq!(
            painted("(item name=0 ordinal=1)"),
            [
                ("item", role::HEAD),
                ("name", role::ATTRIBUTE),
                ("0", role::NUMBER),
                ("ordinal", role::ATTRIBUTE),
                ("1", role::NUMBER),
            ]
        );
    }

    #[test]
    fn a_string_survives_an_escaped_quote() {
        // The whole literal is one run: a `\"` inside must not end it, or
        // everything after would paint as if it were code.
        assert_eq!(
            painted(r#"(name_table 0 "a \" b" 1 "c")"#),
            [
                ("name_table", role::HEAD),
                ("0", role::NUMBER),
                (r#""a \" b""#, role::STRING),
                ("1", role::NUMBER),
                (r#""c""#, role::STRING),
            ]
        );
    }

    #[test]
    fn a_trailing_backslash_does_not_run_off_the_end() {
        // A truncated dump must not panic the paint pass.
        let text = r#"("abc\"#;
        let runs = lex(text);
        assert!(
            runs.iter().all(|(r, _)| r.end <= text.len()),
            "runs stay inside the text: {runs:?}"
        );
    }

    #[test]
    fn numbers_carry_their_sign_and_their_point() {
        assert_eq!(
            painted("(v -12 3.5 0x0f)"),
            [
                ("v", role::HEAD),
                ("-12", role::NUMBER),
                ("3.5", role::NUMBER),
                ("0x0f", role::NUMBER),
            ]
        );
    }

    #[test]
    fn a_semicolon_line_is_a_comment_and_ends_at_the_newline() {
        // Compiled Output's "no program" explanation, which is the only
        // thing that ever writes this shape.
        assert_eq!(
            painted("; the project has errors\n(story"),
            [
                ("; the project has errors", role::COMMENT),
                ("story", role::HEAD),
            ]
        );
    }

    #[test]
    fn the_runs_are_ordered_and_disjoint() {
        let text = r#"(story checksum=0x1f (name_table 0 "a") (global $01_ff :int -3 mutable))"#;
        let runs = lex(text);
        for pair in runs.windows(2) {
            assert!(
                pair[0].0.end <= pair[1].0.start,
                "overlapping runs: {:?} then {:?}",
                pair[0],
                pair[1]
            );
        }
        assert!(runs.iter().all(|(r, _)| r.start < r.end), "no empty runs");
    }

    #[test]
    fn nothing_is_claimed_outside_the_text() {
        let text = "(story)";
        assert!(lex(text).iter().all(|(r, _)| r.end <= text.len()));
    }
}
