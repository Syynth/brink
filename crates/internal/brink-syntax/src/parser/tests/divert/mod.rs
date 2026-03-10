mod cst;

use super::check;
use crate::parse;

#[test]
fn simple_divert() {
    check("-> knot\n");
}

#[test]
fn divert_to_done() {
    check("-> DONE\n");
}

#[test]
fn divert_to_end() {
    check("-> END\n");
}

#[test]
fn divert_chain() {
    check("-> tunnel -> next\n");
}

#[test]
fn divert_with_args() {
    check("-> greet(\"hello\")\n");
}

#[test]
fn divert_dotted() {
    check("-> knot.stitch\n");
}

#[test]
fn thread_start() {
    check("<- background_thread\n");
}

#[test]
fn thread_with_args() {
    check("<- greet(name)\n");
}

#[test]
fn tunnel_onwards() {
    check("->->\n");
}

#[test]
fn tunnel_onwards_with_divert() {
    check("->-> -> next\n");
}

#[test]
fn content_then_divert() {
    check("Hello -> knot\n");
}

#[test]
fn tunnel_call_simple() {
    check("-> tunnel_test ->\n");
}

#[test]
fn tunnel_call_with_args() {
    check("-> tunnel_test(x) ->\n");
}

#[test]
fn tunnel_call_dotted() {
    check("-> knot.stitch ->\n");
}

#[test]
fn tunnel_call_is_tunnel_call_node() {
    let p = parse("-> tunnel_test ->\n");
    let root = p.syntax();
    let has_tunnel_call = root
        .descendants()
        .any(|n| n.kind() == crate::SyntaxKind::TUNNEL_CALL_NODE);
    assert!(has_tunnel_call, "expected TUNNEL_CALL_NODE in CST");
}

/// `-> tunnel2 ->->` = tunnel call to tunnel2, then tunnel return.
#[test]
fn tunnel_call_before_tunnel_onwards() {
    check("-> tunnel2 ->->\n");
    let p = parse("-> tunnel2 ->->\n");
    let root = p.syntax();
    let has_tunnel_call = root
        .descendants()
        .any(|n| n.kind() == crate::SyntaxKind::TUNNEL_CALL_NODE);
    assert!(
        has_tunnel_call,
        "expected TUNNEL_CALL_NODE for `-> tunnel2 ->->`"
    );
}

#[test]
fn regular_divert_not_tunnel_call() {
    let p = parse("-> target\n");
    let root = p.syntax();
    let has_tunnel_call = root
        .descendants()
        .any(|n| n.kind() == crate::SyntaxKind::TUNNEL_CALL_NODE);
    assert!(
        !has_tunnel_call,
        "regular divert should not have TUNNEL_CALL_NODE"
    );
}

#[test]
fn chained_divert_not_tunnel_call() {
    let p = parse("-> a -> b\n");
    let root = p.syntax();
    let has_tunnel_call = root
        .descendants()
        .any(|n| n.kind() == crate::SyntaxKind::TUNNEL_CALL_NODE);
    assert!(
        !has_tunnel_call,
        "chained divert should not have TUNNEL_CALL_NODE"
    );
}

#[test]
fn thread_with_divert_arg() {
    check("<- listAvailableStorylets(2, -> opts)\n");
}

/// `->-> B` = tunnel onwards with direct target override.
#[test]
fn tunnel_onwards_with_target() {
    check("->-> B\n");
    let p = parse("->-> B\n");
    let root = p.syntax();
    let tunnel = root
        .descendants()
        .find(|n| n.kind() == crate::SyntaxKind::TUNNEL_ONWARDS_NODE)
        .expect("expected TUNNEL_ONWARDS_NODE");
    let has_target = tunnel
        .descendants()
        .any(|n| n.kind() == crate::SyntaxKind::DIVERT_TARGET_WITH_ARGS);
    assert!(
        has_target,
        "tunnel onwards `->-> B` should have a DIVERT_TARGET_WITH_ARGS child"
    );
}

/// `->-> DONE` = tunnel onwards to DONE keyword.
#[test]
fn tunnel_onwards_to_done() {
    check("->-> DONE\n");
}

/// `->-> target(arg)` = tunnel onwards with target and arguments.
#[test]
fn tunnel_onwards_with_target_args() {
    check("->-> target(x)\n");
}

#[test]
fn insta_tunnel_call() {
    let p = parse("-> tunnel_test ->\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_divert_chain() {
    let p = parse("-> tunnel -> knot.stitch\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_thread_start() {
    let p = parse("<- background\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}
