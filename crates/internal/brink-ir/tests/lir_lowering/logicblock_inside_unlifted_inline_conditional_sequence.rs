use crate::support::*;
use brink_ir::lir;

// ─── #578: LogicBlock inside an un-lifted inline conditional/sequence ──────
//
// `hir::normalize_file` (called unconditionally by `lower_to_program`) lifts
// every `InlineConditional`/`InlineSequence` it finds in a block's own
// `Stmt::Content` parts into a top-level `Stmt::Conditional`/`Stmt::Sequence`
// — the safe path that reaches `lower_stmt` via `lower_block_with_children`,
// which explicitly dispatches `hir::Stmt::LogicBlock`. But normalization
// only walks `Block.stmts` (root/knot/stitch bodies, choice bodies,
// continuations) — it never touches a `Choice`'s own display text
// (`start_content`/`bracket_content`/`inner_content`), which LIR lowering
// feeds straight to `content::lower_content` regardless. A multiline
// branched conditional embedded in choice text (e.g. `* Pick {cond: - a:
// ~{...} - b: ...}`) therefore keeps its `InlineConditional` shape all the
// way to `content::lower_inline_block`, and if a branch contains a `~ { … }`
// T1b block, that reached `stmts::lower_stmt`'s `debug_assert!`-guarded
// "should be dispatched by lower_block_with_children" arm — panicking in
// debug builds, silently dropping the block's statements in release.
#[test]
fn logic_block_in_choice_text_inline_conditional_lowers_without_panicking() {
    let src = "VAR x = 1\n* Pick {x > 0:\n- true: ~ { x = x + 1 }\n- else: ~ { x = x - 1 }\n}\n    -> END\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let program =
        program.expect("choice-text LogicBlock should lower to a real program, not panic/drop");

    // Prove the LogicBlock's `Assign` statements actually made it into the
    // lowered choice's display text (not silently dropped): find the
    // top-level ChoiceSet, dig into its one choice's `start_content`, and
    // confirm the `InlineConditional`'s branches carry `Stmt::Assign`.
    let Some(lir::StmtKind::ChoiceSet(cs)) = program
        .root
        .body
        .iter()
        .find(|s| matches!(&s.kind, lir::StmtKind::ChoiceSet(_)))
        .map(|s| &s.kind)
    else {
        panic!("expected a ChoiceSet in the root body");
    };
    assert_eq!(cs.choices.len(), 1);
    let start_content = cs.choices[0]
        .start_content
        .as_ref()
        .expect("choice should have start_content (the inline conditional's text)");
    let has_assign_in_branches = start_content.parts.iter().any(|p| {
        let lir::ContentPart::InlineConditional(cond) = p else {
            return false;
        };
        cond.branches.iter().any(|b| {
            b.body
                .iter()
                .any(|s| matches!(&s.kind, lir::StmtKind::Assign { .. }))
        })
    });
    assert!(
        has_assign_in_branches,
        "expected the LogicBlock's Assign statements spliced into the \
         InlineConditional's branches, not dropped"
    );
}

#[test]
fn logic_block_second_inline_construct_on_a_content_line_lowers() {
    // Two inline logics on one content line: the first (`{x}`) is a plain
    // interpolation, so `lower_multiline_block_from_inline`'s "is it the
    // first inline logic AND promotable" check fails on it and the whole
    // line falls through to the generic content-parts path — the second
    // inline logic (the multiline conditional with LogicBlock branches)
    // becomes an un-lifted `ContentPart::InlineConditional` too, independent
    // of the choice-text case above.
    let src =
        "VAR x = 1\nHello {x} and {x > 0:\n- true: ~ { x = x + 1 }\n- else: ~ { x = x - 1 }\n}\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let program = program.expect("should lower to a real program, not panic/drop");
    assert!(!program.root.body.is_empty());
}
