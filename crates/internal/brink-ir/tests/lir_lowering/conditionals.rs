use crate::support::*;
use brink_ir::lir;

// ─── Conditionals ───────────────────────────────────────────────────

#[test]
fn block_conditional() {
    let p = lower_ink(
        "\
VAR x = true
{
    - x:
        Yes.
    - else:
        No.
}
",
    );
    let r = root(&p);
    let has_cond = r
        .body
        .iter()
        .any(|s| matches!(&s.kind, lir::StmtKind::Conditional(_)));
    assert!(has_cond, "should have a Conditional statement");
}

#[test]
fn conditional_branch_count() {
    let p = lower_ink(
        "\
VAR x = 1
{
    - x == 1:
        One.
    - x == 2:
        Two.
    - else:
        Other.
}
",
    );
    let r = root(&p);
    let cond = r.body.iter().find_map(|s| {
        if let lir::StmtKind::Conditional(c) = &s.kind {
            Some(c)
        } else {
            None
        }
    });
    assert!(cond.is_some());
    assert_eq!(cond.unwrap().branches.len(), 3, "should have 3 branches");
}

#[test]
fn conditional_else_has_no_condition() {
    let p = lower_ink(
        "\
VAR x = 1
{
    - x == 1:
        One.
    - else:
        Other.
}
",
    );
    let r = root(&p);
    let cond = r.body.iter().find_map(|s| {
        if let lir::StmtKind::Conditional(c) = &s.kind {
            Some(c)
        } else {
            None
        }
    });
    let cond = cond.unwrap();
    assert!(
        cond.branches.last().unwrap().condition.is_none(),
        "else branch should have no condition"
    );
}
