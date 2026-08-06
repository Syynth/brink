use crate::support::*;
use brink_ir::lir;

// ─── Thread starts ──────────────────────────────────────────────────

#[test]
fn thread_start_statement() {
    let p = lower_ink(
        "\
== main ==
<- background
Main content.
-> END

== background ==
Background.
-> DONE
",
    );
    let knot = find_child(&p.root, "main");
    let has_thread = knot
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::ThreadStart(_)));
    assert!(has_thread, "should have a ThreadStart statement");
}
