//! Choice anatomy — choice points, bracket display, guards, interpolation.
//! Family for #1195.
//!
//! Parity target: `brink-syntax/src/parser/tests/choice/mod.rs`. That
//! grammar's `double_plus_choice`/`triple_plus_choice`/
//! `double_plus_choice_in_knot` tests exercise ink's stacked-bullet nesting
//! depth (`**`/`+++`), which reads as ink's implicit gather-based nesting
//! (charter §5). The native grammar has no such concept: `choice()` bumps
//! exactly one `STAR`/`PLUS` bullet token per `CHOICE`, and a second
//! `*`/`+` immediately following just becomes ordinary `CHOICE_START_CONTENT`
//! prose (see `stacked_bullets_do_not_open_a_second_choice`
//! below) — nesting is instead expressed structurally, a `CHOICE_BODY`
//! containing another `{? … }` (see
//! `choice_body_may_nest_another_choice_point`). So those three parity
//! tests have no native analogue; every other shape in the parity file is
//! covered here.

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

// ── CHOICE_BULLET: each kind standalone, and mixed within one point ──────

#[test]
fn star_bullet_choice_standalone() {
    let src = "flow f() {\n  {?\n    * Choice text\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let bullet = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_BULLET)
        .expect("CHOICE_BULLET");
    assert_eq!(bullet.text().to_string(), "*");
}

#[test]
fn plus_bullet_choice_standalone() {
    let src = "flow f() {\n  {?\n    + Choice text\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let bullet = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_BULLET)
        .expect("CHOICE_BULLET");
    assert_eq!(bullet.text().to_string(), "+");
}

#[test]
fn star_and_plus_bullets_mixed_within_one_choice_point() {
    let src = "flow f() {\n  {?\n    * a\n    + b\n    * c\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::CHOICE), 3);
    let bullets: Vec<String> = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CHOICE_BULLET)
        .map(|n| n.text().to_string())
        .collect();
    assert_eq!(bullets, vec!["*", "+", "*"]);
}

#[test]
fn stacked_bullets_do_not_open_a_second_choice() {
    // The native-grammar complement to brink-syntax's `double_plus_choice`:
    // a second `+` immediately stacked on the first (`++`, no star
    // involved — the actual mixing of `*` and `+` within one point is
    // covered separately by `star_and_plus_bullets_mixed_within_one_choice_point`
    // above) is NOT a deeper nesting level here (there is no gather-based
    // nesting concept, charter §5 / this file's module doc) — it is
    // ordinary `CHOICE_START_CONTENT` prose, folded into the one
    // `CHOICE`'s `TEXT`.
    let src = "flow f() {\n  {?\n    ++[text] inner\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(
        count_node_kind(&p.syntax(), SyntaxKind::CHOICE),
        1,
        "the second `+` must not open a second CHOICE node"
    );
    let start = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_START_CONTENT)
        .expect("CHOICE_START_CONTENT");
    assert_eq!(text_run_concat(&start), "+");
}

// ── LABEL + CHOICE_GUARD ──────────────────────────────────────────────

#[test]
fn choice_label_alone_produces_a_label_node() {
    let src = "flow f() {\n  {?\n    * (myLabel) Choice text\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let label = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::LABEL)
        .expect("LABEL");
    assert_eq!(label.text().to_string(), "(myLabel)");
}

#[test]
fn choice_guard_alone_produces_a_guard_node() {
    // Unlike brink-syntax's bare `{x > 5}` condition shape, the native
    // grammar's `CHOICE_GUARD` always requires the literal `if` keyword
    // (`choice_guard`'s `p.expect(KW_IF)`) — `{x > 5}` alone (no `if`)
    // would not even be recognized as a guard at all (the dispatch check
    // is `p.at(L_BRACE) && p.nth(1) == KW_IF`).
    let src = "flow f() {\n  {?\n    * {if x > 5} Choice text\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CHOICE_GUARD));
}

#[test]
fn choice_guard_and_label_combine_in_the_documented_order() {
    // `choice.rs`'s own doc comment on `choice()`: "bullet, optional
    // `{if cond}` guard, optional `(label)`" — guard THEN label, both
    // present together.
    let src = "flow f() {\n  {?\n    * {if visited} (again) Been here.\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let choice = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE)
        .expect("CHOICE");
    let guard = choice
        .children()
        .find(|n| n.kind() == SyntaxKind::CHOICE_GUARD)
        .expect("CHOICE_GUARD");
    let label = choice
        .children()
        .find(|n| n.kind() == SyntaxKind::LABEL)
        .expect("LABEL");
    assert!(
        guard.text_range().start() < label.text_range().start(),
        "guard must precede label in child order"
    );
}

#[test]
fn label_before_guard_is_currently_not_recognized_as_a_choice_guard() {
    // GAP, not a ruling: `choice()` only checks for a guard BEFORE
    // consuming a label, never after (`choice.rs`'s own doc comment
    // reflects this implementation order, but the charter itself
    // (native-surface-charter.md §6, §11) gives `{if cond}` and `(name)`
    // as separate exhibits and is silent on their combined order).
    // brink-syntax's reference grammar — the ink-parity source of truth —
    // takes label FIRST, then condition(s)
    // (`brink-syntax/src/parser/choice.rs`'s `choice()`: `label?` before
    // `choice_condition*`), i.e. `* (name) {cond} text` is the canonical
    // ink spelling. The native parser currently rejects that spelling: a
    // `{if cond}` following a label is reparsed from scratch by the
    // generic content scanner, which recognizes it as a bare inline
    // `CONDITIONAL_BLOCK` (the annotated-brace family, charter §6)
    // instead of a `CHOICE_GUARD` — and since that shorthand form has no
    // `:`/`{` body opener here, it errors. Asserting the CURRENT (buggy)
    // behavior below, characterized as a gap, not a documented ruling —
    // see #1253 for the tracking issue on this ordering divergence.
    let src = "flow f() {\n  {?\n    * (again) {if visited} Been here.\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "the reversed order is expected to produce an error: {:?}",
        p.errors()
    );
    let choice = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE)
        .expect("CHOICE");
    assert!(!has_node_kind(&choice, SyntaxKind::CHOICE_GUARD));
    assert!(has_node_kind(&choice, SyntaxKind::CONDITIONAL_BLOCK));
}

// ── Bracket-split anatomy: isolated, and combined with tags/diverts ──────

#[test]
fn three_part_bracket_split_anatomy_in_isolation() {
    // `text[bracket]inner` — the exact three-node split, no other anatomy.
    let src = "flow f() {\n  {?\n    * Start[middle]end\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let choice = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE)
        .expect("CHOICE");
    let start = choice
        .children()
        .find(|n| n.kind() == SyntaxKind::CHOICE_START_CONTENT)
        .expect("CHOICE_START_CONTENT");
    let bracket = choice
        .children()
        .find(|n| n.kind() == SyntaxKind::CHOICE_BRACKET_CONTENT)
        .expect("CHOICE_BRACKET_CONTENT");
    let inner = choice
        .children()
        .find(|n| n.kind() == SyntaxKind::CHOICE_INNER_CONTENT)
        .expect("CHOICE_INNER_CONTENT");
    assert_eq!(text_run_concat(&start), "Start");
    assert_eq!(text_run_concat(&bracket), "middle");
    assert_eq!(text_run_concat(&inner), "end");
}

#[test]
fn bracket_only_choice_with_no_start_or_inner_text() {
    // `[hidden] shown` — an empty `CHOICE_START_CONTENT` before the
    // bracket (parity: brink-syntax's `choice_with_bracket`).
    let src = "flow f() {\n  {?\n    * [hidden] shown\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let choice = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE)
        .expect("CHOICE");
    let start = choice
        .children()
        .find(|n| n.kind() == SyntaxKind::CHOICE_START_CONTENT)
        .expect("CHOICE_START_CONTENT");
    assert_eq!(text_run_concat(&start), "");
    let bracket = choice
        .children()
        .find(|n| n.kind() == SyntaxKind::CHOICE_BRACKET_CONTENT)
        .expect("CHOICE_BRACKET_CONTENT");
    assert_eq!(text_run_concat(&bracket), "hidden");
}

#[test]
fn choice_text_followed_by_divert_without_a_bracket() {
    // Parity: brink-syntax's `choice_with_divert` (`* Choice -> knot`) —
    // no bracket-split anatomy at all, just prose then a bare divert.
    let src = "flow f() {\n  {?\n    * Choice -> knot\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let choice = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE)
        .expect("CHOICE");
    assert!(has_node_kind(&choice, SyntaxKind::DIVERT_STMT));
    assert!(
        !has_node_kind(&choice, SyntaxKind::CHOICE_BRACKET_CONTENT),
        "no `[` was ever written, so no bracket anatomy should appear"
    );
}

#[test]
fn bracket_split_anatomy_combined_with_a_divert_in_the_bracket_region() {
    // The bracket region itself (not just the inner region, already
    // covered by `divert.rs`) can carry a divert.
    let src = "flow f() {\n  {?\n    * Go[-> elsewhere]stays\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let bracket = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_BRACKET_CONTENT)
        .expect("CHOICE_BRACKET_CONTENT");
    assert!(has_node_kind(&bracket, SyntaxKind::DIVERT_STMT));
}

// ── Trailing `#tag` on a choice line ───────────────────────────────────────
//
// #1264 (fixes #1252/#1195): unlike `content_line` (`content.rs`), which
// calls `tag_line_tail` after its `content_items_until` scan to fold
// trailing `#tag`s into the `CONTENT_LINE`, `choice_text` (`choice.rs`)
// never did — `content_items_until`'s loop unconditionally breaks on `HASH`
// (see `content.rs`'s `content_items_until` doc comment), but `choice()`
// had no follow-up call to consume it, so the `#tag` fell to the enclosing
// `choice_point` loop's `error_recover` arm and got wrapped in `ERROR`
// nodes token-by-token. `choice()` now mirrors `content_line`'s tail call,
// matching brink-syntax's `choice_with_tags` parity test.
#[test]
fn choice_line_trailing_tag_is_recognized_as_a_tag_node() {
    let src = "flow f() {\n  {?\n    * Choice #tag1\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(
        has_node_kind(&p.syntax(), SyntaxKind::TAG),
        "the `#tag1` should be a real TAG node"
    );
    assert!(
        !has_node_kind(&p.syntax(), SyntaxKind::ERROR),
        "the `#tag1` must no longer be wrapped in ERROR nodes"
    );
    let choice = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE)
        .expect("CHOICE");
    assert!(
        has_node_kind(&choice, SyntaxKind::TAG),
        "the TAG node should attach to the CHOICE the tag trails"
    );
}

// ── CHOICE_BODY: braced nested-content form, including nested choices ────

#[test]
fn choice_body_multiline_multi_region_content() {
    let src = concat!(
        "flow f() {\n",
        "  {?\n",
        "    * [Eat] {\n",
        "      You eat a donut.\n",
        "      Delicious.\n",
        "    }\n",
        "  }\n",
        "}\n",
    );
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let body = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_BODY)
        .expect("CHOICE_BODY");
    assert_eq!(count_node_kind(&body, SyntaxKind::CONTENT_LINE), 2);
}

#[test]
fn choice_body_may_nest_another_choice_point() {
    // The native equivalent of brink-syntax's `nested_choice`: since there
    // is no gather-based stacking (this file's module doc), nesting is
    // spelled structurally — a `CHOICE_BODY` containing its own `{? … }`.
    let src = concat!(
        "flow f() {\n",
        "  {?\n",
        "    * outer {\n",
        "      {?\n",
        "        * inner\n",
        "      }\n",
        "    }\n",
        "  }\n",
        "}\n",
    );
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::CHOICE_POINT), 2);
    let outer_body = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_BODY)
        .expect("CHOICE_BODY");
    assert!(has_node_kind(&outer_body, SyntaxKind::CHOICE_POINT));
}

// ── ELSE_BRANCH: present and absent ───────────────────────────────────

#[test]
fn else_branch_present_provides_a_fallback() {
    let src = "flow f() {\n  {?\n    * a\n    else { Nothing left. }\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let point = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_POINT)
        .expect("CHOICE_POINT");
    let else_branch = point
        .children()
        .find(|n| n.kind() == SyntaxKind::ELSE_BRANCH)
        .expect("ELSE_BRANCH");
    assert!(has_node_kind(&else_branch, SyntaxKind::CHOICE_BODY));
}

#[test]
fn else_branch_absent_is_not_required() {
    let src = "flow f() {\n  {?\n    * a\n    * b\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::ELSE_BRANCH));
}

#[test]
fn else_not_immediately_followed_by_a_brace_is_not_treated_as_an_else_branch_attempt() {
    // `else` always requires a braced body (choice.rs's doc comment: "no
    // colon form, unlike IF_ARM/ELSE_BRANCH in the conditional family").
    // The `choice_point` loop's own dispatch guard is
    // `KW_ELSE if p.nth(1) == L_BRACE` — `else` NOT immediately followed
    // by `{` (trivia aside) isn't even recognized as an else-branch
    // attempt; it falls through to the loop's generic recovery arm
    // instead (one `ERROR`-wrapped token at a time), same as any other
    // unexpected token. `else_branch`'s own "expected `{` after a choice
    // point's `else`" error path is therefore unreachable through this
    // call site by construction — dispatch already guarantees `{` follows
    // whenever `else_branch` runs.
    let src = "flow f() {\n  {?\n    * a\n    else nope\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
    assert!(
        !has_node_kind(&p.syntax(), SyntaxKind::ELSE_BRANCH),
        "no ELSE_BRANCH should be attempted when `else` isn't followed by `{{`"
    );
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ERROR));
}

// ── SPLICE: `<- flow(args)` in choice context ─────────────────────────

#[test]
fn splice_with_arguments_parses() {
    let src = "flow f() {\n  {?\n    <- options(1, 2)\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let splice = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SPLICE)
        .expect("SPLICE");
    assert!(has_node_kind(&splice, SyntaxKind::ARG_LIST));
}

#[test]
fn splice_without_arguments_parses() {
    let src = "flow f() {\n  {?\n    <- options\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let splice = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SPLICE)
        .expect("SPLICE");
    assert!(!has_node_kind(&splice, SyntaxKind::ARG_LIST));
}

#[test]
fn multiple_splices_mixed_with_choices_in_one_point() {
    let src = "flow f() {\n  {?\n    * a\n    <- shared_options()\n    + b\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::SPLICE), 1);
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::CHOICE), 2);
}

#[test]
fn splice_missing_a_target_path_does_not_panic() {
    let src = "flow f() {\n  {?\n    <-\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::SPLICE));
}

// ── Error recovery ─────────────────────────────────────────────────────

#[test]
fn choice_point_with_no_choices_parses_with_no_errors() {
    // An empty `{? … }` is not itself malformed — no choice lines is a
    // legal (if useless) choice point, not an error-recovery case.
    let src = "flow f() {\n  {?\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CHOICE_POINT));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::CHOICE));
}

#[test]
fn unclosed_choice_point_brace_recovers_without_panicking() {
    let src = "flow f() {\n  {?\n    * a\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CHOICE_POINT));
}

#[test]
fn bullet_with_no_content_parses_with_an_empty_start_content() {
    // `*` alone on a line: no error — an empty `CHOICE_START_CONTENT` is
    // valid, not a recovery case.
    let src = "flow f() {\n  {?\n    *\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let start = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_START_CONTENT)
        .expect("CHOICE_START_CONTENT");
    assert_eq!(text_run_concat(&start), "");
}

#[test]
fn unclosed_bracket_recovers_without_panicking() {
    let src = "flow f() {\n  {?\n    * [text\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
    assert!(has_node_kind(
        &p.syntax(),
        SyntaxKind::CHOICE_BRACKET_CONTENT
    ));
}

#[test]
fn malformed_guard_missing_condition_does_not_panic() {
    let src = "flow f() {\n  {?\n    * {if} text\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CHOICE_GUARD));
}

#[test]
fn malformed_label_non_ident_falls_back_to_prose_without_panicking() {
    // `label()` requires `L_PAREN IDENT R_PAREN`; a non-`IDENT` (a bare
    // integer here) doesn't satisfy `expect(IDENT)`, so the `LABEL` node
    // ends up containing only the `L_PAREN`, and the rest becomes ordinary
    // choice text — no panic, no infinite loop (the `(` was still real
    // forward progress).
    let src = "flow f() {\n  {?\n    * (1) text\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
    let label = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::LABEL)
        .expect("LABEL");
    assert!(!has_node_kind(&label, SyntaxKind::IDENT));
}

#[test]
fn garbage_inside_choice_point_recovers_token_by_token() {
    // A token that is none of `STAR`/`PLUS`/`THREAD`/`KW_ELSE`/`R_BRACE`
    // (here a bare `=`) hits the `choice_point` loop's `_` arm, gets
    // wrapped in one `ERROR` node per token, and the loop keeps making
    // forward progress rather than spinning or panicking.
    let src = "flow f() {\n  {?\n    = = =\n    * a\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ERROR));
    // Recovery didn't eat the whole rest of the point: the trailing real
    // choice line still parses as a CHOICE.
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CHOICE));
}

// ── Adversarial ─────────────────────────────────────────────────────────

#[test]
fn extremely_deep_nested_choice_points_do_not_overflow_the_stack() {
    // Pathological nesting: a `CHOICE_POINT` inside a `CHOICE_BODY` inside
    // the previous `CHOICE`, repeated well past `MAX_DEPTH` (256,
    // `parser/mod.rs`). Each level increments the shared depth counter
    // twice (`choice_point`'s own `enter_depth`, plus
    // `braced_item_list`'s for the `CHOICE_BODY`), so this must trip the
    // guard — CLAUDE.md's "guard against unbounded growth" — well before
    // Rust's own recursion limit, and recover without panicking or hanging
    // (the VM/parser step-limit rule applies here too).
    const LEVELS: usize = 150;
    let mut src = String::new();
    for _ in 0..LEVELS {
        src.push_str("{?\n* a {\n");
    }
    src.push_str("* leaf\n");
    for _ in 0..LEVELS {
        src.push_str("}\n}\n");
    }
    let wrapped = format!("flow f() {{\n{src}}}\n");
    // Lossless round-trip must hold even under the depth guard's
    // best-effort recovery path.
    let p = assert_lossless(&wrapped);
    assert!(
        p.errors()
            .iter()
            .any(|e| e.message.contains("maximum nesting depth exceeded")),
        "expected the depth guard to trip: {:?}",
        p.errors()
    );
}

#[test]
fn adversarial_structural_soup_inside_a_choice_point_never_panics() {
    // Every choice-family structural character in one line, malformed:
    // unmatched brackets/braces/parens, a bare `<-`, a bare `?`.
    let src = "flow f() {\n  {?\n    *(){}[]<-?{if}else\n  }\n}\n";
    // No specific assertion on error count/shape — the invariant under
    // test is solely "does not panic, does not hang, stays lossless"
    // (`assert_lossless` already checks the round-trip).
    assert_lossless(src);
}

// ── Local proptest generators (per this issue's own instruction: a
// family-owned generator, not a change to the shared `tests/proptest_native.rs`,
// which #1199 owns this wave) ──────────────────────────────────────────

mod choice_proptests {
    use proptest::prelude::*;

    use super::{SyntaxKind, has_node_kind, parse};

    /// Mirrors `tests/proptest_native.rs`'s own `arb_ident` keyword filter
    /// (studied for this local generator's style, per this issue's "put it
    /// in your own family file" instruction): a bare keyword tokenizes as
    /// its `KW_*` kind, not `IDENT`, which would make `label()`'s
    /// `expect(IDENT)` or `expression()`'s atom parsing fail — breaking the
    /// "well-formed input round-trips with zero errors" property below for
    /// reasons unrelated to what this generator is testing.
    const KEYWORDS: &[&str] = &[
        "flow", "fn", "var", "const", "let", "flags", "struct", "extern", "import", "use",
        "module", "return", "ref", "if", "match", "else", "as", "in", "true", "false", "END", "DONE",
    ];

    fn arb_ident() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9_]{0,6}"
            .prop_filter("must not be a keyword", |s| !KEYWORDS.contains(&s.as_str()))
    }

    fn arb_bullet() -> impl Strategy<Value = &'static str> {
        prop_oneof![Just("*"), Just("+")]
    }

    /// A choice line combining every optional piece this family owns:
    /// bullet, guard, label, bracket-split anatomy — in the grammar's
    /// documented order (guard, then label).
    fn arb_full_choice_line() -> impl Strategy<Value = String> {
        (
            arb_bullet(),
            proptest::option::of(arb_ident()),
            proptest::option::of(arb_ident()),
            arb_ident(),
            proptest::option::of(arb_ident()),
        )
            .prop_map(|(bullet, guard_cond, label, start, bracket)| {
                use std::fmt::Write as _;

                let mut line = bullet.to_string();
                if let Some(cond) = guard_cond {
                    let _ = write!(line, " {{if {cond}}}");
                }
                if let Some(name) = label {
                    let _ = write!(line, " ({name})");
                }
                line.push(' ');
                line.push_str(&start);
                if let Some(inner) = bracket {
                    let _ = write!(line, "[{inner}]");
                }
                line.push('\n');
                line
            })
    }

    fn arb_full_choice_point() -> impl Strategy<Value = String> {
        prop::collection::vec(arb_full_choice_line(), 1..=4)
            .prop_map(|lines| format!("{{?\n{}}}\n", lines.join("")))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn full_choice_point_roundtrips(point in arb_full_choice_point()) {
            let src = format!("flow f() {{\n{point}}}\n");
            let parsed = parse(&src);
            prop_assert_eq!(parsed.syntax().text().to_string(), src);
        }

        #[test]
        fn full_choice_point_never_panics_and_produces_a_choice_point_node(point in arb_full_choice_point()) {
            let src = format!("flow f() {{\n{point}}}\n");
            let parsed = parse(&src);
            prop_assert!(has_node_kind(&parsed.syntax(), SyntaxKind::CHOICE_POINT));
        }

        #[test]
        fn full_choice_point_produces_no_errors(point in arb_full_choice_point()) {
            // Every piece is generated in the documented, well-formed
            // shape (guard before label, valid idents), so this must be
            // clean end to end.
            let src = format!("flow f() {{\n{point}}}\n");
            let parsed = parse(&src);
            prop_assert!(parsed.errors().is_empty(), "errors: {:?}", parsed.errors());
        }
    }
}
