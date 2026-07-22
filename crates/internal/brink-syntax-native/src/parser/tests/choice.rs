//! Choice anatomy — choice points, bracket display, guards, interpolation.
//! Family for #1195.

use super::*;

#[test]
fn choice_point_parses() {
    let src = "flow garden() {\n  {?\n    * [Look] You look around.\n    + (again) [Look again] Still a garden.\n    else { Nothing left to do. }\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

// ── Significant inter-token whitespace in content position ───────────────

#[test]
fn space_after_choice_bracket_close_survives_inside_the_text_node() {
    // `* text[] more` — the display-split anatomy. The space right after the
    // `]` bracket close is the leading char of `CHOICE_INNER_CONTENT` and
    // must be preserved AS PROSE (folded into the inner `TEXT` node), not
    // eaten as bare trivia. Regression for the `'A wager!'I returned.`
    // (native) vs `'A wager!' I returned.` (oracle) divergence.
    let src = "flow f() {\n  {?\n    * 'A wager!'[] I returned.\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());

    let inner = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_INNER_CONTENT)
        .expect("CHOICE_INNER_CONTENT");
    let inner_text = text_run_concat(&inner);
    assert_eq!(
        inner_text, " I returned.",
        "space after `]` must be folded into the inner TEXT node"
    );

    // The reconstructed choice display (start-content + inner-content, the
    // bracket content being choice-only) keeps the separating space.
    let choice = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE)
        .expect("CHOICE");
    let start = choice
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_START_CONTENT)
        .expect("CHOICE_START_CONTENT");
    let display = format!("{}{}", text_run_concat(&start), inner_text);
    assert_eq!(display, "'A wager!' I returned.");
}

#[test]
fn charter_wager_shape_preserves_space_after_bracket_close() {
    // The charter's literal spelling `* 'A wager!'[] I returned. { … }`
    // (complex-flow-v1) — the bracket-close space with a trailing
    // nested-content CHOICE_BODY brace also present, exercising the G-2
    // body-open disambiguation alongside the whitespace fix.
    let src = concat!(
        "flow f() {\n",
        "  {?\n",
        "    * 'A wager!'[] I returned. {\n",
        "      -> f\n",
        "    }\n",
        "  }\n",
        "}\n",
    );
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());

    let inner = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_INNER_CONTENT)
        .expect("CHOICE_INNER_CONTENT");
    assert_eq!(text_run_concat(&inner), " I returned. ");
    // The trailing brace still opened a real CHOICE_BODY (not interpolation).
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CHOICE_BODY));
}

#[test]
fn whitespace_only_inner_content_makes_no_spurious_text_node() {
    // The complement: whitespace that is NOT followed by prose (a bracket
    // close then only trailing spaces before the newline) must still be bare
    // trivia — the fix must not manufacture an empty/whitespace-only `TEXT`
    // node. `starts_text_run` returns `false` when the next non-trivia token
    // is a stop token (here `NEWLINE`), so the loop `skip_ws`es and breaks.
    let src = "flow f() {\n  {?\n    * choose[] \n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let inner = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_INNER_CONTENT)
        .expect("CHOICE_INNER_CONTENT");
    assert!(
        !has_node_kind(&inner, SyntaxKind::TEXT),
        "whitespace-only inner content must not create a TEXT node, tree: {inner:#?}"
    );
}

// ── G-2: choice-line `{expr}` interpolation ──────────────────────────

#[test]
fn choice_line_interpolation_before_bracket_parses_as_interpolation() {
    // `* Gold: {gold}` — from the README's G-2 finding: the `{` used to be
    // swallowed as a premature CHOICE_BODY open.
    let src = "flow f() {\n  {?\n    * Gold: {gold}\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let choice = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE)
        .expect("CHOICE");
    assert!(has_node_kind(&choice, SyntaxKind::INTERPOLATION));
    // And no CHOICE_BODY was spuriously opened — this choice has no
    // nested-content braces at all.
    assert!(!has_node_kind(&choice, SyntaxKind::CHOICE_BODY));
}

#[test]
fn choice_line_interpolation_inside_bracket_inner_content_parses() {
    let src = "flow f() {\n  {?\n    * [Buy] You have {gold} left.\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let inner = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_INNER_CONTENT)
        .expect("CHOICE_INNER_CONTENT");
    assert!(has_node_kind(&inner, SyntaxKind::INTERPOLATION));
}

#[test]
fn choice_body_still_opens_as_a_body_not_interpolation() {
    // The flip side: a genuine multiline CHOICE_BODY brace must still be
    // recognized as CHOICE_BODY, not mis-swallowed as a (garbage)
    // interpolation expression now that plain `{` no longer stops early.
    let src = "flow f() {\n  {?\n    * [Eat] {\n      You eat. -> f\n    }\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CHOICE_BODY));
}

#[test]
fn choice_line_conditional_guard_and_trailing_interpolation_coexist() {
    // Guard braces (handled before `choice_text` even runs) plus a trailing
    // interpolation in the same choice line.
    let src = "flow f() {\n  {?\n    * {if hp > 0} Gold: {gold}\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let choice = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE)
        .expect("CHOICE");
    assert!(has_node_kind(&choice, SyntaxKind::CHOICE_GUARD));
    assert!(has_node_kind(&choice, SyntaxKind::INTERPOLATION));
}
