use super::check;
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
