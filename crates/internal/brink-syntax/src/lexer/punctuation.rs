use crate::SyntaxKind;

/// Try to lex punctuation starting at `pos`. Returns `(kind, advance)`.
/// Handles all greedy multi-char disambiguation.
#[expect(
    clippy::too_many_lines,
    reason = "flat lookup table, splitting hurts readability"
)]
pub fn lex_punctuation(bytes: &[u8], pos: usize) -> Option<(SyntaxKind, usize)> {
    use SyntaxKind::{
        AMP, AMP_AMP, BACKSLASH, BANG, BANG_EQ, BANG_QUESTION, CARET, COLON, COMMA, DIVERT, DOLLAR,
        DOT, EQ, EQ_EQ, GLUE, GT, GT_EQ, HASH, L_BRACE, L_BRACKET, L_PAREN, LT, LT_EQ, MINUS,
        MINUS_EQ, PERCENT, PIPE, PIPE_PIPE, PLUS, PLUS_EQ, QUESTION, QUOTE, R_BRACE, R_BRACKET,
        R_PAREN, SLASH, STAR, THREAD, TILDE, TUNNEL_ONWARDS,
    };

    let b = bytes[pos];
    let len = bytes.len();
    let b1 = if pos + 1 < len {
        Some(bytes[pos + 1])
    } else {
        None
    };
    let b2 = if pos + 2 < len {
        Some(bytes[pos + 2])
    } else {
        None
    };
    let b3 = if pos + 3 < len {
        Some(bytes[pos + 3])
    } else {
        None
    };

    Some(match b {
        // `->->` before `->` before `-=` before `-`
        // NOTE: `--` is NOT lexed as a single token. Two consecutive MINUS
        // tokens let the parser decide: gather dash vs postfix decrement.
        b'-' => {
            if b1 == Some(b'>') && b2 == Some(b'-') && b3 == Some(b'>') {
                (TUNNEL_ONWARDS, 4)
            } else if b1 == Some(b'>') {
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
        // `==` before `=`
        b'=' => {
            if b1 == Some(b'=') {
                (EQ_EQ, 2)
            } else {
                (EQ, 1)
            }
        }
        // `!=` before `!?` before `!`
        b'!' => {
            if b1 == Some(b'=') {
                (BANG_EQ, 2)
            } else if b1 == Some(b'?') {
                (BANG_QUESTION, 2)
            } else {
                (BANG, 1)
            }
        }
        // `||` before `|`
        b'|' => {
            if b1 == Some(b'|') {
                (PIPE_PIPE, 2)
            } else {
                (PIPE, 1)
            }
        }
        // `&&` before `&`
        b'&' => {
            if b1 == Some(b'&') {
                (AMP_AMP, 2)
            } else {
                (AMP, 1)
            }
        }
        // `+=` before `+` — `++` is NOT a compound token; the parser handles
        // adjacent PLUS PLUS as postfix increment vs choice bullets from context.
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

        // Single-char punctuation
        b'/' => (SLASH, 1),
        b'*' => (STAR, 1),
        b'%' => (PERCENT, 1),
        b'^' => (CARET, 1),
        b'?' => (QUESTION, 1),
        b'$' => (DOLLAR, 1),
        b'(' => (L_PAREN, 1),
        b')' => (R_PAREN, 1),
        b'{' => (L_BRACE, 1),
        b'}' => (R_BRACE, 1),
        b'[' => (L_BRACKET, 1),
        b']' => (R_BRACKET, 1),
        b',' => (COMMA, 1),
        b'.' => (DOT, 1),
        b':' => (COLON, 1),
        b'#' => (HASH, 1),
        b'~' => (TILDE, 1),
        b'\\' => (BACKSLASH, 1),
        b'"' => (QUOTE, 1),

        _ => return None,
    })
}
