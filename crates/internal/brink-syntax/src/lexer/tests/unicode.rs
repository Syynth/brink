use super::*;
use crate::lexer::is_ink_ident_codepoint;
use SyntaxKind::*;

// ── Full identifier lexing ───────────────────────────────────────

#[test]
fn cjk_identifier() {
    assert_eq!(tokens("日本語"), vec![(IDENT, "日本語")]);
}

#[test]
fn latin_extended() {
    assert_eq!(tokens("foo_Ā"), vec![(IDENT, "foo_Ā")]); // Ā = U+0100
}

#[test]
fn latin1_supplement_is_valid() {
    assert_eq!(tokens("café"), vec![(IDENT, "café")]);
}

#[test]
fn cyrillic_identifier() {
    assert_eq!(tokens("привет"), vec![(IDENT, "привет")]);
}

#[test]
fn arabic_identifier() {
    assert_eq!(tokens("مرحبا"), vec![(IDENT, "مرحبا")]);
}

#[test]
fn hebrew_identifier() {
    assert_eq!(tokens("שלום"), vec![(IDENT, "שלום")]);
}

#[test]
fn korean_identifier() {
    assert_eq!(tokens("안녕"), vec![(IDENT, "안녕")]);
}

#[test]
fn hiragana_identifier() {
    assert_eq!(tokens("こんにちは"), vec![(IDENT, "こんにちは")]);
}

#[test]
fn katakana_identifier() {
    assert_eq!(tokens("カタカナ"), vec![(IDENT, "カタカナ")]);
}

#[test]
fn mixed_ascii_and_unicode() {
    assert_eq!(tokens("myVar_日本語"), vec![(IDENT, "myVar_日本語")]);
}

#[test]
fn outside_ranges_is_error() {
    // U+2603 SNOWMAN — not in any ink identifier range
    let toks = tokens("☃");
    assert_eq!(toks[0].0, ERROR_TOKEN);
}

#[test]
fn emoji_is_error() {
    let toks = tokens("🎉");
    assert_eq!(toks[0].0, ERROR_TOKEN);
}

// ── Codepoint-level range checks ─────────────────────────────────

#[test]
fn greek_with_exclusions() {
    assert!(is_ink_ident_codepoint('\u{0370}')); // GREEK CAPITAL LETTER HETA
    assert!(is_ink_ident_codepoint('\u{0386}')); // GREEK CAPITAL LETTER ALPHA WITH TONOS
    assert!(is_ink_ident_codepoint('\u{03FF}')); // end of range

    // Excluded codepoints
    assert!(!is_ink_ident_codepoint('\u{0374}')); // GREEK NUMERAL SIGN
    assert!(!is_ink_ident_codepoint('\u{0375}')); // GREEK LOWER NUMERAL SIGN
    assert!(!is_ink_ident_codepoint('\u{0378}')); // start of exclude range
    assert!(!is_ink_ident_codepoint('\u{0385}')); // end of exclude range
    assert!(!is_ink_ident_codepoint('\u{0387}')); // GREEK ANO TELEIA
    assert!(!is_ink_ident_codepoint('\u{038B}')); // unassigned
    assert!(!is_ink_ident_codepoint('\u{038D}')); // unassigned
    assert!(!is_ink_ident_codepoint('\u{03A2}')); // unassigned
}

#[test]
fn cyrillic_with_exclusions() {
    assert!(is_ink_ident_codepoint('\u{0400}')); // CYRILLIC CAPITAL IE WITH GRAVE
    assert!(is_ink_ident_codepoint('\u{0481}')); // just before exclusion
    assert!(is_ink_ident_codepoint('\u{048A}')); // just after exclusion
    assert!(is_ink_ident_codepoint('\u{04FF}')); // end of range

    assert!(!is_ink_ident_codepoint('\u{0482}')); // CYRILLIC THOUSANDS SIGN
    assert!(!is_ink_ident_codepoint('\u{0489}')); // end of exclusion
}

#[test]
fn armenian_with_exclusions() {
    assert!(is_ink_ident_codepoint('\u{0531}')); // ARMENIAN CAPITAL AYB
    assert!(is_ink_ident_codepoint('\u{0556}')); // just before exclusion
    assert!(is_ink_ident_codepoint('\u{0561}')); // just after exclusion
    assert!(is_ink_ident_codepoint('\u{0587}')); // just before exclusion
    assert!(is_ink_ident_codepoint('\u{058F}')); // ARMENIAN DRAM SIGN (end)

    assert!(!is_ink_ident_codepoint('\u{0530}')); // excluded
    assert!(!is_ink_ident_codepoint('\u{0557}')); // start of exclusion
    assert!(!is_ink_ident_codepoint('\u{0560}')); // end of exclusion
    assert!(!is_ink_ident_codepoint('\u{0588}')); // start of exclusion
    assert!(!is_ink_ident_codepoint('\u{058E}')); // end of exclusion
}

#[test]
fn boundary_codepoints() {
    // Just outside each range
    assert!(!is_ink_ident_codepoint('\u{007F}')); // below Latin 1 Supplement
    assert!(!is_ink_ident_codepoint('\u{0250}')); // above Latin Extended B
    assert!(!is_ink_ident_codepoint('\u{036F}')); // below Greek
    assert!(!is_ink_ident_codepoint('\u{0500}')); // above Cyrillic
    assert!(!is_ink_ident_codepoint('\u{0700}')); // above Arabic
    assert!(!is_ink_ident_codepoint('\u{3040}')); // below Hiragana
    assert!(!is_ink_ident_codepoint('\u{3097}')); // above Hiragana
    assert!(!is_ink_ident_codepoint('\u{309F}')); // below Katakana
    assert!(!is_ink_ident_codepoint('\u{30FD}')); // above Katakana
    assert!(!is_ink_ident_codepoint('\u{4DFF}')); // below CJK
    assert!(!is_ink_ident_codepoint('\u{A000}')); // above CJK
    assert!(!is_ink_ident_codepoint('\u{ABFF}')); // below Korean
    assert!(!is_ink_ident_codepoint('\u{D7B0}')); // above Korean
}
