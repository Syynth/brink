use crate::support::*;
use brink_ir::lir;

// ─── Tunnel calls ───────────────────────────────────────────────────

#[test]
fn tunnel_call_statement() {
    let p = lower_ink(
        "\
== start ==
-> helper ->
Done.
-> END

== helper ==
Helping.
->->
",
    );
    let start = find_child(&p.root, "start");
    let has_tunnel = start
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::TunnelCall(_)));
    assert!(has_tunnel, "should have a TunnelCall statement");
}
