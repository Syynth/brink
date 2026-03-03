mod cst;

use super::{check, check_lossless};
use crate::parse;

#[test]
fn bare_expression() {
    check("Hello {x}\n");
}

#[test]
fn conditional_inline() {
    check("{x: true text}\n");
}

#[test]
fn conditional_with_branches() {
    check("{x: yes|no}\n");
}

#[test]
fn sequence_stopping() {
    check("{stopping: first|second|third}\n");
}

#[test]
fn sequence_symbol() {
    check("{&first|second|third}\n");
}

#[test]
fn implicit_sequence() {
    check("{a|b|c}\n");
}

#[test]
fn nested_inline() {
    check("Hello {x: {y}|no}\n");
}

#[test]
fn multiline_block_conditional() {
    check("{\n- x > 5:\n  Big.\n- else:\n  Small.\n}\n");
}

#[test]
fn inline_function_call() {
    check("Hello {greet(name)}\n");
}

#[test]
fn implicit_sequence_sentence() {
    check("{I bought a coffee.|I bought another.}\n");
}

#[test]
fn implicit_sequence_multi_sentence() {
    check("{First option.|Second option.|Third option.|Fourth option.}\n");
}

#[test]
fn implicit_sequence_with_diverts() {
    check("{->Fish1->|->Fish2->|nothing.}\n");
}

#[test]
fn implicit_sequence_nested_braces() {
    check("{The {&big|small} dog.|The cat.}\n");
}

#[test]
fn conditional_nested_with_outer_pipe() {
    check(
        "{ midnight : Wow! { midnight : Nice! | Bad! } | { not midnight : Very nice! | Very bad! } This is the end. }\n",
    );
}

#[test]
fn insta_conditional() {
    let p = parse("{x > 5: big|small}\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_sequence() {
    let p = parse("{stopping: first|second}\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

/// Regression test for fuzzer-discovered timeout: repeated divert + brace patterns
/// cause the parser to take exponential time in `inline_logic` / expression / content
/// mutual recursion. The parser must complete in bounded time.
#[test]
fn fuzz_deeply_nested_braces_completes() {
    // Simplified version of the fuzz artifact — repeated `->=(-> Z{` patterns
    // that cause pathological behavior via mutual recursion in the parser.
    let src = "->=(-> Z{={{{{;{;; ".repeat(150);
    let p = parse(&src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}

/// Regression test for fuzzer-discovered infinite loop in `looks_like_condition`.
/// An unclosed paren followed by an unterminated block comment caused the
/// lookahead to spin forever because EOF was only checked when depth == 0.
#[test]
fn fuzz_looks_like_condition_eof_with_depth() {
    // Exact bytes from the fuzzer artifact (as UTF-8 subset)
    let src = "/[\x01\x00\x00\x02{\n-(/*{\n-(**{";
    check_lossless(src);
}

// ── Choices inside multiline conditional branches ───────────────────

#[test]
fn multiline_cond_single_choice() {
    check("{\n- x:\n  * Go outside\n}\n");
}

#[test]
fn multiline_cond_multiple_choices() {
    check("{\n- x:\n  * Option A\n  * Option B\n}\n");
}

#[test]
fn multiline_cond_choices_both_branches() {
    check("{\n- door_open:\n  * Go outside\n- else:\n  * Ask permission\n  * Open the door\n}\n");
}

#[test]
fn multiline_cond_choice_with_divert() {
    check("{\n- x:\n  * Go outside -> garden\n}\n");
}

#[test]
fn multiline_cond_choice_with_brackets() {
    check("{\n- x:\n  * [hidden]shown\n}\n");
}

#[test]
fn multiline_cond_choice_with_label() {
    check("{\n- x:\n  * (my_label) Go outside\n}\n");
}

#[test]
fn multiline_cond_text_then_choice() {
    check("{\n- x:\n  Some text.\n  * A choice\n}\n");
}

#[test]
fn multiline_cond_sticky_choice() {
    check("{\n- x:\n  + Sticky option\n}\n");
}

#[test]
fn multiline_cond_nested_choice() {
    check("{\n- x:\n  * * Nested choice\n}\n");
}

#[test]
fn multiline_cond_choice_with_condition() {
    check("{\n- x:\n  * {flag} Conditional choice\n}\n");
}
