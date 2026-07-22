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
fn colon_form_else_on_the_same_line_is_not_recognized_as_an_else_arm() {
    // GENUINE PARSER BUG (not fixed here — test-only issue). `colon_body`
    // (family.rs) only calls `at_else_arm` BETWEEN top-level `body_line`
    // calls — never mid-line. `body_line` → `content_line` scans an
    // entire physical line as one `CONTENT_LINE`, stopping only at
    // `NEWLINE`/`R_BRACE`/`HASH` — `else` has no special recognition
    // inside that scan. The result: the "inline colon body" form the
    // `CONDITIONAL_BLOCK` doc comment calls out BY NAME as "the inline
    // colon body (`{if cond: … else: …}`, charter §6's literal example)"
    // — a genuinely single physical line — silently fails to produce an
    // `ELSE_BRANCH` at all when written as its own literal example
    // spells it: `else` gets swallowed into the if-arm's `TEXT`, and
    // whatever follows `else:` is lost as sibling prose alongside it,
    // not a real fallback branch. Confirmed via `conditional_block_colon_form`
    // above, which has asserted `errors().is_empty()` since before this
    // family's test-coverage pass — that assertion is still true (no
    // parse error is raised), which is exactly why this has gone
    // unnoticed: the failure is silent, not diagnosed.
    //
    // Multi-line colon bodies (`else:` starting its own line) DO work —
    // see `conditional_colon_if_and_else_items_accessors` below — so the
    // bug is specifically "on the identical physical line as trailing
    // if-body content", not "colon-form else in general".
    //
    // Reported on #1197 (scope overflow). TODO: update this assertion if
    // `colon_body` is later taught to recognize `else` as a boundary
    // mid-line (would need `content_items_until` or `colon_body` itself
    // to special-case `KW_ELSE` the way it already special-cases `HASH`).
    let src = "flow garden() {\n  {if hp > 0: You live. else: You die.}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cond: ast::ConditionalBlock = p
        .syntax()
        .descendants()
        .find_map(ast::ConditionalBlock::cast)
        .expect("CONDITIONAL_BLOCK");
    assert!(
        cond.else_arm().is_none(),
        "TODO(#1197 follow-up): no ELSE_BRANCH is produced today; `else: You die.` \
         is swallowed into the if-arm's TEXT instead — update this assertion once fixed"
    );
    // The swallowed "else: You die." literally appears as visible prose
    // text inside the if-arm — not just "missing", but silently wrong.
    let if_arm = cond.if_arm().expect("IF_ARM");
    assert!(
        text_run_concat(if_arm.syntax()).contains("else"),
        "the else keyword and its body leaked into the if-arm's TEXT"
    );
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
    let src = "flow garden() {\n  {if hp > 0 { You live. } else: You die.}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
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
fn else_if_flat_chain_is_not_recognized_as_a_chain() {
    // GENUINE GRAMMAR GAP (not fixed here — test-only issue, see
    // brace_family.rs module doc + CLAUDE.md "do not patch symptoms"):
    // `at_else_arm` (family.rs) only starts an `ELSE_BRANCH` when `else`
    // is IMMEDIATELY followed by its body opener (`{` or `:`) — `else if`
    // written flat, without an inner `{if …}` wrapper, does NOT chain.
    // `if_arm` closes, `at_else_arm` sees `KW_IF` (not `L_BRACE`/`COLON`)
    // and returns false, so `conditional_block` never opens an
    // `ELSE_BRANCH` at all: it immediately `expect(R_BRACE)`s against the
    // literal `else` token (error), unwinds, and the rest
    // (`else if b { B } else { C }`) falls through to the enclosing
    // block's ordinary body-line dispatch as prose/interpolation debris.
    // Reported on #1197 (scope overflow) rather than fixed here — fixing
    // it means either teaching `at_else_arm` to recognize `else if` as a
    // third arm-opener shape or auto-wrapping, either of which is a real
    // grammar change, not test coverage.
    let src = "flow garden() {\n  {if a { A } else if b { B } else { C }}\n}\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "TODO(#1197 follow-up): flat `else if` currently fails to chain; \
         update this assertion if/when `at_else_arm` gains `else if` support"
    );
    // The malformed tail never becomes a second CONDITIONAL_BLOCK arm —
    // it's stray prose/interpolation content instead.
    assert_eq!(
        count_node_kind(&p.syntax(), SyntaxKind::CONDITIONAL_BLOCK),
        1
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
    // tag doesn't spin the loop (family.rs comment on the `HASH` arm).
    let src = "flow f() {\n  {~a|b} #tag\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
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
fn empty_inline_alternation_currently_accepted_without_error() {
    // GAP relative to the parity target: `brink-syntax`'s twin
    // (`sequence_stopping_empty_emits_error`/`sequence_symbol_empty_emits_error`)
    // requires at least one branch and errors on `{&}`/`{stopping:}`. This
    // grammar's `inline_alternatives` loop breaks immediately on `R_BRACE`
    // with zero iterations and records no error — an `ALTERNATION_BLOCK`
    // with only a marker child, no alternatives, is currently accepted.
    // Not fixed here (test-only issue); reported on #1197.
    let src = "flow f() {\n  {~}\n}\n";
    let p = assert_lossless(src);
    assert!(
        p.errors().is_empty(),
        "TODO(#1197 follow-up): update if empty-alternation validation is added; errors: {:?}",
        p.errors()
    );
    let alt: ast::AlternationBlock = p
        .syntax()
        .descendants()
        .find_map(ast::AlternationBlock::cast)
        .expect("ALTERNATION_BLOCK");
    assert_eq!(alt.entries().count(), 0);
}

#[test]
fn empty_multiline_alternation_currently_accepted_without_error() {
    // Same gap as the inline case above, multiline form: zero `-` entries,
    // no diagnostic.
    let src = "flow f() {\n  {&\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(
        p.errors().is_empty(),
        "TODO(#1197 follow-up): update if empty-alternation validation is added; errors: {:?}",
        p.errors()
    );
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

mod proptests {
    use super::*;
    use proptest::prelude::*;
    use std::fmt::Write as _;

    const NUM_CASES: u32 = 256;

    fn arb_ident() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9_]{0,6}".prop_filter("must not be a keyword", |s| {
            !matches!(s.as_str(), "if" | "match" | "else" | "flow" | "fn")
        })
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
