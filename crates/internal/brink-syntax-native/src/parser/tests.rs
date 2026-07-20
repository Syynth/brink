use super::*;
use crate::SyntaxNode;

/// Every parser test's baseline invariant: the CST's total text equals the
/// source, byte-for-byte (rowan guarantees this for any well-formed tree —
/// this test catches a builder bug that would violate it).
fn assert_lossless(source: &str) -> Parse {
    let parsed = parse(source);
    assert_eq!(parsed.syntax().text().to_string(), source);
    parsed
}

/// The first direct-child node castable to the typed AST wrapper `N` — the
/// `parser::tests` module's own escape hatch, since `ast::support`'s
/// helpers of the same shape are `pub(super)`-scoped to the `ast` module
/// and not visible from here. (N-1: used by the new inline-divert tests to
/// pull the `DIVERT_TARGET` back out of a `DIVERT_STMT`.)
fn find_child<N: crate::ast::AstNode>(node: &SyntaxNode) -> Option<N> {
    node.children().find_map(N::cast)
}

#[test]
fn empty_source_parses() {
    let p = assert_lossless("");
    assert_eq!(p.syntax().kind(), SyntaxKind::SOURCE_FILE);
    assert!(p.errors().is_empty());
}

#[test]
fn minimal_flow_decl() {
    let p = assert_lossless("flow greet() {\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn flow_with_prose_body() {
    let p = assert_lossless("flow greet(name) {\n  Hello, {name}! <>\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn fn_decl_parses() {
    let p = assert_lossless("fn heal(hp) {\n  var x = hp + 1\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn choice_point_parses() {
    let src = "flow garden() {\n  {?\n    * [Look] You look around.\n    + (again) [Look again] Still a garden.\n    else { Nothing left to do. }\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

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

#[test]
fn annotation_line_parses() {
    let src = "fn heal(hp) {\n  @[effects(pure, silent, reads(gold, hp))]\n  var x = hp\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn divert_and_tunnel_and_return() {
    let src = "flow a() {\n  -> b\n}\nflow b() {\n  -> c ->\n  return\n}\nflow c() {\n  return -> a\n  -> END\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn use_and_import_and_module() {
    let src = "use story::npcs::{guard, merchant as trader};\nimport story::items\nmodule inner {\n  var secret = 1\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn use_decl_semicolon_is_consumed_by_the_decl_not_left_as_prose() {
    // `;` has no role anywhere else in the grammar — confirm it becomes a
    // token *inside* USE_DECL, not a stray token that just happens to
    // round-trip as unrelated adjacent prose text.
    let src = "use a::b;\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let use_decl = p
        .syntax()
        .children()
        .find(|n| n.kind() == SyntaxKind::USE_DECL)
        .expect("USE_DECL");
    assert!(
        use_decl
            .children_with_tokens()
            .any(|t| t.kind() == SyntaxKind::SEMICOLON),
        "expected the `;` inside USE_DECL, tree: {use_decl:#?}"
    );
    // And nothing else at the top level — the `;` didn't spawn its own
    // stray CONTENT_LINE sibling.
    assert_eq!(p.syntax().children().count(), 1);
}

#[test]
fn use_decl_without_semicolon_still_parses() {
    let src = "use a::b\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn var_const_flags_struct_extern() {
    let src = "var hp = 10\nconst MAX = 100\nflags Mood = (calm), wary, hostile\nstruct Item {\n  name: string,\n  weight: int\n}\nextern log(msg)\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn splice_inside_choice_point() {
    let src = "flow hub() {\n  {?\n    <- options()\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn lambda_pipe_tokenizes_and_parses() {
    let src = "var f = |x, y| x + y\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn nested_stitch_flow() {
    let src = "flow garden() {\n  flow gate() {\n    Creak.\n  }\n  -> gate\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

// ── Error recovery — malformed input must never panic or hang, and must
// still round-trip losslessly ────────────────────────────────────────

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

// ── Charter exhibit (docs/native-surface-charter.md §9) ─────────────
//
// b0-sequencing.md's B0.5 exit criteria calls for "the two charter
// exhibits (the Fogg passage, `FUNC_populate_options_thread` respelled)
// parse clean". Neither exhibit's respelled `.brink` text is actually
// checked into the repo — the charter says the respellings "live in the
// sitting transcript" (§9), but no such transcript file exists anywhere
// in this tree, and `FUNC_populate_options_thread`'s ink source isn't
// checked in either (grep-searched, see the B0.5 report's findings).
// The Fogg passage's ink ORIGINAL does exist, as an oracle fixture
// (`tests/tier2/conditional/condtext-v1/story.ink`) — this test is a
// good-faith respelling of that fixture into the ruled B0.5 surface,
// standing in for the missing official exhibit. It is not a substitute
// for running the real exhibit once it's committed somewhere.

#[test]
fn charter_exhibit_fogg_passage_respelling() {
    let src = concat!(
        "flow fogg_wager() {\n",
        "  \"We are going on a trip,\" said Monsieur Fogg.\n",
        "  {?\n",
        "    * [The wager.] -> know_about_wager\n",
        "    * [I was surprised.] -> i_stared\n",
        "  }\n",
        "}\n",
        "\n",
        "flow know_about_wager() {\n",
        "  I had heard about the wager.\n",
        "  -> i_stared\n",
        "}\n",
        "\n",
        "flow i_stared() {\n",
        "  I stared at Monsieur Fogg.\n",
        "  {if know_about_wager {\n",
        "    <> \"But surely you are not serious?\" I demanded.\n",
        "  } else {\n",
        "    <> \"But there must be a reason for this trip,\" I observed.\n",
        "  }}\n",
        "  He said nothing in reply, merely considering his newspaper ",
        "with as much thoroughness as entomologist considering his ",
        "latest pinned addition.\n",
        "  -> END\n",
        "}\n",
    );
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    // The dissolved gather (charter §5): "I stared at Monsieur Fogg."
    // is plain content immediately after the closed choice point, no
    // gather dash — this must NOT trip the `MINUS`-as-entry-marker path.
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CHOICE_POINT));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CONDITIONAL_BLOCK));
    // N-1: the two `* [text] -> target` choice lines must now each
    // produce a real DIVERT_STMT node (previously folded into TEXT — see
    // `tests/tier1-brink-respell/README.md`'s N-1 finding). Three standalone
    // statement-position diverts (`-> i_stared`, `-> END`) were already
    // recognized before this fix, so the total is 2 (content-position) + 2
    // (statement-position) = 4.
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::DIVERT_STMT), 4);
}

fn has_node_kind(root: &SyntaxNode, kind: SyntaxKind) -> bool {
    root.descendants().any(|node| node.kind() == kind)
}

fn count_node_kind(root: &SyntaxNode, kind: SyntaxKind) -> usize {
    root.descendants()
        .filter(|node| node.kind() == kind)
        .count()
}

// ── N-1: inline diverts in content position ─────────────────────────

#[test]
fn divert_after_choice_bracket_text_is_a_divert_node_not_text() {
    // The exact shape from the exhibit/manual-stitch-v1 fixtures:
    // `* [text] -> target`. Before N-1's fix this parsed with zero errors
    // but folded `-> know_about_wager` into a literal TEXT run.
    let src = "flow f() {\n  {?\n    * [The wager.] -> know_about_wager\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let choice_inner = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_INNER_CONTENT)
        .expect("CHOICE_INNER_CONTENT");
    assert!(
        has_node_kind(&choice_inner, SyntaxKind::DIVERT_STMT),
        "expected a DIVERT_STMT inside CHOICE_INNER_CONTENT, tree: {choice_inner:#?}"
    );
    // The divert's target must be a real PATH, not swallowed text.
    let divert = choice_inner
        .descendants()
        .find(|n| n.kind() == SyntaxKind::DIVERT_STMT)
        .expect("DIVERT_STMT");
    let target = find_child::<crate::ast::DivertTarget>(&divert).expect("DIVERT_TARGET");
    let path = target.path().expect("path");
    let segs: Vec<_> = path.segments().map(|t| t.text().to_string()).collect();
    assert_eq!(segs, vec!["know_about_wager".to_string()]);
}

#[test]
fn divert_after_dotted_path_target_in_choice_text_parses() {
    // manual-stitch-v1's other shape: a dotted stitch-addressing target.
    let src = "flow f() {\n  {?\n    * [go] -> f.g\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let divert = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::DIVERT_STMT)
        .expect("DIVERT_STMT");
    let target = find_child::<crate::ast::DivertTarget>(&divert).expect("DIVERT_TARGET");
    let path = target.path().expect("path");
    assert!(!path.crosses_module_wall()); // `.` not `::`
    let segs: Vec<_> = path.segments().map(|t| t.text().to_string()).collect();
    assert_eq!(segs, vec!["f".to_string(), "g".to_string()]);
}

#[test]
fn divert_inside_multiline_choice_body_after_prose_is_a_divert_node() {
    // The sticky-choice shape: a `->` following prose on the SAME content
    // line inside a braced CHOICE_BODY (as opposed to a divert on its own
    // line, which was already recognized before this fix).
    let src = "flow f() {\n  {?\n    + [Eat] {\n      You eat another donut. -> f\n    }\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let body = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_BODY)
        .expect("CHOICE_BODY");
    let content_line = body
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    // The divert is a child of the same CONTENT_LINE as the preceding
    // prose, not a sibling body item.
    assert!(
        has_node_kind(&content_line, SyntaxKind::DIVERT_STMT),
        "expected DIVERT_STMT nested inside the CONTENT_LINE, tree: {content_line:#?}"
    );
    assert!(has_node_kind(&content_line, SyntaxKind::TEXT));
}

#[test]
fn tunnel_call_in_content_position_parses() {
    // `->->` in content position: still a TUNNEL_CALL, not a divert
    // followed by stray text.
    let src = "flow f() {\n  {?\n    * [go] visit -> place ->\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::TUNNEL_CALL));
}

#[test]
fn divert_to_end_in_content_position_parses() {
    let src = "flow f() {\n  {?\n    * [go] The end. -> END\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let divert = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::DIVERT_STMT)
        .expect("DIVERT_STMT");
    let target = find_child::<crate::ast::DivertTarget>(&divert).expect("DIVERT_TARGET");
    assert!(target.is_end());
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
