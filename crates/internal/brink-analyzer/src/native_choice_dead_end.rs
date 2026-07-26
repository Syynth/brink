//! Native-only lint: **asymmetric choice-branch dead-end** (`E151`, issue
//! #1219, decision-log 2026-07-22 "Flows end implicitly (native)" item 4).
//!
//! That ruling retired ink's "ran out of content. Need a `-> DONE` or `->
//! END`?" runtime error on the native surface — a flow or braced body that
//! runs out of content now ends implicitly (`DONE`), never a fault. The
//! ruling also identified the **one genuinely useful thing** that error used
//! to catch and relocated it here as a narrow, **warning-severity, opt-in**
//! lint rather than losing it outright: a choice branch that falls through
//! (no divert/return) while a sibling branch diverts onward is the
//! fingerprint of a forgotten `->`.
//!
//! ```text
//! {?
//!   * Parry -> riposte
//!   * [Dodge] {
//!     You sidestep the blade.
//!   }
//! }
//! // ← Parry diverts, Dodge doesn't — did the author forget `-> riposte`?
//! ```
//!
//! # The exact shape flagged
//!
//! Within one [`ChoiceSet`], compare each choice's own body: if **at least
//! one** choice's body diverges ([`diverges`]) and **at least one** doesn't,
//! every non-diverging choice is flagged. A choice ending in `return`
//! counts exactly like one ending in `->` — both are the author explicitly
//! transferring control away, the same signal [`crate::strict`]'s
//! fall-through checks (`E150`) already treat `Divert`/`Return` as
//! interchangeable terminators for.
//!
//! ## Why this does *not* read [`Block::tail`] directly
//!
//! [`Block::tail`]'s contract is the *literal last statement* — see
//! `hir::normalize`'s `lifted_conditional_branch_with_divert_recomputes_tail`
//! regression test, which pins "a trailing `Stmt::EndOfLine` after a divert
//! flips the tail to `Tail::Unit`" as intentional for *that* consumer. But
//! every native choice unconditionally gets a boundary `Stmt::EndOfLine`
//! appended right after its own inline divert region, *before* any braced
//! body content (`hir::lower_native::choice::lower_choice`: `stmts =
//! [divert?, EndOfLine, ...body]`, "matches old ink's choice body preamble
//! … preserved for runtime output parity"). So the single most common
//! diverting-choice idiom — a bare `* Choice -> target`, no braces, exactly
//! the issue's own worked example — *always* has `Block::tail() ==
//! Tail::Unit` by that literal-last-statement rule, even though the
//! `EndOfLine` after the divert is dead code the divert already made
//! unreachable. Reading `Block::tail` directly here would make this lint
//! permanently blind to the shape it exists to catch, so [`diverges`]
//! instead walks past a trailing run of `Stmt::EndOfLine` markers before
//! asking whether the choice actually ends in a `Divert`/`Return`.
//!
//! [`Block::tail`]: brink_ir::hir::Block::tail
//!
//! # Precision over recall: the dissolved-gather exclusion
//!
//! **The single precision-critical exclusion**: a [`ChoiceSet`] whose
//! `continuation` is non-empty is never flagged, regardless of how its
//! choices compare. Native prose structure has **no gather**
//! (`docs/native-surface-charter.md` §5, "THE GATHER IS DISSOLVED") —
//! `continuation` is built by `hir::lower_native::body::lower_continuation`
//! from whatever source text follows the closed `{? … }` block, i.e. the
//! dissolved gather's own content. When that continuation is non-empty,
//! *every* non-diverging choice converges there by design — reconverging
//! some branches while others divert elsewhere entirely is the single most
//! common legitimate asymmetric-weave shape ("`[Flee]` rejoins the shared
//! narration, `[Fight]` diverts to a whole different scene"), not a
//! mistake. Flagging it would be exactly the false-positive-on-legitimate-
//! content failure mode issue #1219 warns is worse than a miss. Only a
//! **genuine dead end** — nothing at all follows the choice point in this
//! scope, so a non-diverging choice silently lets the *whole flow* end
//! implicitly while its sibling diverts on purpose — is the shape this lint
//! exists for.
//!
//! # Deliberately NOT excluded (considered and rejected)
//!
//! - **An empty choice body** (`* Flee` with no body at all) is *not*
//!   exempted just because it's terse. It still doesn't divert, and a bare,
//!   undiverted choice sitting next to a diverting sibling at a genuine dead
//!   end is, if anything, a *stronger* candidate for "forgot the `->`" than
//!   a choice with narration first — terseness is not evidence of intent
//!   either way, so it is not a basis for silence.
//! - **A fallback (`else`) choice** is compared like any other sibling. The
//!   ruling's signal is intra-choice-set asymmetry, not "not a keyword
//!   choice" — an undiverted fallback beside a diverting ordinary choice at
//!   a dead end carries the identical fingerprint.
//! - **A single-choice set** needs no explicit guard: one choice can never
//!   both diverge and not at once, so the "at least one of each" test is
//!   unsatisfiable by construction — never a special case.
//!
//! # Native-only, by construction
//!
//! Mirrors [`crate::validate_native_accept_list`]'s dispatch posture (F-I#10,
//! `docs/hir-admission-contract.md` §4.4): this pass is written against
//! native's own documented semantics (the dissolved gather above, and the
//! choice-preamble `EndOfLine` boundary marker) and must never run against
//! ink-produced HIR, whose weave-fold `continuation` means something
//! entirely different and whose choice lowering has no such marker. Unlike
//! that admission gate, though, this is **not** a non-suppressible
//! always-on check — it is `Severity::Warning` (`DiagnosticCode::E151`),
//! flows through the ordinary suppressible `diagnostics` channel
//! (`//brink-disable` applies), and is configurable through the project's
//! `[lints]` table exactly like every other `Warning`-base-severity code
//! (`brink_project_config`'s `[lints]` schema doc,
//! `AnalysisOptions::apply_project_config`).

use brink_ir::hir::{Block, ChoiceSet, Stmt};
use brink_ir::{Diagnostic, DiagnosticCode, FileId, HirFile};

fn diag(file: FileId, range: rowan::TextRange) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: DiagnosticCode::E151.title().to_string(),
        code: DiagnosticCode::E151,
    }
}

/// Run the lint over one native file's already-lowered [`HirFile`]. Only
/// meaningful for HIR the native frontend produced — never call this against
/// ink-produced HIR (see the module doc's "Native-only" section).
#[must_use]
pub fn check(file_id: FileId, hir: &HirFile) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    walk_block(file_id, &hir.root_content, &mut diags);
    for knot in &hir.knots {
        walk_block(file_id, &knot.body, &mut diags);
        for stitch in &knot.stitches {
            walk_block(file_id, &stitch.body, &mut diags);
        }
    }
    diags
}

/// Recurse through every nested block a native lowering can produce (mirrors
/// [`crate::native_admission`]'s own `walk_block` shape): a `ChoiceSet`'s
/// continuation and each choice's own body, a `LabeledBlock`, and each
/// `Conditional`/`Sequence` branch — a `{? … }` can appear nested inside any
/// of these.
fn walk_block(file_id: FileId, block: &Block, diags: &mut Vec<Diagnostic>) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::ChoiceSet(cs) => {
                check_choice_set(file_id, cs, diags);
                walk_block(file_id, &cs.continuation, diags);
                for choice in &cs.choices {
                    walk_block(file_id, &choice.body, diags);
                }
            }
            Stmt::LabeledBlock(b) => walk_block(file_id, b, diags),
            Stmt::Conditional(c) => {
                for branch in &c.branches {
                    walk_block(file_id, &branch.body, diags);
                }
            }
            Stmt::Sequence(s) => {
                for branch in &s.branches {
                    walk_block(file_id, branch, diags);
                }
            }
            Stmt::Content(_)
            | Stmt::Divert(_)
            | Stmt::TunnelCall(_)
            | Stmt::ThreadStart(_)
            | Stmt::TempDecl(_)
            | Stmt::Assignment(_)
            | Stmt::Return(_)
            | Stmt::ExprStmt(_)
            | Stmt::EndOfLine
            | Stmt::LogicBlock(_)
            | Stmt::Await(_) => {}
        }
    }
}

/// Whether a choice's own body transfers control away and never falls
/// through — see the module doc's "Why this does not read `Block::tail`
/// directly" section for why this is not simply `matches!(body.tail(),
/// Tail::Diverge(_))`. Walks past a trailing run of `Stmt::EndOfLine`
/// markers (dead code once a real terminator precedes them) before checking
/// whether the last substantive statement is a `Divert`/`Return`.
fn diverges(body: &Block) -> bool {
    body.stmts
        .iter()
        .rev()
        .find(|s| !matches!(s, Stmt::EndOfLine))
        .is_some_and(|s| matches!(s, Stmt::Divert(_) | Stmt::Return(_)))
}

/// The check proper — see the module doc's "The exact shape flagged" and
/// "Precision over recall" sections.
fn check_choice_set(file_id: FileId, cs: &ChoiceSet, diags: &mut Vec<Diagnostic>) {
    // The dissolved-gather exclusion: a non-empty continuation means every
    // non-diverging choice converges there by design, never a dead end.
    if !cs.continuation.stmts.is_empty() {
        return;
    }
    let any_diverges = cs.choices.iter().any(|c| diverges(&c.body));
    let any_falls_through = cs.choices.iter().any(|c| !diverges(&c.body));
    if !any_diverges || !any_falls_through {
        return;
    }
    for choice in &cs.choices {
        if !diverges(&choice.body) {
            diags.push(diag(file_id, choice.ptr.text_range()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::Provenance;
    use brink_ir::hir::{Choice, ChoiceSetContext, Divert, DivertPath, DivertTarget, Name, Path};
    use brink_ir::provenance::NodeClass;

    fn synthetic_range() -> rowan::TextRange {
        rowan::TextRange::new(0.into(), 1.into())
    }

    fn ptr() -> Provenance {
        Provenance::synthetic(NodeClass::Choice, synthetic_range())
    }

    fn base_choice(body: Block) -> Choice {
        Choice {
            ptr: ptr(),
            is_sticky: false,
            is_fallback: false,
            label: None,
            condition: None,
            start_content: None,
            bracket_content: None,
            inner_content: None,
            tags: Vec::new(),
            body,
            container_id: None,
        }
    }

    fn divert_stmt() -> Stmt {
        let range = synthetic_range();
        Stmt::Divert(Divert {
            ptr: Some(Provenance::synthetic(NodeClass::Divert, range)),
            target: DivertTarget {
                path: DivertPath::Path(Path {
                    segments: vec![Name {
                        text: "elsewhere".to_string(),
                        range,
                    }],
                    range,
                }),
                args: Vec::new(),
            },
        })
    }

    /// The single most common native diverting-choice idiom — `* Choice ->
    /// target`, no braces — lowers to `[Divert, EndOfLine]`
    /// (`hir::lower_native::choice::lower_choice`'s mandatory boundary
    /// marker). `diverges` must see through that, or this lint would never
    /// fire on the issue's own worked example.
    fn idiomatic_diverting_choice() -> Choice {
        base_choice(Block::from_stmts(vec![divert_stmt(), Stmt::EndOfLine]))
    }

    fn falling_through_choice() -> Choice {
        base_choice(Block::from_stmts(vec![Stmt::EndOfLine]))
    }

    fn choice_set(choices: Vec<Choice>, continuation: Block) -> ChoiceSet {
        ChoiceSet {
            choices,
            continuation,
            context: ChoiceSetContext::Inline,
            depth: 0,
            gather_id: None,
        }
    }

    #[test]
    fn diverges_sees_through_the_choice_preamble_endofline() {
        assert!(diverges(&idiomatic_diverting_choice().body));
        assert!(!diverges(&falling_through_choice().body));
    }

    #[test]
    fn mixed_tail_with_no_continuation_is_flagged() {
        let cs = choice_set(
            vec![idiomatic_diverting_choice(), falling_through_choice()],
            Block::from_stmts(Vec::new()),
        );
        let mut diags = Vec::new();
        check_choice_set(FileId(0), &cs, &mut diags);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E151);
    }

    #[test]
    fn all_diverge_is_clean() {
        let cs = choice_set(
            vec![idiomatic_diverting_choice(), idiomatic_diverting_choice()],
            Block::from_stmts(Vec::new()),
        );
        let mut diags = Vec::new();
        check_choice_set(FileId(0), &cs, &mut diags);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn all_fall_through_is_clean() {
        let cs = choice_set(
            vec![falling_through_choice(), falling_through_choice()],
            Block::from_stmts(Vec::new()),
        );
        let mut diags = Vec::new();
        check_choice_set(FileId(0), &cs, &mut diags);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn non_empty_continuation_is_the_dissolved_gather_exclusion() {
        let cs = choice_set(
            vec![idiomatic_diverting_choice(), falling_through_choice()],
            Block::from_stmts(vec![Stmt::Content(brink_ir::hir::Content {
                ptr: None,
                parts: Vec::new(),
                tags: Vec::new(),
            })]),
        );
        let mut diags = Vec::new();
        check_choice_set(FileId(0), &cs, &mut diags);
        assert!(
            diags.is_empty(),
            "non-empty continuation is legitimate reconvergence, not a dead end: {diags:?}"
        );
    }

    #[test]
    fn single_choice_never_flags_by_construction() {
        let cs = choice_set(
            vec![falling_through_choice()],
            Block::from_stmts(Vec::new()),
        );
        let mut diags = Vec::new();
        check_choice_set(FileId(0), &cs, &mut diags);
        assert!(diags.is_empty(), "{diags:?}");
    }
}
