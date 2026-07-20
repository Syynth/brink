use crate::SyntaxKind;

/// Try to lex punctuation starting at `pos`. Returns `(kind, advance)`.
/// Handles all greedy multi-char disambiguation. `None` means the byte
/// isn't punctuation at all (falls through to ident/digit/error handling).
#[expect(
    clippy::too_many_lines,
    reason = "flat lookup table, splitting hurts readability"
)]
pub fn lex_punctuation(bytes: &[u8], pos: usize) -> Option<(SyntaxKind, usize)> {
    use SyntaxKind::{
        AMP, AMP_AMP, AT, AT_L_BRACKET, BACKSLASH, BANG, BANG_EQ, CARET, COLON, COLON_COLON, COMMA,
        DIVERT, DOT, EQ, EQ_EQ, FAT_ARROW, GLUE, GT, GT_EQ, HASH, L_BRACE, L_BRACKET, L_PAREN, LT,
        LT_EQ, MINUS, MINUS_EQ, PERCENT, PIPE, PLUS, PLUS_EQ, QUESTION, QUOTE, R_BRACE, R_BRACKET,
        R_PAREN, SEMICOLON, SLASH, SLASH_EQ, STAR, STAR_EQ, THREAD, TILDE,
    };

    let b = bytes[pos];
    let len = bytes.len();
    let b1 = if pos + 1 < len {
        Some(bytes[pos + 1])
    } else {
        None
    };

    Some(match b {
        // `@[` — the annotation-line opener. Only the adjacent pair lexes;
        // a lone `@` is `AT` (round-trips losslessly; not otherwise
        // meaningful punctuation in this grammar).
        b'@' => {
            if b1 == Some(b'[') {
                (AT_L_BRACKET, 2)
            } else {
                (AT, 1)
            }
        }
        // `->` before `-=` before `-`
        b'-' => {
            if b1 == Some(b'>') {
                (DIVERT, 2)
            } else if b1 == Some(b'=') {
                (MINUS_EQ, 2)
            } else {
                (MINUS, 1)
            }
        }
        // `<>` before `<-` before `<=` before `<`
        b'<' => {
            if b1 == Some(b'>') {
                (GLUE, 2)
            } else if b1 == Some(b'-') {
                (THREAD, 2)
            } else if b1 == Some(b'=') {
                (LT_EQ, 2)
            } else {
                (LT, 1)
            }
        }
        // `==` before `=>` before `=`
        b'=' => {
            if b1 == Some(b'=') {
                (EQ_EQ, 2)
            } else if b1 == Some(b'>') {
                (FAT_ARROW, 2)
            } else {
                (EQ, 1)
            }
        }
        // `!=` before `!`
        b'!' => {
            if b1 == Some(b'=') {
                (BANG_EQ, 2)
            } else {
                (BANG, 1)
            }
        }
        // `::` before `:`
        b':' => {
            if b1 == Some(b':') {
                (COLON_COLON, 2)
            } else {
                (COLON, 1)
            }
        }
        // `|` — two adjacent `PIPE`s are NOT compounded into a logical-or
        // token (mirrors brink-syntax's `||`/`++`/`--` precedent): the
        // parser disambiguates `a || b` from `|x|` lambda-param delimiters
        // by position, not lexical shape.
        b'|' => (PIPE, 1),
        // `&&` before `&`
        b'&' => {
            if b1 == Some(b'&') {
                (AMP_AMP, 2)
            } else {
                (AMP, 1)
            }
        }
        // `+=` before `+`
        b'+' => {
            if b1 == Some(b'=') {
                (PLUS_EQ, 2)
            } else {
                (PLUS, 1)
            }
        }
        // `>=` before `>`
        b'>' => {
            if b1 == Some(b'=') {
                (GT_EQ, 2)
            } else {
                (GT, 1)
            }
        }
        // `*=` before `*`
        b'*' => {
            if b1 == Some(b'=') {
                (STAR_EQ, 2)
            } else {
                (STAR, 1)
            }
        }
        // `/=` before `/` (line/block comments are intercepted earlier by
        // the lexer driver, before punctuation lookup runs).
        b'/' => {
            if b1 == Some(b'=') {
                (SLASH_EQ, 2)
            } else {
                (SLASH, 1)
            }
        }

        // Single-char punctuation
        b'%' => (PERCENT, 1),
        b'^' => (CARET, 1),
        b'?' => (QUESTION, 1),
        b'(' => (L_PAREN, 1),
        b')' => (R_PAREN, 1),
        b'{' => (L_BRACE, 1),
        b'}' => (R_BRACE, 1),
        b'[' => (L_BRACKET, 1),
        b']' => (R_BRACKET, 1),
        b',' => (COMMA, 1),
        b'.' => (DOT, 1),
        b'#' => (HASH, 1),
        b';' => (SEMICOLON, 1),
        b'~' => (TILDE, 1),
        b'\\' => (BACKSLASH, 1),
        b'"' => (QUOTE, 1),

        _ => return None,
    })
}
