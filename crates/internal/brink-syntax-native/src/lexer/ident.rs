use crate::SyntaxKind;

/// Returns `true` if the byte at `pos` starts (or continues) a native
/// identifier.
///
/// Finding #2 (`syntax_kind.rs` doc comment): ASCII-only (`[A-Za-z_]`), no
/// digits at this call site (callers handle digit-start sequences
/// separately to distinguish numbers from idents), no Unicode identifier
/// ranges. The charter's S4 casing partition (`snake_case` modules /
/// `UpperCamel` types) is an ASCII-shaped rule with no ruling extending it
/// to ink's Unicode identifier table; widening later is additive.
pub fn is_ident_start_byte(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

/// Returns `true` if the byte at `pos` can continue an identifier
/// (start-class plus digits).
pub fn is_ident_continue_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Scan forward from `pos` (already past the first identifier byte) while
/// bytes continue the identifier.
pub fn scan_ident(bytes: &[u8], mut pos: usize) -> usize {
    while pos < bytes.len() && is_ident_continue_byte(bytes[pos]) {
        pos += 1;
    }
    pos
}

/// Classify an identifier string as a keyword or plain `IDENT`.
pub fn classify_keyword(text: &str) -> SyntaxKind {
    use SyntaxKind::{
        IDENT, KW_AS, KW_CONST, KW_DONE, KW_ELSE, KW_END, KW_EXTERN, KW_FALSE, KW_FLAGS, KW_FLOW,
        KW_FN, KW_FOR, KW_IF, KW_IMPORT, KW_IN, KW_LET, KW_MATCH, KW_MODULE, KW_REF, KW_RETURN,
        KW_STRUCT, KW_TRUE, KW_UNTIL, KW_USE, KW_VAR, KW_WHILE,
    };
    match text {
        "flow" => KW_FLOW,
        "fn" => KW_FN,
        "var" => KW_VAR,
        "const" => KW_CONST,
        "let" => KW_LET,
        "flags" => KW_FLAGS,
        "struct" => KW_STRUCT,
        "extern" => KW_EXTERN,
        "import" => KW_IMPORT,
        "use" => KW_USE,
        "module" => KW_MODULE,
        "return" => KW_RETURN,
        "ref" => KW_REF,
        "if" => KW_IF,
        "match" => KW_MATCH,
        "else" => KW_ELSE,
        "while" => KW_WHILE,
        "for" => KW_FOR,
        "in" => KW_IN,
        "until" => KW_UNTIL,
        "as" => KW_AS,
        "true" => KW_TRUE,
        "false" => KW_FALSE,
        "END" => KW_END,
        "DONE" => KW_DONE,
        _ => IDENT,
    }
}
