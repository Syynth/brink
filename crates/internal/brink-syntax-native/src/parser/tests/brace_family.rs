//! Annotated-brace family — conditionals, `match`, alternations.
//! Family for #1197.
//!
//! Parity target: `brink-syntax/src/parser/tests/inline/mod.rs` (the ink
//! twin's inline/multiline conditional+sequence anatomy) — studied for
//! structure and depth, not copied verbatim: this grammar's own shape
//! (`{if cond}`/`{match expr}` with colon-body OR braced-arm forms;
//! `{~}`/`{&}`/`{!}`/`{|}` single-char markers with inline-pipe OR
//! multiline-dash-`ENTRY` forms) is a deliberate divergence documented on
//! `CONDITIONAL_BLOCK` in `syntax_kind.rs` (Finding #4, flagged for
//! Track-B confirmation).
//!
//! Two genuine parser gaps surfaced while writing this coverage (same-line
//! colon-form `else:` swallowed silently; no flat `else if` chain sugar)
//! were tracked in #1254, ruled in #1258, and fixed in #1261 — see
//! `colon_form_else_on_the_same_line_is_recognized_as_an_else_arm` and
//! `else_if_flat_chain_is_recognized_as_a_chain` below, which now assert
//! the fixed behavior instead of pinning the old bugs.

use super::*;

// ── Section A: CONDITIONAL_BLOCK — braced-arm form ───────────────────

#[test]
fn conditional_block_braced_form() {
    let src = "flow garden() {\n  {if hp > 0 { You live. } else { You die. }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn conditional_braced_if_only_no_else() {
    let src = "flow garden() {\n  {if hp > 0 { You live. }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cond: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("CONDITIONAL_BLOCK");
    assert!(cond.is_if());
    assert!(!cond.is_match());
    assert!(cond.condition().is_some(), "condition expr present");
    let if_arm = cond.if_arm().expect("IF_ARM");
    assert!(if_arm.block().is_some(), "braced IF_ARM has a BLOCK child");
    assert!(cond.else_arm().is_none(), "no else present");
}

#[test]
fn conditional_braced_if_and_else_accessors() {
    let src = "flow garden() {\n  {if hp > 0 { You live. } else { You die. }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cond: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("CONDITIONAL_BLOCK");
    assert!(cond.if_arm().expect("IF_ARM").block().is_some());
    assert!(cond.else_arm().expect("ELSE_BRANCH").block().is_some());
}

#[test]
fn conditional_braced_condition_is_an_infix_expression() {
    let src = "flow garden() {\n  {if hp > 0 { You live. }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INFIX_EXPR));
}

// ── Section B: CONDITIONAL_BLOCK — colon-body form ────────────────────

#[test]
fn conditional_block_colon_form() {
    let src = "flow garden() {\n  {if hp > 0: You live. else: You die.}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn colon_form_else_on_the_same_line_is_recognized_as_an_else_arm() {
    // FIXED (#1254 Gap 1, #1261): `colon_body` (family.rs) now dispatches
    // its per-physical-line items through `colon_body_line`, whose prose
    // fallback (`content::content_line_else_boundary`) recognizes a
    // same-line `else:` as a boundary and stops the content scan there,
    // instead of swallowing it into the if-arm's `TEXT` the way plain
    // `content_line`'s unbounded scan would. The "inline colon body" form
    // the `CONDITIONAL_BLOCK` doc comment calls out BY NAME as "the
    // inline colon body (`{if cond: … else: …}`, charter §6's literal
    // example)" — a genuinely single physical line — now produces a real
    // `ELSE_BRANCH`, matching the multi-line colon form
    // (`conditional_colon_if_and_else_items_accessors` below).
    let src = "flow garden() {\n  {if hp > 0: You live. else: You die.}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cond: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("CONDITIONAL_BLOCK");
    let else_arm = cond.else_arm().expect("ELSE_BRANCH is now produced");
    assert!(else_arm.block().is_none(), "colon form has no BLOCK child");
    assert!(
        else_arm.items().next().is_some(),
        "colon-form else arm has direct-child body items"
    );
    // The if-arm's own TEXT must NOT contain the leaked else body anymore
    // (trailing space before the boundary stays part of the TEXT run,
    // same convention as every other trailing structural break — see
    // `text_run_until`'s doc comment).
    let if_arm = cond.if_arm().expect("IF_ARM");
    assert_eq!(text_run_concat(if_arm.syntax()), "You live. ");
    assert_eq!(text_run_concat(else_arm.syntax()), "You die.");
}

#[test]
fn splice_inside_a_colon_body_still_warns() {
    // Regression guard (#1261): the colon-body per-line dispatcher
    // (`colon_body_line`) must keep every non-prose line shape `body_line`
    // recognizes — including a bare `<-` (THREAD) outside a choice point,
    // which ruling #1263 requires warn (not silently swallow into TEXT).
    // An earlier revision of `colon_body_line` omitted the THREAD arm, so a
    // `<-` inside a colon-form conditional body regressed to a silent prose
    // swallow with no diagnostic — caught in review, pinned here.
    let src = "flow f() {\n  {if ready:\n    <- side_thread\n  }\n}\n";
    let p = assert_lossless(src);
    assert_eq!(
        p.errors().len(),
        1,
        "`<-` in a colon body must still raise the #1263 warning; errors: {:?}",
        p.errors()
    );
    assert_eq!(
        p.errors()[0].severity,
        ParseSeverity::Warning,
        "a splice outside a choice point warns, never hard-errors"
    );
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::SPLICE));
}

#[test]
fn conditional_colon_if_only_no_else() {
    let src = "flow garden() {\n  {if hp > 0: You live.}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cond: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("CONDITIONAL_BLOCK");
    let if_arm = cond.if_arm().expect("IF_ARM");
    assert!(if_arm.block().is_none(), "colon form has no BLOCK child");
    assert!(
        if_arm.items().next().is_some(),
        "colon form has direct-child body items"
    );
    assert!(cond.else_arm().is_none());
}

#[test]
fn conditional_colon_if_and_else_items_accessors() {
    // `else:` must start its own line to be recognized as a boundary —
    // see `colon_form_else_on_the_same_line_is_not_recognized_as_an_else_arm`
    // above for the same-line gap this sidesteps.
    let src = "flow garden() {\n  {if hp > 0:\n  You live.\n  else:\n  You die.\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cond: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("CONDITIONAL_BLOCK");
    let if_arm = cond.if_arm().expect("IF_ARM");
    assert_eq!(
        if_arm
            .items()
            .filter(|n| n.kind() == SyntaxKind::CONTENT_LINE)
            .count(),
        1,
        "one CONTENT_LINE body item"
    );
    let else_arm = cond.else_arm().expect("ELSE_BRANCH");
    assert_eq!(
        else_arm
            .items()
            .filter(|n| n.kind() == SyntaxKind::CONTENT_LINE)
            .count(),
        1
    );
}

#[test]
fn conditional_colon_body_runs_multiple_content_lines_until_else_or_close() {
    // The colon body isn't limited to one line — `colon_body` loops on
    // `body_line` until `}`/EOF/an `else` boundary (`family.rs`).
    let src = "flow garden() {\n  {if hp > 0:\n    You live.\n    Barely.\n  else:\n    You die.\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cond: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("CONDITIONAL_BLOCK");
    let if_arm = cond.if_arm().expect("IF_ARM");
    assert_eq!(
        if_arm
            .items()
            .filter(|n| n.kind() == SyntaxKind::CONTENT_LINE)
            .count(),
        2,
        "two content lines before the `else:` boundary stop colon_body"
    );
}

// ── Section C: mixed forms + else-chaining via nesting ────────────────

#[test]
fn conditional_arms_may_independently_choose_colon_or_brace_form() {
    // `arm_body` decides colon-vs-brace per call (`if_arm`/`else_branch`
    // each call it independently) — an `if` arm may use the colon form
    // while its `else` uses the braced form, or vice versa. `else` must
    // start its own line for the colon-form if-arm to hand off to it —
    // see the same-line gap documented above.
    let src = "flow garden() {\n  {if hp > 0:\n  You live.\n  else { You die. }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cond: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("CONDITIONAL_BLOCK");
    assert!(cond.if_arm().expect("IF_ARM").block().is_none());
    assert!(cond.else_arm().expect("ELSE_BRANCH").block().is_some());
}

#[test]
fn conditional_arms_mixed_form_other_direction() {
    // Mirror of `conditional_arms_may_independently_choose_colon_or_brace_form`
    // above, with the forms swapped: braced `if` arm, colon-form `else`.
    // Pins the actual shape, not just "no error" — this would still pass
    // if the braced-if/colon-else combination stopped producing an
    // ELSE_BRANCH.
    let src = "flow garden() {\n  {if hp > 0 { You live. } else: You die.}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cond: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("CONDITIONAL_BLOCK");
    assert!(cond.if_arm().expect("IF_ARM").block().is_some());
    let else_arm = cond.else_arm().expect("ELSE_BRANCH");
    assert!(else_arm.block().is_none());
    assert!(else_arm.items().next().is_some());
}

#[test]
fn else_if_chain_via_braced_nesting() {
    // There is no flat `else if` sugar in this grammar (see
    // `else_if_flat_chain_is_not_recognized_as_a_chain` below) — a chain
    // is spelled as an ordinary nested `CONDITIONAL_BLOCK` sitting inside
    // the outer `else` arm's body, exactly like any other body item.
    let src = "flow garden() {\n  {if hp > 10 { Great. } else { {if hp > 0 { OK. } else { Dead. }} }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(
        count_node_kind(&p.syntax(), SyntaxKind::CONDITIONAL_BLOCK),
        2
    );
}

#[test]
fn else_if_chain_via_colon_nesting() {
    // The colon form's `else:` doesn't need the double-brace dance —
    // `colon_body` is a plain body-line loop, so a nested `{if …: …}`
    // reads like a chained `else if` even though it's really "an else
    // arm whose one body item happens to be another CONDITIONAL_BLOCK".
    // As above, each `else:` must start its own line for the boundary to
    // be recognized at all (the same-line gap documented above applies
    // at both nesting levels here).
    let src =
        "flow garden() {\n  {if a:\n  A\n  else:\n  {if b:\n    B\n    else:\n    C\n  }\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(
        count_node_kind(&p.syntax(), SyntaxKind::CONDITIONAL_BLOCK),
        2
    );
    let outer: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("outer CONDITIONAL_BLOCK");
    let outer_else = outer.else_arm().expect("outer ELSE_BRANCH");
    let inner = outer_else
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("nested CONDITIONAL_BLOCK inside outer else");
    assert!(inner.is_if());
}

#[test]
fn else_if_flat_chain_is_recognized_as_a_chain() {
    // RULED 2026-07-22 (#1258, implemented #1261): flat `else if <cond>
    // { … }` chains exactly like the explicit-nesting spelling —
    // `at_else_arm` (family.rs) learns `else` immediately followed by
    // `KW_IF` as a third arm-opener shape (alongside `{`/`:`), and
    // `else_branch` parses that shape as a brace-less `CONDITIONAL_BLOCK`
    // sharing `conditional_body` with the ordinary `{if …}` entry point —
    // same node kinds, same `is_if`/`condition`/`if_arm`/`else_arm`
    // accessors as `else_if_chain_via_braced_nesting` below, just without
    // the extra delimiter tokens the flat spelling never had in source.
    let src = "flow garden() {\n  {if a { A } else if b { B } else { C }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    // Two CONDITIONAL_BLOCKs: the outer `if a …` and the chained `if b …`
    // — identical count to `else_if_chain_via_braced_nesting`'s explicit
    // nesting.
    assert_eq!(
        count_node_kind(&p.syntax(), SyntaxKind::CONDITIONAL_BLOCK),
        2
    );
    let outer: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("outer CONDITIONAL_BLOCK");
    let outer_else = outer.else_arm().expect("outer ELSE_BRANCH");
    let inner = outer_else
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("chained CONDITIONAL_BLOCK inside the outer else arm");
    assert!(inner.is_if());
    assert!(inner.else_arm().is_some(), "chained if's own else branch");
}

#[test]
fn else_if_flat_chain_colon_form_is_recognized_as_a_chain() {
    // Colon-form companion to `else_if_flat_chain_is_recognized_as_a_chain`
    // above: `else if <cond>: …` chains the same way — `at_else_arm`'s
    // `KW_IF` shape doesn't care which body form the chained `if` itself
    // uses.
    let src = "flow garden() {\n  {if a:\n  A\n  else if b:\n  B\n  else:\n  C\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(
        count_node_kind(&p.syntax(), SyntaxKind::CONDITIONAL_BLOCK),
        2
    );
}

// ── Section D: MATCH_ARM / MATCH_PATTERN shapes ────────────────────────

#[test]
fn match_block_parses() {
    let src = "flow garden() {\n  {match mood { calm => { Peaceful. }, wary => { Tense. } }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn match_is_match_not_is_if() {
    let src = "flow garden() {\n  {match mood { calm => { Peaceful. } }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cond: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("CONDITIONAL_BLOCK");
    assert!(cond.is_match());
    assert!(!cond.is_if());
    assert!(cond.condition().is_some(), "match subject expr present");
}

#[test]
fn match_single_braced_arm() {
    let src = "flow garden() {\n  {match mood { calm => { Peaceful. } }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cond: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("CONDITIONAL_BLOCK");
    let arms: Vec<_> = cond.match_arms().collect();
    assert_eq!(arms.len(), 1);
    assert!(arms[0].pattern_expr().is_some());
    assert!(arms[0].block().is_some());
    assert!(arms[0].bare_expr().is_none());
}

#[test]
fn match_single_bare_expr_arm() {
    // `pattern => expr` — no braces, the arm body is a bare expression
    // (`match_arm`'s `else` branch: `super::expr::expression(p)`).
    let src = "flow garden() {\n  {match mood { calm => tranquil }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cond: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("CONDITIONAL_BLOCK");
    let arms: Vec<_> = cond.match_arms().collect();
    assert_eq!(arms.len(), 1);
    assert!(arms[0].block().is_none());
    assert!(arms[0].bare_expr().is_some());
}

#[test]
fn match_multiple_arms_with_trailing_commas() {
    let src = "flow garden() {\n  {match mood { calm => a, wary => b, hostile => c, }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cond: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("CONDITIONAL_BLOCK");
    assert_eq!(cond.match_arms().count(), 3);
}

#[test]
fn match_arms_newline_separated_without_commas() {
    // `match_arm_list`'s loop only eats a comma when present (`p.eat`,
    // not `p.expect`) — arms separated by bare newlines parse cleanly.
    let src = "flow garden() {\n  {match m {\n    calm => Peaceful\n    wary => Tense\n  }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cond: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("CONDITIONAL_BLOCK");
    assert_eq!(cond.match_arms().count(), 2);
}

#[test]
fn match_pattern_expr_is_reused_expression_grammar() {
    // MATCH_PATTERN is intentionally shallow (syntax_kind.rs doc comment)
    // — any expression is accepted as a "pattern", including an infix one.
    let src = "flow garden() {\n  {match mood { hp > 0 => alive }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cond: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("CONDITIONAL_BLOCK");
    let arm = cond.match_arms().next().expect("one arm");
    let pattern = arm.pattern_expr().expect("pattern expr");
    assert_eq!(pattern.kind(), SyntaxKind::INFIX_EXPR);
}

// ── Section E: ALTERNATION_MARKER — inline pipe-separated form ────────

#[test]
fn alternation_inline_parses() {
    let src = "flow garden() {\n  {~ red|blue|green}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn alternation_inline_all_four_marker_kinds() {
    for (marker, kind) in [
        ("~", SyntaxKind::TILDE),
        ("&", SyntaxKind::AMP),
        ("!", SyntaxKind::BANG),
        ("|", SyntaxKind::PIPE),
    ] {
        let src = format!("flow f() {{\n  {{{marker} a|b|c}}\n}}\n");
        let p = assert_lossless(&src);
        assert!(p.errors().is_empty(), "{marker:?} errors: {:?}", p.errors());
        let alt: ast::AlternationBlock = p
            .syntax()
            .descendants()
            .find_map(ast::AlternationBlock::cast)
            .expect("ALTERNATION_BLOCK for marker");
        let tok = alt.marker_token().expect("marker token");
        assert_eq!(tok.kind(), kind, "marker {marker:?}");
    }
}

#[test]
fn alternation_inline_two_alternatives_minimal() {
    let src = "flow f() {\n  {~a|b}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn alternation_inline_stopping_marker_is_pipe_itself() {
    // The marker char may itself be `|` (the stopping-sequence spelling)
    // — only the FIRST `|` right after the marker is consumed as the
    // marker; every later bare `|` is an ordinary alternative separator
    // (family.rs's `inline_alternatives` doc comment).
    let src = "flow f() {\n  {| a|b|c}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let alt: ast::AlternationBlock = p
        .syntax()
        .descendants()
        .find_map(ast::AlternationBlock::cast)
        .expect("ALTERNATION_BLOCK");
    assert_eq!(alt.marker_token().expect("marker").kind(), SyntaxKind::PIPE);
    // Two remaining bare PIPE separators, not folded into the marker.
    let pipe_tokens = alt
        .syntax()
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .filter(|t| t.kind() == SyntaxKind::PIPE)
        .count();
    assert_eq!(pipe_tokens, 2, "separators only, marker excluded");
}

#[test]
fn alternation_inline_empty_alternatives_between_pipes() {
    let src = "flow f() {\n  {~a||c}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn alternation_inline_trailing_tag() {
    // `inline_alternatives` special-cases `HASH` explicitly so a trailing
    // tag doesn't spin the loop (family.rs comment on the `HASH` arm) —
    // the tag must land INSIDE the ALTERNATION_BLOCK for this to actually
    // exercise that arm (a tag after the closing `}` is a sibling
    // TAG_LINE and never reaches `inline_alternatives` at all).
    let src = "flow f() {\n  {~a|b #tag}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let alt: ast::AlternationBlock = p
        .syntax()
        .descendants()
        .find_map(ast::AlternationBlock::cast)
        .expect("ALTERNATION_BLOCK");
    assert!(
        alt.syntax()
            .descendants()
            .any(|n| n.kind() == SyntaxKind::TAG),
        "trailing #tag must be a child of the ALTERNATION_BLOCK"
    );
}

#[test]
fn alternation_inline_trailing_tag_outside_the_brace_is_not_inside_the_block() {
    // Companion to the above: a tag AFTER the closing `}` is ordinary
    // trailing-tag syntax on the enclosing line, not something
    // `inline_alternatives`' HASH special case ever sees.
    let src = "flow f() {\n  {~a|b} #tag\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let alt: ast::AlternationBlock = p
        .syntax()
        .descendants()
        .find_map(ast::AlternationBlock::cast)
        .expect("ALTERNATION_BLOCK");
    assert!(
        !alt.syntax()
            .descendants()
            .any(|n| n.kind() == SyntaxKind::TAG),
        "the outside-the-brace tag must NOT be a child of the ALTERNATION_BLOCK"
    );
}

// ── Section F: ALTERNATION_MARKER — multiline dash-ENTRY form ─────────

#[test]
fn alternation_multiline_parses() {
    let src = "flow garden() {\n  {&\n    - red\n    - blue\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn alternation_multiline_all_four_marker_kinds() {
    for (marker, kind) in [
        ("~", SyntaxKind::TILDE),
        ("&", SyntaxKind::AMP),
        ("!", SyntaxKind::BANG),
        ("|", SyntaxKind::PIPE),
    ] {
        let src = format!("flow f() {{\n  {{{marker}\n    - a\n    - b\n  }}\n}}\n");
        let p = assert_lossless(&src);
        assert!(p.errors().is_empty(), "{marker:?} errors: {:?}", p.errors());
        let alt: ast::AlternationBlock = p
            .syntax()
            .descendants()
            .find_map(ast::AlternationBlock::cast)
            .expect("ALTERNATION_BLOCK for marker");
        assert_eq!(alt.marker_token().expect("marker").kind(), kind);
        assert_eq!(alt.entries().count(), 2);
    }
}

#[test]
fn alternation_multiline_three_entries() {
    let src = "flow f() {\n  {&\n    - a\n    - b\n    - c\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let alt: ast::AlternationBlock = p
        .syntax()
        .descendants()
        .find_map(ast::AlternationBlock::cast)
        .expect("ALTERNATION_BLOCK");
    assert_eq!(alt.entries().count(), 3);
}

#[test]
fn alternation_multiline_entry_items_accessor() {
    let src = "flow f() {\n  {&\n    - Hello there.\n    - Bye now.\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let alt: ast::AlternationBlock = p
        .syntax()
        .descendants()
        .find_map(ast::AlternationBlock::cast)
        .expect("ALTERNATION_BLOCK");
    let entries: Vec<_> = alt.entries().collect();
    assert_eq!(entries.len(), 2);
    for entry in &entries {
        assert!(entry.items().next().is_some(), "entry has a body item");
    }
}

#[test]
fn alternation_multiline_entry_runs_until_next_dash_or_close() {
    // `entry` (family.rs) loops on `body_line` until the next `-` or `}`
    // — a multi-line entry body (more than one content line per `-`) must
    // all land inside the same ENTRY node.
    let src = "flow f() {\n  {&\n    - First line.\n      Second line.\n    - Other.\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let alt: ast::AlternationBlock = p
        .syntax()
        .descendants()
        .find_map(ast::AlternationBlock::cast)
        .expect("ALTERNATION_BLOCK");
    let entries: Vec<_> = alt.entries().collect();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[0]
            .items()
            .filter(|n| n.kind() == SyntaxKind::CONTENT_LINE)
            .count(),
        2,
        "both lines before the second `-` belong to the first ENTRY"
    );
}

// ── Section G: nesting — brace-family constructs inside each other ────

#[test]
fn alternation_nested_inside_conditional_braced_arm() {
    let src = "flow f() {\n  {if x { {~a|b} } else { none }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CONDITIONAL_BLOCK));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ALTERNATION_BLOCK));
}

#[test]
fn conditional_nested_inside_alternation_inline_alternative() {
    let src = "flow f() {\n  {~ {if x: a else: b} | c}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CONDITIONAL_BLOCK));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ALTERNATION_BLOCK));
}

#[test]
fn conditional_nested_inside_alternation_multiline_entry() {
    let src = "flow f() {\n  {&\n    - {if x: a else: b}\n    - plain\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CONDITIONAL_BLOCK));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ALTERNATION_BLOCK));
}

#[test]
fn alternation_nested_inside_match_braced_arm() {
    let src = "flow f() {\n  {match m {\n    a => { {~x|y} }\n  }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ALTERNATION_BLOCK));
    let cond: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("CONDITIONAL_BLOCK");
    assert!(cond.is_match());
}

#[test]
fn conditional_nested_inside_match_braced_arm() {
    let src = "flow f() {\n  {match m {\n    a => { {if x { yes } else { no }} }\n  }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(
        count_node_kind(&p.syntax(), SyntaxKind::CONDITIONAL_BLOCK),
        2
    );
}

#[test]
fn three_levels_deep_conditional_nesting() {
    let src = "flow f() {\n  {if a { {if b { {if c { deep } } } } }}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(
        count_node_kind(&p.syntax(), SyntaxKind::CONDITIONAL_BLOCK),
        3
    );
}

// ── Section H: error recovery — malformed input must not panic ────────

#[test]
fn truncated_if_with_no_expression_recovers() {
    let src = "flow f() {\n  {if}\n}\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "missing condition expr must be flagged"
    );
    // Still produces a CONDITIONAL_BLOCK (with an empty IF_ARM), not a
    // dropped/absent node — error recovery keeps the tree shape.
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CONDITIONAL_BLOCK));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::IF_ARM));
}

#[test]
fn truncated_match_with_no_subject_recovers() {
    let src = "flow f() {\n  {match}\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CONDITIONAL_BLOCK));
}

#[test]
fn unclosed_conditional_at_eof_recovers() {
    let src = "flow f() {\n  {if hp > 0 { You live.\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
}

#[test]
fn unclosed_alternation_at_eof_recovers() {
    let src = "flow f() {\n  {~ red|blue\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
}

#[test]
fn unclosed_match_arm_list_recovers() {
    let src = "flow f() {\n  {match mood { calm => \n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
}

#[test]
fn if_missing_body_opener_recovers() {
    // Neither `:` nor `{` after the condition — `arm_body` errors but
    // still makes forward progress (charter's "unexpected token" recovery
    // shape), not an infinite loop.
    let src = "flow f() {\n  {if hp > 0 no opener here}\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
}

#[test]
fn match_arm_missing_fat_arrow_recovers() {
    let src = "flow f() {\n  {match mood { calm Peaceful }}\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
}

#[test]
fn stray_dash_outside_alternation_is_not_an_entry() {
    // A bare `-` in ordinary prose (not inside an ALTERNATION_BLOCK) must
    // not be mistaken for an ENTRY marker — `multiline_entries`/`entry`
    // are only reachable from inside `alternation_block`.
    let src = "flow f() {\n  - just a dash in prose\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::ENTRY));
}

#[test]
fn empty_inline_alternation_emits_error() {
    // FIXED (ruled #1258/#1261, brink-syntax parity —
    // `sequence_stopping_empty_emits_error`/`sequence_symbol_empty_emits_error`):
    // `inline_alternatives` now tracks whether it saw any branch at all;
    // `alternation_block` emits a diagnostic when it didn't. An
    // `ALTERNATION_BLOCK` with only a marker child, no alternatives, is a
    // parse error, not a silently-accepted degenerate case.
    let src = "flow f() {\n  {~}\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty(), "empty alternation must now error");
    let alt: ast::AlternationBlock = p
        .syntax()
        .descendants()
        .find_map(ast::AlternationBlock::cast)
        .expect("ALTERNATION_BLOCK");
    assert_eq!(alt.entries().count(), 0);
}

#[test]
fn empty_multiline_alternation_emits_error() {
    // Same fix as the inline case above, multiline form: zero `-` entries
    // now raises the same diagnostic.
    let src = "flow f() {\n  {&\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty(), "empty alternation must now error");
    let alt: ast::AlternationBlock = p
        .syntax()
        .descendants()
        .find_map(ast::AlternationBlock::cast)
        .expect("ALTERNATION_BLOCK");
    assert_eq!(alt.entries().count(), 0);
}

// ── Section I: adversarial inputs targeting family.rs's dispatch ──────

#[test]
fn keyword_prefixed_identifier_is_not_mistaken_for_the_keyword() {
    // "ifoo"/"matcher" lex as single IDENT tokens (max-munch), so
    // `at_conditional`'s `p.nth(1)` lookahead never sees a bare
    // `KW_IF`/`KW_MATCH` here — this is bare `{expr}` interpolation, not
    // a CONDITIONAL_BLOCK. Exercises the family dispatch's reliance on
    // token-kind equality rather than any textual prefix check.
    for src in ["flow f() {\n  {ifoo}\n}\n", "flow f() {\n  {matcher}\n}\n"] {
        let p = assert_lossless(src);
        assert!(p.errors().is_empty(), "{src:?} errors: {:?}", p.errors());
        assert!(!has_node_kind(&p.syntax(), SyntaxKind::CONDITIONAL_BLOCK));
        assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    }
}

#[test]
fn each_alternation_sigil_does_not_leak_into_choice_or_conditional_dispatch() {
    // `at_choice_point`/`at_conditional`/`at_alternation` all gate on
    // `p.nth(1)` from the SAME `L_BRACE` — confirm the three checks are
    // mutually exclusive for every sigil in this family, i.e. a `{~…}`
    // never also satisfies `at_conditional`, etc. If a future edit ever
    // widened one of these lookaheads to overlap another, this would
    // start asserting the wrong node kind and fail loudly.
    let cases: &[(&str, SyntaxKind)] = &[
        (
            "flow f() {\n  {? \n    * a\n  }\n}\n",
            SyntaxKind::CHOICE_POINT,
        ),
        (
            "flow f() {\n  {if x: y}\n}\n",
            SyntaxKind::CONDITIONAL_BLOCK,
        ),
        (
            "flow f() {\n  {match x { y => z }}\n}\n",
            SyntaxKind::CONDITIONAL_BLOCK,
        ),
        ("flow f() {\n  {~a|b}\n}\n", SyntaxKind::ALTERNATION_BLOCK),
        ("flow f() {\n  {&a|b}\n}\n", SyntaxKind::ALTERNATION_BLOCK),
        ("flow f() {\n  {!a|b}\n}\n", SyntaxKind::ALTERNATION_BLOCK),
        ("flow f() {\n  {|a|b}\n}\n", SyntaxKind::ALTERNATION_BLOCK),
    ];
    for (src, expected) in cases {
        let p = assert_lossless(src);
        assert!(p.errors().is_empty(), "{src:?} errors: {:?}", p.errors());
        assert!(
            has_node_kind(&p.syntax(), *expected),
            "{src:?} expected a {expected:?}, tree: {:#?}",
            p.syntax()
        );
        for other in [
            SyntaxKind::CHOICE_POINT,
            SyntaxKind::CONDITIONAL_BLOCK,
            SyntaxKind::ALTERNATION_BLOCK,
        ] {
            if other != *expected {
                assert!(
                    !has_node_kind(&p.syntax(), other),
                    "{src:?} unexpectedly also produced a {other:?}"
                );
            }
        }
    }
}

#[test]
fn second_marker_char_is_literal_text_not_a_combined_marker() {
    // The marker is exactly one token (`alternation_marker` bumps once) —
    // unlike ink's combined symbol annotations (`{&!…}`), a second sigil
    // character right after the marker is NOT special-cased here; it
    // falls into the loop's generic `content_items_until` dispatch and
    // becomes literal prose. Documents current (deliberately minimal)
    // shape, not a bug: charter §6 spells single-char markers only.
    let src = "flow f() {\n  {~&x|y}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let alt: ast::AlternationBlock = p
        .syntax()
        .descendants()
        .find_map(ast::AlternationBlock::cast)
        .expect("ALTERNATION_BLOCK");
    assert_eq!(
        alt.marker_token().expect("marker").kind(),
        SyntaxKind::TILDE
    );
    assert_eq!(text_run_concat(alt.syntax()), "&xy");
}

#[test]
fn lone_l_brace_at_eof_never_panics() {
    let src = "flow f() {\n  {\n";
    let p = assert_lossless(src);
    let _ = p.errors();
}

#[test]
fn bare_brace_immediately_followed_by_close_is_multiline_shape_not_alternation() {
    // A brace immediately followed by newline-then-close is the
    // "multiline body opener" shape `is_multiline`/`is_body_open_brace`
    // key off — but only alternation markers (`~&!|`) trigger
    // `at_alternation`; a bare `{` with nothing recognizable right after
    // it is plain interpolation, whose missing expression is reported,
    // not silently swallowed as a zero-branch alternation.
    let src = "flow f() {\n  {\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::ALTERNATION_BLOCK));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::CONDITIONAL_BLOCK));
}

#[test]
fn fuzz_deeply_nested_conditional_braces_completes_and_hits_depth_guard() {
    // Regression-shaped after `brink-syntax`'s
    // `fuzz_deeply_nested_braces_completes` and this crate's own
    // `deeply_nested_interpolation_does_not_overflow_stack`
    // (trivia.rs) — repeated unclosed `{if x {` must hit `MAX_DEPTH`
    // (256) and recover rather than blow the stack or hang. `depth` is
    // one shared counter (`Parser::depth`) crossed by both
    // `conditional_block`'s and `expression`'s own `enter_depth` calls,
    // so this single repeated pattern exercises both.
    let mut src = String::from("flow f() {\n");
    for _ in 0..300 {
        src.push_str("{if x {");
    }
    let p = assert_lossless(&src);
    assert!(
        p.errors()
            .iter()
            .any(|e| e.message.contains("maximum nesting depth exceeded")),
        "expected the depth guard to fire at least once"
    );
}

#[test]
fn fuzz_deeply_nested_alternation_entries_completes() {
    let mut src = String::from("flow f() {\n");
    for _ in 0..300 {
        src.push_str("{~ {if x { ");
    }
    let p = assert_lossless(&src);
    // Bounded completion + lossless round-trip is the property under
    // test; whatever error shape results is secondary.
    let _ = p.errors();
}

#[test]
fn garbage_tokens_inside_conditional_body_never_panic() {
    let src = "flow f() {\n  {if x { }}}{{{ @[ ) :: match if if\n}\n";
    let p = assert_lossless(src);
    let _ = p.errors();
}

// ── Section J: proptest round-trip generators ──────────────────────────
//
// Local to this file (issue #1199 owns `tests/proptest_native.rs` this
// wave) — a small generator covering the shapes this family adds:
// `{if}`/`{match}` (both body forms) and multiline alternation.

// ── The `as` binding in the template condition position (B1b, issue
//    #1475) ──────────────────────────────────────────────────────────

#[test]
fn conditional_block_carries_an_as_binding_on_the_colon_form() {
    let p = assert_lossless("{if party.leader as l: {l} leads. else: Nobody leads.}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cb: ast::ConditionalBlock = find_child(&p.syntax()).expect("CONDITIONAL_BLOCK");
    assert!(cb.is_if());
    // The head expression accessor must still find the condition, not the
    // trailing `AS_BINDING` sibling.
    assert_eq!(
        cb.condition().map(|n| n.kind()),
        Some(SyntaxKind::PATH_EXPR)
    );
    assert_eq!(
        cb.as_binding()
            .and_then(|b| b.name_token())
            .map(|t| t.text().to_string()),
        Some("l".to_string())
    );
    assert!(cb.if_arm().is_some());
    assert!(cb.else_arm().is_some());
}

#[test]
fn conditional_block_carries_an_as_binding_on_the_braced_form() {
    let p = assert_lossless("{if find(s, \"x\") as i { Found {i}. }}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cb: ast::ConditionalBlock = find_child(&p.syntax()).expect("CONDITIONAL_BLOCK");
    assert_eq!(
        cb.as_binding()
            .and_then(|b| b.name_token())
            .map(|t| t.text().to_string()),
        Some("i".to_string())
    );
}

#[test]
fn conditional_block_without_as_has_no_binding() {
    let p = assert_lossless("{if ready: go else: wait}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cb: ast::ConditionalBlock = find_child(&p.syntax()).expect("CONDITIONAL_BLOCK");
    assert!(cb.as_binding().is_none());
}

#[test]
fn choice_guard_accepts_an_as_binding_for_brink_ir_to_diagnose() {
    // Guard-`as` is ruled but unimplemented (it rides the `.inkb` v6 Choice
    // record) — the grammar accepts it so `brink-ir` can say "not yet
    // supported" (E141) instead of the parser saying "unexpected token".
    let src = "flow f() {\n  {?\n    * {if find(s, \"x\") as i} take it\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let guard = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_GUARD)
        .and_then(ast::ChoiceGuard::cast)
        .expect("CHOICE_GUARD");
    assert_eq!(guard.expr().map(|n| n.kind()), Some(SyntaxKind::CALL_EXPR));
    assert_eq!(
        guard
            .as_binding()
            .and_then(|b| b.name_token())
            .map(|t| t.text().to_string()),
        Some("i".to_string())
    );
}

mod proptests {
    use super::*;
    use proptest::prelude::*;
    use std::fmt::Write as _;

    const NUM_CASES: u32 = 256;

    // Mirrors `tests/proptest_native.rs`'s own `KEYWORDS` list (this
    // family's generators are local to this file, so the list is
    // duplicated here rather than shared — see the module doc comment
    // above). Every hard-reserved keyword must appear here or proptest
    // can generate it into an identifier position and red the case.
    const KEYWORDS: &[&str] = &[
        "flow", "fn", "var", "const", "let", "flags", "struct", "extern", "import", "use",
        "module", "return", "ref", "if", "match", "else", "while", "for", "in", "until", "break",
        "continue", "as", "or", "true", "false", "END", "DONE",
    ];

    fn arb_ident() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9_]{0,6}"
            .prop_filter("must not be a keyword", |s| !KEYWORDS.contains(&s.as_str()))
    }

    fn arb_text() -> impl Strategy<Value = String> {
        "[a-z][a-zA-Z0-9 ]{0,12}"
    }

    fn arb_conditional_colon() -> impl Strategy<Value = String> {
        (arb_ident(), arb_text(), arb_text())
            .prop_map(|(cond, t, f)| format!("{{if {cond}: {t} else: {f}}}\n"))
    }

    fn arb_conditional_braced() -> impl Strategy<Value = String> {
        (arb_ident(), arb_text(), arb_text())
            .prop_map(|(cond, t, f)| format!("{{if {cond} {{ {t} }} else {{ {f} }} }}\n"))
    }

    /// Flat `else if` chain (ruled #1258, implemented #1261) — a third
    /// generator alongside the colon/braced pair above, covering the shape
    /// `at_else_arm`'s new `KW_IF` arm-opener handles.
    fn arb_conditional_else_if_flat() -> impl Strategy<Value = String> {
        (arb_ident(), arb_ident(), arb_text(), arb_text(), arb_text()).prop_map(
            |(cond_a, cond_b, a, b, c)| {
                format!("{{if {cond_a} {{ {a} }} else if {cond_b} {{ {b} }} else {{ {c} }}}}\n")
            },
        )
    }

    fn arb_match_braced() -> impl Strategy<Value = String> {
        (
            arb_ident(),
            prop::collection::vec((arb_ident(), arb_text()), 1..=3),
        )
            .prop_map(|(subject, arms)| {
                let body = arms.into_iter().fold(String::new(), |mut acc, (pat, txt)| {
                    let _ = write!(acc, "{pat} => {{ {txt} }}, ");
                    acc
                });
                format!("{{match {subject} {{ {body}}}}}\n")
            })
    }

    fn arb_alternation_inline() -> impl Strategy<Value = String> {
        (
            prop::sample::select(&["~", "&", "!", "|"][..]),
            prop::collection::vec(arb_text(), 2..=4),
        )
            .prop_map(|(marker, alts)| format!("{{{marker} {}}}\n", alts.join("|")))
    }

    fn arb_alternation_multiline() -> impl Strategy<Value = String> {
        (
            prop::sample::select(&["~", "&", "!", "|"][..]),
            prop::collection::vec(arb_text(), 2..=4),
        )
            .prop_map(|(marker, entries)| {
                let body = entries.iter().fold(String::new(), |mut acc, e| {
                    let _ = writeln!(acc, "    - {e}");
                    acc
                });
                format!("{{{marker}\n{body}  }}\n")
            })
    }

    fn arb_family_line() -> impl Strategy<Value = String> {
        prop_oneof![
            arb_conditional_colon(),
            arb_conditional_braced(),
            arb_conditional_else_if_flat(),
            arb_match_braced(),
            arb_alternation_inline(),
            arb_alternation_multiline(),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(NUM_CASES))]

        /// Every generated brace-family construct, wrapped in a flow
        /// body, round-trips losslessly — the heart property
        /// (`docs/b0-sequencing.md` §B0.5's exit criterion), specialized
        /// to this family's own generators.
        #[test]
        fn brace_family_construct_round_trips(line in arb_family_line()) {
            let src = format!("flow f() {{\n  {line}}}\n");
            let p = parse(&src);
            prop_assert_eq!(&src, &p.syntax().text().to_string());
        }

        /// #1261's two conditional-arm fixes hold at scale, not just on the
        /// hand-picked unit-test examples: every generated same-line
        /// colon-form else and every generated flat `else if` chain parses
        /// with zero errors.
        #[test]
        fn conditional_colon_and_else_if_flat_parse_clean(
            colon_line in arb_conditional_colon(),
            else_if_line in arb_conditional_else_if_flat(),
        ) {
            for line in [colon_line, else_if_line] {
                let src = format!("flow f() {{\n  {line}}}\n");
                let p = parse(&src);
                prop_assert!(p.errors().is_empty(), "{src:?} errors: {:?}", p.errors());
                prop_assert!(has_node_kind(&p.syntax(), SyntaxKind::ELSE_BRANCH), "{src:?}");
            }
        }

        /// Two brace-family constructs back to back (still inside one
        /// flow body) round-trip too — guards against the dispatch loop
        /// making zero progress and needing `error_recover` between two
        /// adjacent constructs of this family specifically.
        #[test]
        fn two_brace_family_constructs_back_to_back_round_trip(
            a in arb_family_line(),
            b in arb_family_line(),
        ) {
            let src = format!("flow f() {{\n  {a}  {b}}}\n");
            let p = parse(&src);
            prop_assert_eq!(&src, &p.syntax().text().to_string());
        }
    }
}
