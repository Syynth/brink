use super::*;

#[test]
fn simple_roundtrip() {
    let src = "Hello, world! -> knot\n~ x = 5\n";
    let toks = lex(src);
    let reconstructed: String = toks.iter().map(|(_, t)| *t).collect();
    assert_eq!(src, reconstructed);
}

#[test]
fn complex_roundtrip() {
    let src = "=== function greet(ref name) ===\nHello, {name}!\n~ temp x = 3.14 + 1\n* [Choice] -> knot\n- (gather) Done.\n";
    let toks = lex(src);
    let reconstructed: String = toks.iter().map(|(_, t)| *t).collect();
    assert_eq!(src, reconstructed);
}

#[test]
fn roundtrip_with_strings() {
    let src = r#"~ x = "hello {world} \n""#;
    let toks = lex(src);
    let reconstructed: String = toks.iter().map(|(_, t)| *t).collect();
    assert_eq!(src, reconstructed);
}

#[test]
fn roundtrip_with_unicode() {
    let src = "VAR café = 42\nVAR 日本語 = true\n";
    let toks = lex(src);
    let reconstructed: String = toks.iter().map(|(_, t)| *t).collect();
    assert_eq!(src, reconstructed);
}

#[test]
fn roundtrip_with_bom() {
    let src = "\u{FEFF}hello\n";
    let toks = lex(src);
    let reconstructed: String = toks.iter().map(|(_, t)| *t).collect();
    assert_eq!(src, reconstructed);
}

#[test]
fn roundtrip_with_error_tokens() {
    let src = "hello ` world ☃ end\n";
    let toks = lex(src);
    let reconstructed: String = toks.iter().map(|(_, t)| *t).collect();
    assert_eq!(src, reconstructed);
}
