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
}

fn has_node_kind(root: &SyntaxNode, kind: SyntaxKind) -> bool {
    root.descendants().any(|node| node.kind() == kind)
}
