//! Per-branch span shape tests (issue #404 review finding).
//!
//! The PR's only prior coverage (`provenance_seam.rs`) round-trips and
//! garbles every stamped `ptr`, but both guarantees hold identically
//! whether a branch's `ptr` is its own per-branch span or a copy of the
//! enclosing construct's whole-block `ptr` — neither test asserts branches
//! actually carry *disjoint, per-branch* spans, which is the PR's entire
//! deliverable. This file pins that shape directly: for every branch
//! variant the PR touched, each branch's range must sit inside the
//! enclosing construct's range, and sibling branch ranges must not overlap.
//!
//! The branchless-body-with-else case (`branchless_body_with_else_arms_are_disjoint`)
//! is the regression test for the review's correctness finding: prior to
//! the fix, the implicit first arm was stamped from the whole
//! `BranchlessCondBody` node, which — per the parser's own
//! `branchless_body_with_else` CST snapshot — has `ElseBranch` as a
//! *child*, so the first arm's span strictly contained the sibling else
//! arm instead of sitting disjoint from it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_ir::hir::lower::{EffectSink, LowerScope, lower_simple_body};
use brink_ir::{Block, ContentPart, Diagnostic, FileId, Provenance, Stmt};
use brink_syntax::ast::AstNode;

// ─── Shared assertions ────────────────────────────────────────────────

/// `branch`'s range must sit inside `construct`'s range (a branch's span is
/// always narrower than the whole construct it belongs to — condition,
/// annotation, and delimiters live outside every branch).
fn assert_contained(construct: Provenance, branch: Provenance, what: &str) {
    assert!(
        construct.text_range().contains_range(branch.text_range()),
        "{what}: branch range {:?} must be contained in construct range {:?}",
        branch.text_range(),
        construct.text_range()
    );
    assert_ne!(
        construct.text_range(),
        branch.text_range(),
        "{what}: branch range must be strictly narrower than the whole construct"
    );
}

/// No two sibling branch ranges may overlap — the core issue #404 shape
/// invariant: a diagnostic/fold anchored to one branch must never also
/// cover a sibling branch.
fn assert_disjoint_siblings(branches: &[Provenance], what: &str) {
    for i in 0..branches.len() {
        for j in (i + 1)..branches.len() {
            let a = branches[i].text_range();
            let b = branches[j].text_range();
            let overlap = a.start() < b.end() && b.start() < a.end();
            assert!(
                !overlap,
                "{what}: sibling branches {i} ({a:?}) and {j} ({b:?}) overlap"
            );
        }
    }
}

fn check_branches(construct: Provenance, branches: &[Provenance], what: &str) {
    for (i, b) in branches.iter().enumerate() {
        assert_contained(construct, *b, &format!("{what} branch {i}"));
    }
    assert_disjoint_siblings(branches, what);
}

// ─── ink frontend ───────────────────────────────────────────────────

fn lower_ink_body(src: &str) -> (Block, Vec<Diagnostic>) {
    let parsed = brink_syntax::parse(src);
    let tree = parsed.tree();
    let scope = LowerScope::new(FileId(0));
    let mut sink = EffectSink::new(FileId(0));
    let block = lower_simple_body(tree.syntax(), &scope, &mut sink);
    let diags = sink.finish();
    (block, diags)
}

#[test]
fn ink_multiline_conditional_branches_are_disjoint() {
    let (block, diags) =
        lower_ink_body("{\n- x > 10:\n  Very big.\n- x > 5:\n  Big.\n- else:\n  Small.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::Conditional(cond) = &block.stmts[0] else {
        panic!("expected Conditional, got {:?}", block.stmts[0]);
    };
    assert_eq!(cond.branches.len(), 3);
    let branches: Vec<Provenance> = cond.branches.iter().map(|b| b.ptr).collect();
    check_branches(cond.ptr, &branches, "ink multiline conditional");
}

/// Regression test for the review's correctness finding: the branchless
/// body's implicit first arm must not contain the sibling `- else:` arm.
#[test]
fn ink_branchless_body_with_else_arms_are_disjoint() {
    let (block, diags) = lower_ink_body("{x > 5:\n  Rich.\n- else:\n  Poor.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::Conditional(cond) = &block.stmts[0] else {
        panic!("expected Conditional, got {:?}", block.stmts[0]);
    };
    assert_eq!(cond.branches.len(), 2);
    let branches: Vec<Provenance> = cond.branches.iter().map(|b| b.ptr).collect();
    check_branches(cond.ptr, &branches, "ink branchless-body-with-else");

    // Named directly, not just via the generic disjointness sweep: the
    // exact shape the finding called out.
    let first = branches[0].text_range();
    let second = branches[1].text_range();
    assert!(
        !first.contains_range(second),
        "first arm {first:?} must not contain the else arm {second:?}"
    );
}

#[test]
fn ink_block_sequence_branches_are_disjoint() {
    let (block, diags) = lower_ink_body("{\nstopping:\n- first\n- second\n- third\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::Sequence(seq) = &block.stmts[0] else {
        panic!("expected Sequence, got {:?}", block.stmts[0]);
    };
    assert_eq!(seq.branches.len(), 3);
    let branches: Vec<Provenance> = seq.branches.iter().map(|b| b.ptr).collect();
    check_branches(seq.ptr, &branches, "ink block sequence");
}

#[test]
fn ink_inline_sequence_branches_are_disjoint() {
    let (block, diags) = lower_ink_body("{First.|Second.|Third.}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::Content(c) = &block.stmts[0] else {
        panic!("expected Content, got {:?}", block.stmts[0]);
    };
    let seq = c
        .parts
        .iter()
        .find_map(|p| match p {
            ContentPart::InlineSequence(s) => Some(s),
            _ => None,
        })
        .expect("expected an InlineSequence part");
    assert_eq!(seq.branches.len(), 3);
    let branches: Vec<Provenance> = seq.branches.iter().map(|b| b.ptr).collect();
    check_branches(seq.ptr, &branches, "ink inline sequence");
}

// ─── native frontend ────────────────────────────────────────────────

fn lower_native_knot_body(src: &str) -> (Block, Vec<Diagnostic>) {
    let parsed = brink_syntax_native::parse(src);
    assert!(
        parsed.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parsed.errors()
    );
    let tree = parsed.tree();
    let (hir, _manifest, diags) = brink_ir::hir::lower_native::lower(FileId(0), &tree);
    (hir.knots[0].body.clone(), diags)
}

#[test]
fn native_if_else_branches_are_disjoint() {
    let (body, diags) = lower_native_knot_body(
        "flow a() {\n  {if true {\n    Yes.\n  } else {\n    No.\n  }}\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::Conditional(cond) = &body.stmts[0] else {
        panic!("expected Conditional, got {:?}", body.stmts[0]);
    };
    assert_eq!(cond.branches.len(), 2);
    let branches: Vec<Provenance> = cond.branches.iter().map(|b| b.ptr).collect();
    check_branches(cond.ptr, &branches, "native if/else");
}

#[test]
fn native_match_branches_are_disjoint() {
    let (body, diags) = lower_native_knot_body(
        "flow a() {\n  {match mood { calm => { Happy. }, wary => { Sad. }, tense => { Meh. } }}\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::Conditional(cond) = &body.stmts[0] else {
        panic!("expected Conditional, got {:?}", body.stmts[0]);
    };
    assert_eq!(cond.branches.len(), 3);
    let branches: Vec<Provenance> = cond.branches.iter().map(|b| b.ptr).collect();
    check_branches(cond.ptr, &branches, "native match");
}

#[test]
fn native_pipe_inline_alternation_branches_are_disjoint() {
    let (body, diags) = lower_native_knot_body("flow a() {\n  {~ One. | Two. | Three.}\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::Sequence(seq) = &body.stmts[0] else {
        panic!("expected Sequence, got {:?}", body.stmts[0]);
    };
    assert_eq!(seq.branches.len(), 3);
    let branches: Vec<Provenance> = seq.branches.iter().map(|b| b.ptr).collect();
    check_branches(
        seq.ptr,
        &branches,
        "native pipe-separated inline alternation",
    );
}
