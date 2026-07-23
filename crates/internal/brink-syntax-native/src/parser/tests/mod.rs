mod annotation;
mod brace_family;
mod choice;
mod content;
mod declaration;
mod divert;
mod expression;
mod statement;
mod trivia;

use super::*;
use crate::SyntaxNode;
use crate::ast::{self, AstNode as _};

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

fn has_node_kind(root: &SyntaxNode, kind: SyntaxKind) -> bool {
    root.descendants().any(|node| node.kind() == kind)
}

/// Token-level counterpart to `has_node_kind` — `descendants()` yields only
/// nodes, so a check for a token kind (e.g. `ERROR_TOKEN`, which
/// `SyntaxKind::is_token()` lists and is therefore never wrapped in a
/// `start_node` call) must walk `descendants_with_tokens()` instead, or the
/// assertion can never fail regardless of what the parser actually emitted.
fn has_token_kind(root: &SyntaxNode, kind: SyntaxKind) -> bool {
    root.descendants_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .any(|token| token.kind() == kind)
}

fn count_node_kind(root: &SyntaxNode, kind: SyntaxKind) -> usize {
    root.descendants()
        .filter(|node| node.kind() == kind)
        .count()
}

/// Concatenate the text of every `TEXT` node under `root`, in tree order —
/// the significant-whitespace fix's key observable: prose lands in `TEXT`
/// nodes, and inter-token whitespace must live INSIDE those nodes (not as
/// bare trivia hung off the enclosing content node, where lowering — which
/// walks node children — would drop it).
fn text_run_concat(root: &SyntaxNode) -> String {
    root.descendants()
        .filter(|node| node.kind() == SyntaxKind::TEXT)
        .map(|node| node.text().to_string())
        .collect()
}

#[test]
fn empty_source_parses() {
    let p = assert_lossless("");
    assert_eq!(p.syntax().kind(), SyntaxKind::SOURCE_FILE);
    assert!(p.errors().is_empty());
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
