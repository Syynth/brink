//! Annotated-brace family — conditionals, `match`, alternations.
//! Family for #1197.

use super::*;

#[test]
fn conditional_block_braced_form() {
    let src = "flow garden() {\n  {if hp > 0 { You live. } else { You die. }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn conditional_block_colon_form() {
    let src = "flow garden() {\n  {if hp > 0: You live. else: You die.}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn match_block_parses() {
    let src = "flow garden() {\n  {match mood { calm => { Peaceful. }, wary => { Tense. } }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn alternation_inline_parses() {
    let src = "flow garden() {\n  {~ red|blue|green}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn alternation_multiline_parses() {
    let src = "flow garden() {\n  {&\n    - red\n    - blue\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}
