//! Content lines & interpolation — glue, labels, prose whitespace.
//! Family for #1194.

use super::*;

// ── Significant inter-token whitespace in content position ───────────────

#[test]
fn space_after_glue_marker_survives_inside_the_text_node() {
    // `<> But surely.` — the space after the `<>` glue marker is the leading
    // char of the following prose run and must be preserved in the `TEXT`
    // node, not discarded (the exhibit-fogg glue lines' divergence).
    let src = "flow f() {\n  <> But surely.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());

    let line = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    assert!(has_node_kind(&line, SyntaxKind::GLUE_NODE));
    assert_eq!(
        text_run_concat(&line),
        " But surely.",
        "space after `<>` must be folded into the following TEXT node"
    );
}

#[test]
fn interior_prose_whitespace_between_words_is_unchanged() {
    // Guard the baseline the fix must not regress: a plain content line's
    // interior word spacing was already preserved by `text_run_until`, and
    // still is.
    let src = "flow f() {\n  You have three gold coins.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    assert_eq!(text_run_concat(&line), "You have three gold coins.");
}

// ── G-1: labeled content lines ────────────────────────────────────────

#[test]
fn labeled_content_line_produces_a_label_node() {
    let src = "flow f() {\n  (start) You arrive at the garden.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let content_line = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    assert!(has_node_kind(&content_line, SyntaxKind::LABEL));
    assert!(has_node_kind(&content_line, SyntaxKind::TEXT));
}

#[test]
fn labeled_content_line_as_backward_loop_divert_target() {
    // Ink's `- (start)` mid-flow re-entry pattern (README G-1 finding):
    // a label on a plain content line, later diverted back to from
    // further down the same flow.
    let src = concat!(
        "flow loop() {\n",
        "  (start) You spin around.\n",
        "  {?\n",
        "    * [Again] -> start\n",
        "    * [Stop] -> END\n",
        "  }\n",
        "}\n",
    );
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::LABEL));
    // Both diverts (one to the label, one to END) parse as real nodes —
    // exercises N-1 and G-1 together, the realistic combined idiom.
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::DIVERT_STMT), 2);
}

#[test]
fn unlabeled_prose_starting_with_paren_is_unaffected() {
    // A multi-word parenthetical does not match the `L_PAREN IDENT
    // R_PAREN` lookahead shape, so it stays plain prose, not a spurious
    // LABEL + error.
    let src = "flow f() {\n  (a very long aside) continues here.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::LABEL));
}

#[test]
fn label_inside_conditional_body_is_still_a_content_line_label() {
    // G-1 says "ANY content line" — including one nested inside the
    // annotated-brace family's colon/braced bodies, since those recurse
    // through `body_line`/`content_line` too.
    let src = "flow f() {\n  {if hp > 0: (alive) You live.}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::LABEL));
}

// ── Section A: TEXT runs — words & punctuation ─────────────────────────
//
// Parity target: `brink-syntax/src/parser/tests/content/cst.rs` Section A.
// Native has no `MIXED_CONTENT` wrapper — `TEXT`/`GLUE_NODE`/`INTERPOLATION`
// land as direct `CONTENT_LINE` children — so these assert node presence
// and text content rather than a full CST shape.

#[test]
fn single_word_is_one_text_node() {
    let p = assert_lossless("Hello\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::TEXT), 1);
}

#[test]
fn multiple_words_aggregate_into_one_text_node() {
    // No structural break between "The", "quick", "brown", "fox" — the
    // whole run is a single `TEXT`, not one node per word.
    let p = assert_lossless("The quick brown fox\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::TEXT), 1);
    assert_eq!(text_run_concat(&p.syntax()), "The quick brown fox");
}

#[test]
fn punctuation_characters_stay_plain_text() {
    // Every one of these characters has no structural meaning in content
    // position (unlike `{`, `<`+`>`, `#`, `->`) and must fold into the
    // surrounding `TEXT` run untouched.
    for src in [
        "Hello.\n",
        "Hello, world\n",
        "It works!\n",
        "How are you?\n",
        "Name: Bob\n",
        "A; B\n",
        "Hello (world)\n",
        "She said \"hello\"\n",
        "Player 1\n",
        "A = B\n",
        "50% chance\n",
        "a & b\n",
        "x * y\n",
        "1 + 1\n",
    ] {
        let p = assert_lossless(src);
        assert!(
            p.errors().is_empty(),
            "src {src:?} errors: {:?}",
            p.errors()
        );
        assert_eq!(
            count_node_kind(&p.syntax(), SyntaxKind::TEXT),
            1,
            "src {src:?} should be a single TEXT run"
        );
    }
}

#[test]
fn text_at_eof_without_trailing_newline() {
    let p = assert_lossless("Hello");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CONTENT_LINE));
    assert_eq!(text_run_concat(&p.syntax()), "Hello");
}

#[test]
fn consecutive_content_lines_are_separate_content_line_nodes() {
    let p = assert_lossless("Line one.\nLine two.\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::CONTENT_LINE), 2);
}

// ── Section B: GLUE ─────────────────────────────────────────────────────

#[test]
fn glue_between_two_text_runs() {
    let p = assert_lossless("a<>b\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::GLUE_NODE));
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::TEXT), 2);
}

#[test]
fn glue_at_line_start() {
    let p = assert_lossless("<>continued\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    // GLUE_NODE must be the line's first child — nothing precedes it.
    let first_child_kind = line.children().next().map(|c| c.kind());
    assert_eq!(first_child_kind, Some(SyntaxKind::GLUE_NODE));
}

#[test]
fn glue_at_line_end_before_newline() {
    let p = assert_lossless("text<>\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::GLUE_NODE));
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::TEXT), 1);
}

#[test]
fn multiple_glue_operators_in_one_line() {
    let p = assert_lossless("a<>b<>c\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::GLUE_NODE), 2);
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::TEXT), 3);
}

#[test]
fn consecutive_glue_operators_with_no_text_between() {
    let p = assert_lossless("<><>\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::GLUE_NODE), 2);
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::TEXT), 0);
}

#[test]
fn glue_only_line_has_no_text_node() {
    let p = assert_lossless("<>\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::GLUE_NODE));
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::TEXT), 0);
}

#[test]
fn glue_before_a_divert() {
    // N-1 + glue together: the divert still stops the text/glue scan
    // cleanly, no swallowed `->` into `TEXT`.
    let src = "text<> -> knot\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::GLUE_NODE));
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::DIVERT_STMT), 1);
}

#[test]
fn lone_angle_bracket_without_its_pair_is_plain_text() {
    // Only the two-character `<>` token is glue (charter §5, "kept as-is").
    // A bare `<` (no matching `>` immediately after) lexes as `LT` and
    // falls through to ordinary prose text — no `GLUE_NODE`, no error.
    let p = assert_lossless("a < b\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::GLUE_NODE));
    assert_eq!(text_run_concat(&p.syntax()), "a < b");
}

// ── Section C: INTERPOLATION — every expression kind (charter §6: bare
// `{expr}` = interpolation, and nothing else, ever) ─────────────────────

#[test]
fn interpolation_wraps_an_integer_literal() {
    let p = assert_lossless("Roll: {12}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTEGER_LIT));
}

#[test]
fn interpolation_wraps_a_float_literal() {
    let p = assert_lossless("Roll: {3.5}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::FLOAT_LIT));
}

#[test]
fn interpolation_wraps_a_string_literal() {
    let p = assert_lossless("Say: {\"hi\"}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::STRING_LIT));
}

#[test]
fn interpolation_wraps_boolean_true() {
    let p = assert_lossless("Flag: {true}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::BOOLEAN_LIT));
}

#[test]
fn interpolation_wraps_boolean_false() {
    let p = assert_lossless("Flag: {false}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::BOOLEAN_LIT));
}

#[test]
fn interpolation_wraps_a_bare_path() {
    let p = assert_lossless("Hello {name}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::PATH_EXPR));
}

#[test]
fn interpolation_wraps_a_dotted_path() {
    let p = assert_lossless("HP: {player.stats.hp}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::PATH_SEGMENT), 3);
}

#[test]
fn interpolation_wraps_a_paren_expr() {
    let p = assert_lossless("Sum: {(1 + 2)}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::PAREN_EXPR));
}

#[test]
fn interpolation_wraps_a_prefix_minus_expr() {
    let p = assert_lossless("Delta: {-x}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::PREFIX_EXPR));
}

#[test]
fn interpolation_wraps_an_infix_expr() {
    let p = assert_lossless("Roll: {1 + 2}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INFIX_EXPR));
}

#[test]
fn interpolation_wraps_a_call_expr() {
    let p = assert_lossless("Result: {roll(1, 6)}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CALL_EXPR));
}

#[test]
fn interpolation_at_start_of_content_line() {
    let p = assert_lossless("{name} arrives.\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    let first_child_kind = line.children().next().map(|c| c.kind());
    assert_eq!(first_child_kind, Some(SyntaxKind::INTERPOLATION));
}

#[test]
fn interpolation_at_end_of_content_line() {
    let p = assert_lossless("You are {name}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION), 1);
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::TEXT), 1);
}

#[test]
fn interpolation_between_two_text_runs() {
    let p = assert_lossless("before {x} after\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::TEXT), 2);
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION), 1);
}

#[test]
fn multiple_interpolations_in_one_line() {
    let p = assert_lossless("{a} and {b}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION), 2);
}

#[test]
fn whitespace_between_two_interpolations_survives_inside_a_text_node() {
    // #1264 (fixes the FIXME this test used to pin, filed on #1194): the
    // significant-whitespace fix this file's very first test guards
    // (`space_after_glue_marker_survives_inside_the_text_node`) only folded
    // leading whitespace into a FOLLOWING `TEXT` run — `starts_text_run`
    // returned `false` for `L_BRACE` unconditionally, so when an
    // interpolation followed whitespace (rather than prose),
    // `content_items_until` called `skip_ws()` and the space was bumped
    // bare, landing as a plain `WHITESPACE` token directly under
    // `CONTENT_LINE`, not inside any `TEXT` node. Since content lowering
    // iterates node children, a bare token produced no visible output —
    // the word-separator space between two interpolations was silently
    // dropped from rendered prose (`"{a} {b}"` → `"AliceBob"` instead of
    // `"Alice Bob"`). `content_items_until` now folds that pending trivia
    // into its own `TEXT` node (mirroring the glue-space fix's treatment)
    // before dispatching the next interpolation.
    let p = assert_lossless("{a} {b}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION), 2);
    assert_eq!(
        text_run_concat(&p.syntax()),
        " ",
        "the separating space must live inside a TEXT node so content lowering keeps it"
    );
    let line = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    let has_bare_whitespace_child = line
        .children_with_tokens()
        .any(|c| c.kind() == SyntaxKind::WHITESPACE);
    assert!(
        !has_bare_whitespace_child,
        "the space must not show up as a bare WHITESPACE token outside any node"
    );
}

#[test]
fn interpolation_flanked_by_glue() {
    let p = assert_lossless("<>{x}<>\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::GLUE_NODE), 2);
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION), 1);
}

// ── Section C2: alternation-marker precedence over interpolation (ruled
// 2026-07-22, #1258/#1261 — "alternation markers win"; formerly pinned as
// unresolved grammar collisions by the #1194 coverage wave) ─────────────

#[test]
fn alternation_once_marker_wins_over_prefix_not_expression() {
    // RULED (#1258/#1261): `at_alternation` (`family.rs`) claims any `{!`
    // unconditionally for the alternation family's `{! }` "once" marker
    // (charter §6), with no lookahead past the single char — this makes it
    // impossible to spell a bare-brace interpolation whose expression
    // starts with the `!` prefix operator (`is_prefix_op` in `expr.rs`
    // does allow `!`, symmetrically with `-`) directly: `{!x}` can never
    // reach `interpolation()` the way `{-x}` does two tests up. Ruled
    // acceptable ("alternation markers win") rather than a bug — the
    // escape hatch is parens, see `paren_escaped_prefix_not_expression_
    // reaches_interpolation_despite_alternation_marker_win` below.
    let p = assert_lossless("Not: {!x}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(
        has_node_kind(&p.syntax(), SyntaxKind::ALTERNATION_BLOCK),
        "ruled behavior: {{!x}} is claimed by the alternation family"
    );
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::PREFIX_EXPR));
}

#[test]
fn paren_escaped_prefix_not_expression_reaches_interpolation_despite_alternation_marker_win() {
    // The ruling's escape hatch (#1258/#1261): `L_PAREN` is never an
    // alternation marker char, so wrapping the expression in parens always
    // falls through past `at_alternation`/`at_conditional`/`at_choice_point`
    // straight to `content::interpolation`, regardless of which marker
    // char the expression itself would otherwise start with.
    let p = assert_lossless("Not: {(!x)}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::PAREN_EXPR));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::PREFIX_EXPR));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::ALTERNATION_BLOCK));
}

#[test]
fn alternation_stopping_marker_wins_over_lambda_expression() {
    // RULED (2026-07-22, superseding the earlier "malformed lambda" clause):
    // `{|}` is a real stopping-sequence marker (charter §116), so a
    // pipe-led brace is ALWAYS a stopping-sequence — `{|x| x}` is a valid
    // two-branch stopping-sequence (branches `x` and `x`), not a "malformed
    // lambda". The alternation family wins the dispatch (a lambda in content
    // position can never reach `interpolation()`), and a real lambda is
    // spelled with parens (`{(|x| x)}`, tested below). There is no
    // malformed-lambda diagnostic; the space after the separator is ordinary
    // branch content.
    let p = assert_lossless("Lambda: {|x| x}\n");
    assert!(
        p.errors().is_empty(),
        "`{{|x| x}}` is a valid stopping-sequence, not an error: {:?}",
        p.errors()
    );
    assert!(
        has_node_kind(&p.syntax(), SyntaxKind::ALTERNATION_BLOCK),
        "`{{|x| x}}` is claimed by the alternation family as a stopping-sequence"
    );
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::LAMBDA_EXPR));
}

#[test]
fn paren_escaped_lambda_expression_reaches_interpolation_despite_alternation_marker_win() {
    // The ruling's escape hatch, lambda case: parens route past the
    // marker-claims-`{|` dispatch the same way they do for the once-marker
    // case above, reaching a real `LAMBDA_EXPR` with no malformed-
    // alternation diagnostic.
    let p = assert_lossless("Lambda: {(|x| x)}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::PAREN_EXPR));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::LAMBDA_EXPR));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::ALTERNATION_BLOCK));
}

#[test]
fn two_branch_pipe_alternation_parses_clean_with_or_without_space() {
    // RULED 2026-07-22: every `{|…}` is a stopping-sequence, so BOTH the
    // glued form `{|heads|tails}` and the spaced form `{|heads| tails}` are
    // valid two-branch alternations — the space is ordinary branch content,
    // never a lambda signal. (An earlier revision mis-flagged the spaced
    // form as a malformed lambda; that heuristic was removed.)
    for src in ["Pick: {|heads|tails}\n", "Pick: {|heads| tails}\n"] {
        let p = assert_lossless(src);
        assert!(p.errors().is_empty(), "{src:?} errors: {:?}", p.errors());
        assert!(has_node_kind(&p.syntax(), SyntaxKind::ALTERNATION_BLOCK));
        assert!(!has_node_kind(&p.syntax(), SyntaxKind::LAMBDA_EXPR));
    }
}

// ── Section D: TAG_LINE vs. a content line's trailing TAG tail ─────────

#[test]
fn standalone_hash_line_is_tag_line_not_content_line() {
    let p = assert_lossless("#tag\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::TAG_LINE));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::CONTENT_LINE));
}

#[test]
fn content_line_with_one_trailing_tag() {
    let p = assert_lossless("Hello #tag\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    assert!(has_node_kind(&line, SyntaxKind::TEXT));
    assert_eq!(count_node_kind(&line, SyntaxKind::TAG), 1);
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::TAG_LINE));
}

#[test]
fn content_line_with_two_trailing_tags() {
    let p = assert_lossless("Hello #tag1 #tag2\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::TAG), 2);
}

#[test]
fn content_divert_then_trailing_tag() {
    let p = assert_lossless("Hello -> knot #tag\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    assert!(has_node_kind(&line, SyntaxKind::TEXT));
    assert!(has_node_kind(&line, SyntaxKind::DIVERT_STMT));
    assert!(has_node_kind(&line, SyntaxKind::TAG));
}

#[test]
fn tags_with_no_space_between_are_two_separate_tag_nodes() {
    let p = assert_lossless("Hello #a#b\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::TAG), 2);
}

#[test]
fn tag_text_runs_raw_to_end_of_line() {
    // A tag's body is scanned raw until NEWLINE/EOF/HASH/R_BRACE (`tag()`
    // in `content.rs`) — punctuation inside it is not re-parsed as prose
    // structure.
    let p = assert_lossless("Hello #tag: with, punctuation!\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::TAG), 1);
}

#[test]
fn a_tag_containing_a_balanced_interpolation_brace_does_not_end_early() {
    // #1728: `tag()`'s free-text scan used to stop at the FIRST literal
    // `}`, even one that only closes a `{…}` the tag's own raw text had
    // already echoed open. The `}` matching that `{` must not be mistaken
    // for the tag's terminator — the tag keeps scanning past it to the
    // real end of line, and the enclosing flow's own closing `}` is never
    // fooled into closing early.
    let src = "flow f() {\n  Hello #tag {gold} coins.\n  The river bends.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::FLOW_DECL), 1);
    assert_eq!(
        count_node_kind(&p.syntax(), SyntaxKind::CONTENT_LINE),
        2,
        "both prose lines must parse as CONTENT_LINEs inside the one flow"
    );
}

#[test]
fn a_tag_containing_a_balanced_alternation_brace_does_not_end_early() {
    // Same defect, alternation-shaped brace instead of interpolation.
    let src = "flow f() {\n  Hello #tag {gold|silver} coins.\n  The river bends.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::FLOW_DECL), 1);
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::CONTENT_LINE), 2);
}

#[test]
fn a_tag_immediately_followed_by_the_enclosing_blocks_own_closer_still_stops_there() {
    // Guard against over-correcting: with no `{` opened inside the tag's
    // own text, depth stays zero and the very first `}` — here the flow
    // body's own closer — must still terminate the tag, exactly as
    // before this fix.
    let src = "flow f() { Hello #tag }\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::FLOW_DECL), 1);
}

#[test]
fn an_unbalanced_open_brace_in_a_tag_eats_the_enclosing_blocks_own_closer() {
    // Review of #1728: the real tradeoff, not "no regression". A raw,
    // unescaped `{` left open inside a tag (no matching `}` before the
    // enclosing block's own closer) is depth-balanced the same as a
    // matched one — the scan can't tell "unbalanced" from "matches the
    // closer" without a real grammar. So this now fails to parse: the
    // tag's `{` is counted, the very next `}` is consumed as its match
    // instead of stopping the tag, and the flow body's own closer is
    // gone by the time `NEWLINE`/`EOF` is reached. This is the accepted,
    // inherent mirror-image of the bug the fix resolves — pinned here so
    // it's a documented tradeoff, not a silent regression.
    let src = "flow f() { Hello #tag { }\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "expected the unbalanced `{{` to consume the flow's own closer and error, got: {:?}",
        p.errors()
    );
}

#[test]
fn a_tag_with_an_escaped_open_brace_does_not_swallow_the_enclosing_blocks_own_closer() {
    // Review of #1728: `\{` is the literal-brace escape (#1716/PR #1732),
    // not a metacharacter, so it must not count as a depth-opener the way
    // a raw `{` does — otherwise the escaped brace above would swallow
    // the enclosing flow's own same-line closer exactly like the
    // unbalanced-raw-brace case, converting previously clean source into
    // a parse error. `tag()` excludes an `L_BRACE` immediately preceded
    // by a raw `BACKSLASH` from the depth counter, so this still parses
    // cleanly.
    let src = "flow f() { Hello #tag \\{ }\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::FLOW_DECL), 1);
}

#[test]
fn a_tag_with_an_escaped_backslash_before_a_real_brace_counts_the_brace() {
    // #1852: `\\{` is an escaped backslash (producing literal `\`), followed
    // by a real interpolation-opening brace. The carve-out that excludes
    // `\{` from the depth counter must not fire for `\\{`, because the
    // backslash is itself escaped. `tag()` must look beyond just the
    // immediately preceding raw token to detect that the backslash is not
    // the escaper. Without the fix, the brace is not counted, so the
    // matching `}` ends the tag prematurely, leaving ` coins` as plain text
    // and the flow body's own closer unconsumed — this fails to parse.
    let src = "flow f() { Hello #tag \\\\{ } coins. }\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::FLOW_DECL), 1);
    // Issue #2045: `ast::Tag::text()` strips a recognized escape's
    // backslash via the same greedy left-to-right consumption
    // `markup::escape` uses, so the escaped backslash here collapses to
    // one literal `\` and the real brace that follows is left alone —
    // `\\{ }` materializes as `\{ }`, not the raw `\\{ }`.
    let tag = find_child::<ast::Tag>(
        &p.syntax()
            .descendants()
            .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
            .expect("CONTENT_LINE"),
    )
    .expect("Tag");
    assert_eq!(
        tag.text(),
        "tag \\{ } coins.",
        "an escaped backslash before a real brace collapses to one literal \
         `\\`, matching `markup::escape`'s greedy consumption for ordinary \
         content"
    );
}

#[test]
fn a_tag_with_an_escaped_hash_does_not_end_the_tag_early() {
    // Issue #1738: `#` is one of the four members of the ruled, final
    // inline escape set (§8d.6), but before this fix `tag()` gave `\#` zero
    // escape treatment — a bare `HASH` always ended a tag, unconditionally,
    // even one immediately preceded by a backslash. Contrast with
    // `tags_with_no_space_between_are_two_separate_tag_nodes` (`#a#b`, no
    // backslash — still two sibling tags, unchanged by this fix): here the
    // `#` is escaped, so it stays inside the ONE tag's own text instead of
    // starting a second `TAG` node.
    let p = assert_lossless("Hello #tag \\#not a new tag\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(
        count_node_kind(&p.syntax(), SyntaxKind::TAG),
        1,
        "an escaped `#` must not split the tag in two"
    );
    let tag = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::TAG)
        .expect("TAG");
    assert_eq!(
        tag.text(),
        "#tag \\#not a new tag",
        "the raw CST node's own text is never rewritten — it stays a \
         faithful, lossless copy of the source, backslash and all; \
         `ast::Tag::text()` is the separate, later materialization point \
         that strips a recognized escape's backslash (issue #2045), see \
         `a_tags_text_accessor_strips_a_recognized_escapes_backslash` below"
    );
}

#[test]
fn a_tags_own_unescaped_closing_brace_remains_the_terminator_even_when_preceded_by_a_backslash() {
    // Issue #1883 (item 2), confirmed intentional, not a residual bug:
    // `\{`'s backslash-parity carve-out (#1852) exists *because* `\{` is
    // one of the ruled, final four-character inline escape set (§8d.6) —
    // #1716/PR #1732 ruled `\{` the literal-brace escape, so counting it
    // as a real depth-opener would be the surprising reading. `}` is *not*
    // a member of that set — `\< \{ \# \\` are the only four — so there is
    // no equivalent "`\}` means a literal, non-metacharacter close-brace"
    // ruling to protect. An `R_BRACE` preceded by a `BACKSLASH` is exactly
    // what it looks like: an ordinary backslash character followed by an
    // ordinary, structurally significant `}` — so it keeps ending the tag
    // exactly like an unescaped `}` would, at depth zero. Here `\{gold\}`
    // opens a real depth-counted brace (`\{`, backslash-escaped so NOT
    // counted — matches `a_tag_with_an_escaped_open_brace_does_not_swallow_the_enclosing_blocks_own_closer`),
    // so depth never leaves zero, and the following `\}` stops the tag
    // there — one character earlier than a naive reading of "`\{`/`\}` are
    // a matched escape pair" would predict, leaving the tag's own trailing
    // backslash as the last consumed token and " more. }" as separate
    // top-level content (a stray `}` once the flow's own closer has
    // already been consumed by the tag's premature stop — see the
    // `errors` assertion).
    let src = "flow f() { Hello #tag \\{a\\} more. }\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "expected the tag's own `\\}}` to swallow the flow's closer early \
         and leave a stray top-level `}}`, got: {:?}",
        p.errors()
    );
    let tag = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::TAG)
        .expect("TAG");
    assert_eq!(
        tag.text(),
        "#tag \\{a\\",
        "the tag stops the instant it meets the `}}` from `\\}}`, backslash \
         and all, exactly as it would for an unescaped `}}` at depth zero"
    );
}

#[test]
fn a_tags_text_accessor_strips_a_recognized_escapes_backslash() {
    // Issue #2045: `markup::escape` already strips a *recognized* escape's
    // backslash for ordinary content, but `tag()`'s raw free-text scan gave
    // `\#`/`\{` structural recognition only (#1738/#1852: an escaped `#`/`{`
    // no longer ends the tag/counts toward depth early) without ever
    // stripping the backslash from the tag's own materialized text — the
    // exact divergence #1738's own filing body used as its motivating
    // example (`Hello \# world #a \#b`). `ast::Tag::text()` is the
    // materialization point that now closes it, in parity with
    // `markup::escape`, while the raw CST node above stays untouched.
    let p = assert_lossless("Hello #tag \\#not a new tag\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let tag = find_child::<ast::Tag>(
        &p.syntax()
            .descendants()
            .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
            .expect("CONTENT_LINE"),
    )
    .expect("Tag");
    assert_eq!(
        tag.text(),
        "tag #not a new tag",
        "a recognized `\\#` strips its backslash in the tag's materialized \
         text, same as ordinary content"
    );
}

#[test]
fn a_tags_text_accessor_strips_a_recognized_open_brace_escapes_backslash() {
    // Issue #2045's own scope note: `\{` gets the identical treatment as
    // `\#`, not just the hash case.
    let p = assert_lossless("Hello #tag \\{gold\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let tag = find_child::<ast::Tag>(
        &p.syntax()
            .descendants()
            .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
            .expect("CONTENT_LINE"),
    )
    .expect("Tag");
    assert_eq!(
        tag.text(),
        "tag {gold",
        "a recognized `\\{{` strips its backslash in the tag's materialized \
         text, same as `\\#`"
    );
}

#[test]
fn a_tags_own_unbalanced_brace_does_not_leak_depth_into_a_sibling_tag() {
    // #1787: `tag()`'s `depth` counter is scoped per-TAG, not per-line —
    // ruled correct, not a gap (see `tag()`'s doc comment). `#a {x #b}`
    // (two trailing tags, the only shape this question can arise in, since
    // a content line's tags are always trailing — `content_line`'s own doc
    // comment): tag `a`'s scan is cut short by the `HASH` that starts `b`
    // — unconditionally, before depth is even consulted — so `a`'s
    // in-progress depth of 1 (from its own unmatched `{`) is discarded,
    // never carried into `b`'s scan. `b` starts fresh at depth zero and
    // immediately meets the `}`, stopping there without consuming it, so
    // that brace is left for the enclosing flow body's own closer — this
    // parses with zero errors, exactly like the single-tag sibling test
    // above (`a_tag_immediately_followed_by_the_enclosing_blocks_own_closer_still_stops_there`),
    // not like the per-line-carried-depth reading, which would instead
    // consume that `}` as `a`'s belated match and run off looking for a
    // SECOND `}` to close the flow body, failing to parse (the same
    // "eats the enclosing closer" tradeoff the sibling `an_unbalanced_open_brace_…`
    // test pins for the single-tag case, but reached across a tag boundary
    // instead of within one tag).
    let src = "flow f() { Hello #a {x #b}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::FLOW_DECL), 1);

    let tags: Vec<String> = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::TAG)
        .map(|n| n.text().to_string())
        .collect();
    assert_eq!(tags.len(), 2, "expected two sibling TAG nodes: {tags:?}");
    assert_eq!(
        tags[0], "#a {x",
        "tag `a` keeps its own unmatched `{{` — HASH cut its scan short \
         before depth was ever consulted"
    );
    assert_eq!(
        tags[1], " #b",
        "tag `b` starts at depth zero and stops at the very first `}}` — \
         it never inherits `a`'s leftover unmatched depth"
    );
}

#[test]
fn a_top_level_tag_with_an_embedded_brace_reproduces_with_no_flow_or_tag_guard_involved() {
    // #1728: the defect is in the free-text scan itself, not anything
    // specific to being inside a flow body or interacting with the
    // header/tag guard (`decl::header_tags_precede_a_body`) — it
    // reproduces identically for a bare top-level content line.
    let src = "Hello #tag {gold} coins.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::TAG), 1);
}

#[test]
fn tag_on_a_labeled_content_line() {
    // G-1 (label) and tags composed on the same line.
    let p = assert_lossless("(start) Hello #tag\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::LABEL));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::TAG));
}

// ── Section E: comments interleaved with content ────────────────────────

#[test]
fn comment_only_line_before_content_is_excluded_from_the_content_line() {
    // A `//`-comment on its own line is pure trivia at the body-item loop
    // level — `skip_ws()` consumes it before `content_line` ever starts,
    // so it never becomes a `CONTENT_LINE` child. This is the baseline the
    // two same-line-trailing-comment cases below diverge from, per
    // `text_run_until`'s documented literal-prose contract.
    let src = "flow f() {\n  // just a comment\n  Hello\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    assert_eq!(text_run_concat(&line), "Hello");
}

#[test]
fn line_comment_mid_text_run_is_literal_prose_per_text_run_untils_contract() {
    // RULED by #1638 (2026-08-01), working as intended — NOT an open
    // question: "keep trailing comments literal in prose. Prose is prose —
    // everything on a prose line is text, exactly as `text_run_until`
    // already documents … The lint variant was offered and explicitly not
    // chosen. Current behavior is correct." So a `//` comment appearing
    // after prose on the same content line is folded into the enclosing
    // `TEXT` node as literal characters, per `text_run_until`'s documented
    // contract ("including any interior whitespace/plain-comments — those
    // are literal prose here, not trivia to discard"), deliberately
    // contrasted with doc-comment tokens, which DO break the run. Note
    // the surface split: the INK-compat parser ELIDES mid-line comments
    // (#2366/#2960/#2976) — that is that surface's own semantics, not a
    // model for this one. Spec home: docs/prose-dialect-spec.md
    // ("Trailing comments in prose (RULED)"). This test pins the ruled
    // behavior.
    let src = "flow f() {\n  Hello // not actually a comment here\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    assert_eq!(
        text_run_concat(&line),
        "Hello // not actually a comment here",
        "current, contract-documented behavior: the // comment's text is part of TEXT"
    );
}

#[test]
fn block_comment_mid_text_run_is_literal_prose_per_text_run_untils_contract() {
    // Same ruled behavior as the line-comment case above (#1638, WAI),
    // for `/* … */`.
    let src = "flow f() {\n  Hello /* aside */ world\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    assert_eq!(
        text_run_concat(&line),
        "Hello /* aside */ world",
        "current, contract-documented behavior: the block comment's text is part of TEXT"
    );
}

#[test]
fn unterminated_block_comment_mid_text_run_never_panics_and_roundtrips() {
    // An unterminated `/*` runs to EOF (`BLOCK_COMMENT`'s own doc comment
    // in `syntax_kind.rs`) — folded into the same TEXT run as the
    // preceding case above, but the key property here is just "doesn't
    // panic/hang, stays lossless", independent of the open design
    // question there.
    let src = "Hello /* never closed\n";
    let p = assert_lossless(src);
    let _ = p.errors();
}

// ── Section F: error recovery — malformed interpolation ────────────────

#[test]
fn unterminated_interpolation_missing_closing_brace_before_newline() {
    let src = "Hello {name\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "expected a parse error for the missing `}}`"
    );
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
}

#[test]
fn unterminated_interpolation_runs_to_eof_with_no_newline_at_all() {
    let src = "Hello {name";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "expected a parse error for the missing `}}`"
    );
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
}

#[test]
fn empty_interpolation_body_errors_but_recovers() {
    let src = "Hello {}\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "expected a parse error for the missing expression"
    );
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
}

#[test]
fn stray_closing_brace_in_content_recovers_without_panicking() {
    // `content_line`'s own doc comment: a bare `R_BRACE` is never consumed
    // by the content line itself. At top level (no enclosing body) that
    // leaves it genuinely stray — an `ERROR` node, not a panic, and the
    // rest of the line keeps parsing.
    let src = "Hello } world\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ERROR));
    // Both text runs on either side of the stray brace still show up.
    assert_eq!(text_run_concat(&p.syntax()), "Hello world");
}

#[test]
fn leading_stray_closing_brace_recovers_without_panicking() {
    let src = "} Hello\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ERROR));
    assert_eq!(text_run_concat(&p.syntax()), "Hello");
}

#[test]
fn adversarial_brace_soup_never_panics_and_roundtrips() {
    for src in [
        "{{{}}}\n",
        "{}{}{}\n",
        "{{{{{{{{{{\n",
        "}}}}}}}}}}\n",
        "{expr}{expr2}{\n",
    ] {
        let p = assert_lossless(src);
        let _ = p.errors();
    }
}

#[test]
fn backslash_escapes_hash_no_tag_opens() {
    // The escape set landed (§8d.6, issue #1716 — `parser::markup::escape`):
    // `\#` is now a real `ESCAPE` node producing a literal `#`, and no
    // `TAG` opens. This test used to lock in the pre-design "no escape
    // mechanism yet" behavior (`\#` opening a real `TAG`); the escape set
    // is final now, so it locks in the opposite.
    let src = "\\# not a tag\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::TAG));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ESCAPE));
}

#[test]
fn backslash_escapes_open_brace_no_interpolation_opens() {
    // Same landing as above, for `{` (§8d.6): `\{` is a literal `{`, and
    // no `INTERPOLATION` opens (so "not logic" is ordinary trailing prose,
    // not a parse error).
    let src = "\\{ not logic\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ESCAPE));
}

// ── Section G: negative space — lines that are NOT a CONTENT_LINE ──────

#[test]
fn blank_line_produces_no_content_line() {
    let p = assert_lossless("\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::CONTENT_LINE));
}

#[test]
fn bare_divert_line_produces_no_content_line_wrapper() {
    // Unlike the brink-syntax parity target (where a bare divert still
    // nests inside an empty-of-MIXED_CONTENT `CONTENT_LINE`), native's
    // `body_line` dispatches `DIVERT` straight to `divert_or_tunnel`
    // (`block.rs`) — the `DIVERT_STMT` is a direct body-item sibling, with
    // no `CONTENT_LINE` wrapper at all.
    let p = assert_lossless("-> knot\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::DIVERT_STMT));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::CONTENT_LINE));
}

#[test]
fn choice_point_line_is_not_a_content_line() {
    let src = "flow f() {\n  {?\n    * Hello\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CHOICE_POINT));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::CONTENT_LINE));
}

#[test]
fn return_line_is_not_a_content_line() {
    let src = "flow f() {\n  return\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::CONTENT_LINE));
}

// ── Section H: G-2 disambiguation — choice-line trailing brace ─────────
//
// `is_body_open_brace` (`content.rs`) is content family's own dispatch
// code, even though the broader choice grammar lives in `choice.rs`.

#[test]
fn trailing_brace_expr_on_choice_line_is_interpolation_not_choice_body() {
    let src = "flow f() {\n  {?\n    * hello {x}\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::CHOICE_BODY));
}

#[test]
fn multiline_brace_on_choice_line_is_choice_body_not_interpolation() {
    let src = "flow f() {\n  {?\n    * hello {\n      inner\n    }\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CHOICE_BODY));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
}

// ── Section I: proptest round-trip generators ───────────────────────────
//
// `tests/proptest_native.rs`'s `arb_text()` deliberately excludes every
// structural character (including `<`/`>`, so `arb_content_line()` never
// generates glue). These generators stay file-local rather than extending
// `arb_text()` itself: it's consumed by many sibling properties beyond
// content (`arb_content_line`, `arb_interpolation_line`, the choice/label/
// divert generators, and the unicode-noise property all build on it), so
// injecting `<>` into its output would perturb every one of those
// properties, not just the content family's. Self-contained generators
// scoped to this file cover the gap instead: glue chains and multi-tag
// lines.

use proptest::prelude::*;

fn arb_word() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9]{0,6}"
}

/// A content line built from 1-4 words joined by `<>` glue, e.g.
/// `"word1<>word2<>word3\n"` — exercises glue-chain parsing the shared
/// generator's structural-character exclusion never reaches.
fn arb_glue_chain_line() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_word(), 1..=4).prop_map(|words| format!("{}\n", words.join("<>")))
}

/// A content line with 1-3 trailing `#tag` words.
fn arb_tagged_content_line() -> impl Strategy<Value = String> {
    (arb_word(), prop::collection::vec(arb_word(), 1..=3)).prop_map(|(text, tags)| {
        let mut line = text;
        for tag in &tags {
            line.push_str(" #");
            line.push_str(tag);
        }
        line.push('\n');
        line
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn glue_chain_line_roundtrips(input in arb_glue_chain_line()) {
        let p = parse(&input);
        prop_assert_eq!(p.syntax().text().to_string(), input);
    }

    #[test]
    fn glue_chain_line_has_no_errors(input in arb_glue_chain_line()) {
        let p = parse(&input);
        prop_assert!(p.errors().is_empty(), "input: {:?}\nerrors: {:?}", input, p.errors());
    }

    #[test]
    fn glue_chain_line_glue_node_count_matches_separator_count(input in arb_glue_chain_line()) {
        let p = parse(&input);
        let expected = input.matches("<>").count();
        prop_assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::GLUE_NODE), expected);
    }

    #[test]
    fn tagged_content_line_roundtrips(input in arb_tagged_content_line()) {
        let p = parse(&input);
        prop_assert_eq!(p.syntax().text().to_string(), input);
    }

    #[test]
    fn tagged_content_line_has_no_errors(input in arb_tagged_content_line()) {
        let p = parse(&input);
        prop_assert!(p.errors().is_empty(), "input: {:?}\nerrors: {:?}", input, p.errors());
    }

    #[test]
    fn tagged_content_line_tag_count_matches_hash_count(input in arb_tagged_content_line()) {
        let p = parse(&input);
        let expected = input.matches('#').count();
        prop_assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::TAG), expected);
    }
}
