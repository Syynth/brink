use super::*;

// ── Path ─────────────────────────────────────────────────────────────

#[test]
fn path_simple() {
    let tree = parse_tree("-> target\n");
    let cl = tree.content_lines().next().unwrap();
    let divert = cl.divert().unwrap();
    let simple = divert.simple_divert().unwrap();
    let target = simple.targets().next().unwrap();
    let path = target.path().unwrap();
    assert_eq!(path.full_name(), "target");
    assert_eq!(path.segments().count(), 1);
}

#[test]
fn path_dotted() {
    let tree = parse_tree("-> knot.stitch\n");
    let cl = tree.content_lines().next().unwrap();
    let divert = cl.divert().unwrap();
    let simple = divert.simple_divert().unwrap();
    let target = simple.targets().next().unwrap();
    let path = target.path().unwrap();
    assert_eq!(path.full_name(), "knot.stitch");
    assert_eq!(path.segments().count(), 2);
}

#[test]
fn path_three_segments() {
    let tree = parse_tree("-> a.b.c\n");
    let cl = tree.content_lines().next().unwrap();
    let divert = cl.divert().unwrap();
    let simple = divert.simple_divert().unwrap();
    let target = simple.targets().next().unwrap();
    let path = target.path().unwrap();
    assert_eq!(path.full_name(), "a.b.c");
    assert_eq!(path.segments().count(), 3);
}

// ── SimpleDivert ─────────────────────────────────────────────────────

#[test]
fn simple_divert_single_target() {
    let tree = parse_tree("-> myKnot\n");
    let cl = tree.content_lines().next().unwrap();
    let divert = cl.divert().unwrap();
    let simple = divert.simple_divert().unwrap();
    assert_eq!(simple.targets().count(), 1);
}

// ── DivertTargetWithArgs ─────────────────────────────────────────────

#[test]
fn divert_to_done() {
    let tree = parse_tree("-> DONE\n");
    let dta = parse_first::<DivertTargetWithArgs>("-> DONE\n");
    assert!(dta.done_kw().is_some());
    assert!(dta.path().is_none());
    let _ = tree;
}

#[test]
fn divert_to_end() {
    let dta = parse_first::<DivertTargetWithArgs>("-> END\n");
    assert!(dta.end_kw().is_some());
}

#[test]
fn divert_target_with_path() {
    let dta = parse_first::<DivertTargetWithArgs>("-> target\n");
    assert!(dta.path().is_some());
    assert!(dta.done_kw().is_none());
    assert!(dta.end_kw().is_none());
}

// ── DivertNode variants ──────────────────────────────────────────────

#[test]
fn divert_node_simple() {
    let dn = parse_first::<DivertNode>("-> target\n");
    assert!(dn.simple_divert().is_some());
    assert!(dn.thread_start().is_none());
    assert!(dn.tunnel_onwards().is_none());
}

// ── ThreadStart ──────────────────────────────────────────────────────

#[test]
fn thread_start_target() {
    let tree = parse_tree("=== k ===\n<- myThread\n");
    let ts: ThreadStart = first(tree.syntax());
    let path = ts.target().unwrap();
    assert_eq!(path.full_name(), "myThread");
}

#[test]
fn thread_start_target_dotted() {
    let tree = parse_tree("=== k ===\n<- knot.stitch\n");
    let ts: ThreadStart = first(tree.syntax());
    let path = ts.target().unwrap();
    assert_eq!(path.full_name(), "knot.stitch");
}

// ── TunnelCallNode ──────────────────────────────────────────────────

#[test]
fn tunnel_call_targets() {
    let tree = parse_tree("=== k ===\n-> tunnel ->\n");
    let tc: TunnelCallNode = first(tree.syntax());
    assert!(tc.targets().next().is_some());
}

// ── DivertTargetExpr ─────────────────────────────────────────────────

#[test]
fn divert_target_expr_target() {
    let tree = parse_tree("=== k ===\n~ temp x = -> target\n");
    let dte: DivertTargetExpr = first(tree.syntax());
    let path = dte.target().unwrap();
    assert_eq!(path.full_name(), "target");
    assert_eq!(path.segments().count(), 1);
}

#[test]
fn divert_target_expr_dotted() {
    let tree = parse_tree("=== k ===\n~ temp x = -> knot.stitch\n");
    let dte: DivertTargetExpr = first(tree.syntax());
    let path = dte.target().unwrap();
    assert_eq!(path.full_name(), "knot.stitch");
    assert_eq!(path.segments().count(), 2);
}
