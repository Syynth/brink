use crate::support::*;
use brink_ir::lir;

// ─── Basic content ──────────────────────────────────────────────────

#[test]
fn minimal_story_has_root_container() {
    let p = lower_ink("Hello, world!\n");
    assert_eq!(p.root.kind, lir::ContainerKind::Root);
}

#[test]
fn root_content_emits_text() {
    let p = lower_ink("Hello, world!\n");
    let r = root(&p);
    let texts = collect_text(&r.body);
    assert_eq!(texts, vec!["Hello, world!"]);
}

#[test]
fn root_has_implicit_done() {
    let p = lower_ink("Hello!\n");
    let r = root(&p);
    assert!(
        ends_with_divert(&r.body),
        "root should end with implicit DONE"
    );
    if let Some(lir::StmtKind::Divert(d)) = r.body.last().map(|s| &s.kind) {
        assert!(
            matches!(d.target, lir::DivertTarget::Done),
            "root should end with DONE, not {:?}",
            std::mem::discriminant(&d.target)
        );
    }
}

#[test]
fn multiple_content_lines() {
    let p = lower_ink("Line one.\nLine two.\nLine three.\n");
    let r = root(&p);
    let texts = collect_text(&r.body);
    assert_eq!(texts, vec!["Line one.", "Line two.", "Line three."]);
}
