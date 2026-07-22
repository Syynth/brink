//! Trivia & error recovery — malformed input must never panic or hang, and
//! must still round-trip losslessly. Family for #1199.

use super::*;

#[test]
fn unclosed_flow_body_recovers() {
    let p = assert_lossless("flow greet() {\n  Hello\n");
    assert!(!p.errors().is_empty());
}

#[test]
fn stray_closing_brace_recovers() {
    let p = assert_lossless("}\n");
    assert!(!p.errors().is_empty());
}

#[test]
fn keyword_as_prose_falls_through() {
    // "flow" not followed by an IDENT is not a declaration head (Finding
    // #5) — it's ordinary prose text and must not error.
    let src = "flow through the garden and see what grows.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn decl_keyword_followed_by_ident_and_brace_is_a_decl() {
    // The flip side of the above: `flow name {` unambiguously looks like a
    // declaration head (Finding #5's third-token disambiguator) and is
    // parsed as one.
    let src = "flow gardenfulofdanger {\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn decl_keyword_followed_by_ident_alone_is_still_prose() {
    // Even `flow IDENT` alone, with nothing brace/paren-shaped after it,
    // stays prose under the strengthened three-token check — this is
    // exactly the residual ambiguity Finding #5 documents, made as safe as
    // a cheap lookahead reasonably can.
    let src = "flow gardenfulofdanger\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn garbage_tokens_never_panic() {
    let src = "flow {}}}{{{ @[ ( ) -> -> -> :: :: match if if if\n";
    let p = assert_lossless(src);
    // Not asserting on error count — only that it doesn't panic/hang and
    // stays lossless.
    let _ = p.errors();
}

#[test]
fn deeply_nested_interpolation_does_not_overflow_stack() {
    let mut src = String::new();
    for _ in 0..2000 {
        src.push('(');
    }
    src.push('1');
    for _ in 0..2000 {
        src.push(')');
    }
    let wrapped = format!("var x = {src}\n");
    let p = assert_lossless(&wrapped);
    // Must hit the depth limit and recover, not blow the stack.
    let _ = p.errors();
}
