use crate::support::*;
use brink_ir::lir;

// ─── Issue #2903 — index-operand postfix in an inline block ────────────────
//
// `content.rs`'s `lower_inline_block` (used for an un-lifted
// `InlineConditional`/`InlineSequence` embedded in choice display text — see
// `logicblock_inside_unlifted_inline_conditional_sequence.rs`'s doc for why
// that shape exists) is a THIRD classic-line-shaped surface, alongside
// `mod.rs`'s top-level dispatch and the plain `~ { … }` block form, that
// calls `stmts::lower_stmt` — whose `Option<Stmt>` return truncates the
// multi-`Stmt` RMW sequence `lower_indexed_assignment` now produces for an
// Index-operand postfix (`a[0]++`) to just its first, harmless-but-inert
// step, dropping the actual write-back silently. `lower_inline_block` now
// intercepts an Index-operand postfix with its own `try_lower_postfix_stmt`
// arm before falling through to `stmts::lower_stmt`, mirroring the arm
// `mod.rs`'s top-level dispatch already has for this same reason.

#[test]
fn index_postfix_in_choice_text_inline_conditional_lowers_to_a_real_assign() {
    let src = "VAR a = #[1, 2, 3]\n* Pick {a[0] > 0:\n- true: ~ a[0]++\n- else: ~ a[0]--\n}\n    -> END\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let program =
        program.expect("choice-text index postfix should lower to a real program, not drop");

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
        "expected the Index-operand postfix's RMW `Assign` write-back spliced into the \
         InlineConditional's branches, not truncated away by `stmts::lower_stmt`'s \
         single-`Option<Stmt>` return (issue #2903)"
    );
}
