//! Structural validation passes over the HIR.
//!
//! These passes walk the HIR statement tree and emit diagnostics for
//! structurally invalid patterns that the parser accepts but the language
//! semantics forbid.

use brink_ir::hir::{Block, Choice, ChoiceSet, ChoiceSetContext, Stmt};
use brink_ir::{Diagnostic, DiagnosticCode, FileId, HirFile};

/// Run all structural validation passes on the given files.
pub fn validate(files: &[(FileId, &HirFile)]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for &(file_id, hir) in files {
        check_choices_in_inline_context(file_id, hir, &mut diagnostics);
    }
    diagnostics
}

// ─── Choice-in-conditional/sequence validation ──────────────────────

/// Inklecate rejects choices nested inside conditionals or sequences when
/// the choice has no continuation path — no explicit divert on the choice
/// AND no statements after the conditional/sequence in the enclosing block
/// to fall through to.
///
/// Invalid: `{ true: * choice }` — dead end, no continuation.
/// Valid:   `{ true: * choice -> target }` — explicit divert.
/// Valid:   `{ true: + [Burn] \n Hello } \n - -> label` — gather after
///          the conditional provides a continuation path.
fn check_choices_in_inline_context(
    file_id: FileId,
    hir: &HirFile,
    diagnostics: &mut Vec<Diagnostic>,
) {
    walk_block(&hir.root_content, false, file_id, diagnostics);
    for knot in &hir.knots {
        walk_block(&knot.body, false, file_id, diagnostics);
        for stitch in &knot.stitches {
            walk_block(&stitch.body, false, file_id, diagnostics);
        }
    }
}

/// Walk a block's statements. `dead_end` is true when we're inside a
/// conditional/sequence that has no continuation after it — meaning
/// inline choices without diverts would be dead ends.
fn walk_block(block: &Block, dead_end: bool, file_id: FileId, diagnostics: &mut Vec<Diagnostic>) {
    for (i, stmt) in block.stmts.iter().enumerate() {
        match stmt {
            Stmt::ChoiceSet(cs) => {
                if cs.context == ChoiceSetContext::Inline && dead_end {
                    check_choice_set_diverts(cs, file_id, diagnostics);
                }
                // Always recurse into choice bodies + continuation.
                walk_choice_set(cs, file_id, diagnostics);
            }
            Stmt::Conditional(cond) => {
                let has_continuation = has_meaningful_stmts_after(&block.stmts, i);
                for branch in &cond.branches {
                    walk_block(&branch.body, !has_continuation, file_id, diagnostics);
                }
            }
            Stmt::Sequence(seq) => {
                let has_continuation = has_meaningful_stmts_after(&block.stmts, i);
                for branch in &seq.branches {
                    walk_block(branch, !has_continuation, file_id, diagnostics);
                }
            }
            Stmt::LabeledBlock(inner) => {
                walk_block(inner, dead_end, file_id, diagnostics);
            }
            _ => {}
        }
    }
}

/// Check if there are meaningful (non-EOL) statements after position `i`.
fn has_meaningful_stmts_after(stmts: &[Stmt], i: usize) -> bool {
    stmts[i + 1..].iter().any(|s| !matches!(s, Stmt::EndOfLine))
}

/// Walk into a choice set's choices and continuation.
fn walk_choice_set(cs: &ChoiceSet, file_id: FileId, diagnostics: &mut Vec<Diagnostic>) {
    for choice in &cs.choices {
        walk_block(&choice.body, false, file_id, diagnostics);
    }
    walk_block(&cs.continuation, false, file_id, diagnostics);
}

/// Check that every choice in the set has an explicit divert in its body.
/// Emit E029 for any choice that doesn't.
fn check_choice_set_diverts(cs: &ChoiceSet, file_id: FileId, diagnostics: &mut Vec<Diagnostic>) {
    for choice in &cs.choices {
        if !choice_has_explicit_divert(choice) {
            diagnostics.push(Diagnostic {
                file: file_id,
                range: choice.ptr.text_range(),
                message: "choice in conditional or sequence must explicitly divert".into(),
                code: DiagnosticCode::E029,
            });
        }
    }
}

/// A choice has an explicit divert if its body contains a `Divert`,
/// `TunnelCall`, or `ThreadStart` statement (at any depth — the divert
/// could be inside nested content).
fn choice_has_explicit_divert(choice: &Choice) -> bool {
    block_has_divert(&choice.body)
}

fn block_has_divert(block: &Block) -> bool {
    block.stmts.iter().any(|stmt| match stmt {
        Stmt::Divert(_) | Stmt::TunnelCall(_) | Stmt::ThreadStart(_) => true,
        Stmt::Conditional(cond) => cond.branches.iter().all(|b| block_has_divert(&b.body)),
        Stmt::LabeledBlock(inner) => block_has_divert(inner),
        _ => false,
    })
}
