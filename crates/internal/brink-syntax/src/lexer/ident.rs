use crate::SyntaxKind;

use super::char_len_utf8;

/// Returns `true` if the character at `pos` is valid in an ink identifier.
///
/// Matches the C# reference implementation (`InkParser_CharacterRanges.cs`):
/// ASCII `A-Z`, `a-z`, `0-9`, `_`, plus 12 Unicode ranges (Latin 1 Supplement,
/// Latin Extended A/B, Greek, Cyrillic, Armenian, Hebrew, Arabic, Korean,
/// CJK Unified Ideographs, Hiragana, Katakana) with per-range exclusions.
///
/// Note: the C# implementation does not distinguish identifier start from
/// identifier continue — digits and all other valid characters may appear
/// at any position.
pub fn is_ident_char(bytes: &[u8], pos: usize) -> bool {
    let b = bytes[pos];

    // ASCII fast path
    if b.is_ascii_alphabetic() || b == b'_' {
        return true;
    }

    // ASCII digits are valid identifier characters (but the caller handles
    // digit-start sequences separately to distinguish numbers from idents)
    // so we intentionally do NOT match digits here.

    // Multi-byte UTF-8: decode the codepoint and check against the C# ranges
    if b >= 0x80 {
        let ch = decode_char_at(bytes, pos);
        return is_ink_ident_codepoint(ch);
    }

    false
}

/// Decode the Unicode codepoint of the UTF-8 character starting at `pos`.
///
/// The input is always a slice of a `&str`, so it is guaranteed to be valid
/// UTF-8. We slice exactly one character's worth of bytes to avoid scanning
/// to end-of-input.
fn decode_char_at(bytes: &[u8], pos: usize) -> char {
    let len = char_len_utf8(bytes, pos);
    let slice = &bytes[pos..pos + len];
    // SAFETY: `bytes` comes from a `&str` (valid UTF-8), and we slice exactly
    // one character boundary. `from_utf8` cannot fail here.
    let s = std::str::from_utf8(slice).unwrap_or("\u{FFFD}");
    s.chars().next().unwrap_or('\u{FFFD}')
}

/// Check whether a Unicode codepoint is in the ink identifier character set.
///
/// These ranges and exclusions are transcribed exactly from the C# reference
/// implementation in `InkParser_CharacterRanges.cs`.
pub fn is_ink_ident_codepoint(ch: char) -> bool {
    let c = ch as u32;
    match c {
        // Ranges without exclusions:
        //   Latin 1 Supplement (U+0080..U+00FF)
        //   Latin Extended A   (U+0100..U+017F)
        //   Latin Extended B   (U+0180..U+024F)
        //   Hebrew             (U+0590..U+05FF)
        //   Arabic             (U+0600..U+06FF)
        //   Hiragana           (U+3041..U+3096)
        //   Katakana           (U+30A0..U+30FC)
        //   CJK Unified        (U+4E00..U+9FFF)
        //   Korean             (U+AC00..U+D7AF)
        0x0080..=0x024F
        | 0x0590..=0x06FF
        | 0x3041..=0x3096
        | 0x30A0..=0x30FC
        | 0x4E00..=0x9FFF
        | 0xAC00..=0xD7AF => true,

        // Greek: U+0370..U+03FF
        // Excludes: U+0374, U+0375, U+0378..U+0385, U+0387, U+038B, U+038D, U+03A2
        0x0370..=0x03FF => !matches!(
            c,
            0x0374 | 0x0375 | 0x0378..=0x0385 | 0x0387 | 0x038B | 0x038D | 0x03A2
        ),

        // Cyrillic: U+0400..U+04FF
        // Excludes: U+0482..U+0489
        0x0400..=0x04FF => !matches!(c, 0x0482..=0x0489),

        // Armenian: U+0530..U+058F
        // Excludes: U+0530, U+0557..U+0560, U+0588..U+058E
        0x0530..=0x058F => !matches!(c, 0x0530 | 0x0557..=0x0560 | 0x0588..=0x058E),

        _ => false,
    }
}

/// Scan forward from `pos` while bytes form identifier characters
/// (letters, digits, underscore, or valid Unicode ranges).
pub fn scan_ident(bytes: &[u8], mut pos: usize) -> usize {
    while pos < bytes.len() {
        let b = bytes[pos];
        if b.is_ascii_alphanumeric() || b == b'_' {
            pos += 1;
        } else if b >= 0x80 {
            let ch = decode_char_at(bytes, pos);
            if is_ink_ident_codepoint(ch) {
                pos += char_len_utf8(bytes, pos);
            } else {
                break;
            }
        } else {
            break;
        }
    }
    pos
}

/// Classify an identifier string as a keyword or plain `IDENT`.
pub fn classify_keyword(text: &str) -> SyntaxKind {
    use SyntaxKind::{
        IDENT, KW_AND, KW_CONST, KW_CYCLE, KW_DONE, KW_ELSE, KW_END, KW_EXTERNAL, KW_FALSE,
        KW_FUNCTION, KW_HAS, KW_HASNT, KW_INCLUDE, KW_LIST, KW_MOD, KW_NOT, KW_ONCE, KW_OR, KW_REF,
        KW_RETURN, KW_SHUFFLE, KW_STOPPING, KW_TEMP, KW_TODO, KW_TRUE, KW_VAR,
    };
    match text {
        "INCLUDE" => KW_INCLUDE,
        "EXTERNAL" => KW_EXTERNAL,
        "VAR" => KW_VAR,
        "CONST" => KW_CONST,
        "LIST" => KW_LIST,
        "temp" => KW_TEMP,
        "return" => KW_RETURN,
        "ref" => KW_REF,
        "true" => KW_TRUE,
        "false" => KW_FALSE,
        "not" => KW_NOT,
        "and" => KW_AND,
        "or" => KW_OR,
        "mod" => KW_MOD,
        "has" => KW_HAS,
        "hasnt" => KW_HASNT,
        "else" => KW_ELSE,
        "function" => KW_FUNCTION,
        "stopping" => KW_STOPPING,
        "cycle" => KW_CYCLE,
        "shuffle" => KW_SHUFFLE,
        "once" => KW_ONCE,
        "DONE" => KW_DONE,
        "END" => KW_END,
        "TODO" => KW_TODO,
        _ => IDENT,
    }
}
