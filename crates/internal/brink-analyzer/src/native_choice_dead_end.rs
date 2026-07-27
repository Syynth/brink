//! Native-only lint: **asymmetric choice-branch dead-end** (`E151`, issue
//! #1219, decision-log 2026-07-22 "Flows end implicitly (native)" item 4).
//!
//! That ruling retired ink's "ran out of content. Need a `-> DONE` or `->
//! END`?" runtime error on the native surface — a flow or braced body that
//! runs out of content now ends implicitly (`DONE`), never a fault. The
//! ruling also identified the **one genuinely useful thing** that error used
//! to catch and relocated it here as a narrow lint rather than losing it
//! outright: a choice branch that falls through (no divert/return) while a
//! sibling branch diverts onward is the fingerprint of a forgotten `->`.
//!
//! **On-by-default, `Warning`-severity, and re-levelable — not opt-in.** The
//! decision log's own wording (2026-07-22 item 4) describes this as a
//! "low-priority, opt-in analyzer lint"; the shipped implementation deviates
//! from that: `DiagnosticCode::E151` is `Severity::Warning` like any other
//! tier-able diagnostic, which means it fires on every compile with no
//! per-project or per-file enable step. "Opt-in" would mean silent unless
//! explicitly turned on; what this actually has is the ordinary
//! `[lints]`/`//brink-disable` machinery every `Warning`-base code gets —
//! `brink_analyzer::strict::effective_severity` maps an explicit
//! `[lints] E151 = "allow"` to `Severity::Warning` (never fully silent, only
//! "never escalate"), and under a project's `[lints] deny-warnings = true`
//! it resolves to `Severity::Error` like any other warning would. The
//! deviation from the ruled posture is flagged on issue #1219 for owner
//! sign-off, not shipped silently.
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
//! ## Why this does *not* read [`Block::tail`] directly, and is not a
//! ## literal-last-statement test either
//!
//! [`Block::tail`]'s contract is the *literal last statement* — see
//! `hir::normalize`'s `lifted_conditional_branch_with_divert_recomputes_tail`
//! regression test, which pins "a trailing `Stmt::EndOfLine` after a divert
//! flips the tail to `Tail::Unit`" as intentional for *that* consumer. A
//! naive last-statement-only [`diverges`] fares no better — it produces at
//! least four false-positive/false-negative classes against real native
//! lowerings (all four reproduced empirically against `compile_native`
//! fixtures during review of this module):
//!
//! - Every native choice unconditionally gets a boundary `Stmt::EndOfLine`
//!   appended right after its own inline divert region, *before* any braced
//!   body content (`hir::lower_native::choice::lower_choice`: `stmts =
//!   [divert?, EndOfLine, ...body]`, "matches old ink's choice body preamble
//!   … preserved for runtime output parity"). So the single most common
//!   diverting-choice idiom — a bare `* Choice -> target`, no braces, exactly
//!   the issue's own worked example — *always* has a trailing `EndOfLine`
//!   after its divert, dead code the divert already made unreachable.
//! - `lower_choice`'s `stmts = [divert?, EndOfLine, ...body]` also means an
//!   inline divert *before* a braced body (`* Choice -> target { text }`)
//!   puts the `Divert` **first**, not last — a literal-last-statement rule
//!   would call this non-diverging and additionally sit alongside the
//!   `E033` "unreachable code after divert" this shape independently earns,
//!   two contradictory diagnostics on the same choice.
//! - `lower_native::body::lower_items`'s G-1 label-absorption (module doc
//!   above) can wrap a choice's own `[content, Divert]` tail into a
//!   *trailing* `Stmt::LabeledBlock` — the divert is real and unconditional,
//!   just one level of wrapping down.
//! - A trailing `{if … {…} else {…}}` where **every** arm diverges is a
//!   terminator in substance (nothing falls through no matter which branch
//!   runs) but is not itself a `Stmt::Divert`/`Stmt::Return`.
//! - A trailing nested `{? … }` choice point is never itself a dead end —
//!   whether *it* has a forgotten `->` is entirely its own, independently
//!   walked [`check_choice_set`] call ([`walk_block`] recurses into every
//!   choice's body); folding its presence into the *outer* choice's
//!   diverge/fall-through verdict would be a category error, not an
//!   approximation.
//!
//! [`diverges`] instead asks "can control ever fall through to whatever
//! follows this choice's body" — a real (if shallow) control-flow-termination
//! predicate, not a syntactic last-statement peek:
//!
//! 1. If **any** top-level statement (searched irrespective of position, not
//!    only the last) is `Stmt::Divert`/`Stmt::Return`, the body diverges —
//!    handles the inline-divert-first and EndOfLine-boundary cases above in
//!    one rule, since an unconditional top-level terminator makes everything
//!    once it executes unreachable regardless of where it sits.
//! 2. Otherwise, walk past a trailing run of `Stmt::EndOfLine` markers to the
//!    last substantive statement and recurse by its shape: a trailing
//!    `Stmt::LabeledBlock` diverges iff its own statements do (recursively,
//!    same two rules); a trailing `Stmt::Conditional` diverges iff it has an
//!    explicit `else` arm (a branch with `condition: None` — otherwise
//!    there's an implicit fall-through path the checker can't see a
//!    terminator for) **and** every branch's body diverges; a trailing
//!    `Stmt::ChoiceSet` counts as diverging for *this* purpose (not a dead
//!    end — see the bullet above); anything else falls through.
//!
//! One shape this deliberately does **not** change: a choice ending in a
//! *tunnel call* (`-> combat ->`) is **not** treated as diverging, matching
//! [`tail_from_stmts`]'s own documented contract ("a tunnel call returns
//! control to the statement after it once the tunnel pops, so a block ending
//! in one still falls through") — a `TunnelCall` is not in the top-level
//! terminator check above and has no trailing-shape recursion case, so it
//! falls through to "does not diverge" like any ordinary statement. That is
//! correct, not a gap: once the tunnel returns, this choice's body really
//! does fall through to whatever follows (or nothing, ending implicitly), so
//! a tunnel-call-only choice beside a diverging sibling at a genuine dead end
//! is flagged exactly as intended.
//!
//! [`Block::tail`]: brink_ir::hir::Block::tail
//! [`tail_from_stmts`]: brink_ir::hir::tail_from_stmts
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
                    walk_block(file_id, &branch.body, diags);
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
/// directly, and is not a literal-last-statement test either" section for
/// the four false-positive/false-negative classes a naive
/// `matches!(body.tail(), Tail::Diverge(_))` (or an equivalent
/// last-statement-only check) produces, and the rules implemented here.
fn diverges(body: &Block) -> bool {
    terminates(&body.stmts)
}

/// A shallow control-flow-termination predicate over a statement list: true
/// iff execution can never fall through to whatever follows. See
/// [`diverges`]'s doc for the two rules this implements.
fn terminates(stmts: &[Stmt]) -> bool {
    // Rule 1: an unconditional terminator anywhere at this level makes
    // everything after it unreachable once it runs — no need to require it
    // be textually last (handles `lower_choice`'s `[divert?, EndOfLine,
    // ...body]` shape, where an inline divert precedes a braced body).
    if stmts
        .iter()
        .any(|s| matches!(s, Stmt::Divert(_) | Stmt::Return(_)))
    {
        return true;
    }
    // Rule 2: no top-level terminator — ask whether the trailing statement
    // (past any dead `EndOfLine` boundary markers) is itself a construct
    // that always terminates.
    match stmts.iter().rev().find(|s| !matches!(s, Stmt::EndOfLine)) {
        Some(Stmt::LabeledBlock(inner)) => terminates(&inner.stmts),
        Some(Stmt::Conditional(cond)) => {
            cond.branches.iter().any(|b| b.condition.is_none())
                && cond.branches.iter().all(|b| terminates(&b.body.stmts))
        }
        // A trailing nested `{? … }` is never itself a dead end for the
        // *outer* choice's purposes — whether it has a forgotten `->` is
        // its own independent `check_choice_set` call via `walk_block`'s
        // recursion, not something this outer verdict should fold in.
        Some(Stmt::ChoiceSet(_)) => true,
        // Deliberately excluded: a trailing `Stmt::TunnelCall` (`-> target
        // ->`) is *not* a terminator here, matching `tail_from_stmts`'s own
        // documented contract — a tunnel call returns control to whatever
        // follows once it pops, so the body genuinely still falls through.
        _ => false,
    }
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
    use brink_ir::hir::{
        Choice, ChoiceSetContext, Divert, DivertPath, DivertTarget, Expr, Name, Path,
    };
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

    // ─── Regression coverage for the four false-positive classes a naive
    // last-statement-only `diverges` produces (module doc's "Why this does
    // not read `Block::tail` directly, and is not a literal-last-statement
    // test either" section). ────────────────────────────────────────────

    fn return_stmt() -> Stmt {
        Stmt::Return(brink_ir::hir::Return {
            ptr: Some(Provenance::synthetic(NodeClass::Return, synthetic_range())),
            kind: brink_ir::hir::ReturnKind::Explicit,
            value: None,
            onwards_args: Vec::new(),
        })
    }

    /// (a) G-1 label absorption: `lower_native::body::lower_items` wraps a
    /// labeled content line and everything after it into a trailing
    /// `Stmt::LabeledBlock`. A choice whose real divert lives one level down
    /// inside that wrap must still be seen as diverging.
    #[test]
    fn diverges_recurses_into_a_trailing_label_absorbed_block() {
        let labeled = Stmt::LabeledBlock(Box::new(Block::from_stmts(vec![
            Stmt::Content(brink_ir::hir::Content {
                ptr: None,
                parts: Vec::new(),
                tags: Vec::new(),
            }),
            Stmt::EndOfLine,
            divert_stmt(),
        ])));
        let body = Block::from_stmts(vec![labeled]);
        assert!(
            diverges(&body),
            "a divert wrapped in a trailing label-absorbed LabeledBlock must count as diverging"
        );
    }

    /// (b) An inline divert placed *before* a braced body (`* Choice ->
    /// target { text }`) lowers to `[Divert, EndOfLine, ...body]` — the
    /// divert is first, not last. Must still count as diverging (and not
    /// contradict the independently-earned E033 on the same choice).
    #[test]
    fn diverges_sees_a_leading_divert_before_a_braced_body() {
        let body = Block::from_stmts(vec![
            divert_stmt(),
            Stmt::EndOfLine,
            Stmt::Content(brink_ir::hir::Content {
                ptr: None,
                parts: Vec::new(),
                tags: Vec::new(),
            }),
        ]);
        assert!(
            diverges(&body),
            "a divert preceding a braced body is still an unconditional terminator"
        );
    }

    fn cond_branch(condition: Option<Expr>, stmts: Vec<Stmt>) -> brink_ir::hir::CondBranch {
        brink_ir::hir::CondBranch {
            ptr: Provenance::synthetic(NodeClass::ConditionalBranch, synthetic_range()),
            condition,
            binding: None,
            body: Block::from_stmts(stmts),
            container_id: None,
        }
    }

    /// (c) A trailing conditional where every arm diverges (and an explicit
    /// `else` covers the implicit fall-through path) is a terminator in
    /// substance, even though it is not itself a `Divert`/`Return`.
    #[test]
    fn diverges_recognizes_an_all_arms_diverging_conditional_with_else() {
        let body = Block::from_stmts(vec![Stmt::Conditional(brink_ir::hir::Conditional {
            ptr: Provenance::synthetic(NodeClass::Conditional, synthetic_range()),
            kind: brink_ir::hir::CondKind::IfElse,
            branches: vec![
                cond_branch(Some(Expr::Bool(true)), vec![divert_stmt()]),
                cond_branch(None, vec![divert_stmt()]),
            ],
        })]);
        assert!(
            diverges(&body),
            "every arm diverges and there's an explicit else — this is a terminator"
        );
    }

    /// The mirror: a conditional missing an explicit `else` arm has an
    /// implicit fall-through path no branch accounts for, so it must not be
    /// treated as diverging even if every *present* branch does.
    #[test]
    fn diverges_rejects_a_conditional_without_an_else_arm() {
        let body = Block::from_stmts(vec![Stmt::Conditional(brink_ir::hir::Conditional {
            ptr: Provenance::synthetic(NodeClass::Conditional, synthetic_range()),
            kind: brink_ir::hir::CondKind::InitialCondition,
            branches: vec![cond_branch(Some(Expr::Bool(true)), vec![divert_stmt()])],
        })]);
        assert!(
            !diverges(&body),
            "no else arm means an implicit fall-through path this checker can't see a terminator for"
        );
    }

    /// (d) A trailing nested `{? … }` is never itself a dead end for the
    /// outer choice's purposes — it is checked independently by
    /// `walk_block`'s own recursion into `choice.body`.
    #[test]
    fn diverges_treats_a_trailing_nested_choice_set_as_not_a_dead_end() {
        let nested = choice_set(
            vec![idiomatic_diverting_choice(), idiomatic_diverting_choice()],
            Block::from_stmts(Vec::new()),
        );
        let body = Block::from_stmts(vec![
            Stmt::Content(brink_ir::hir::Content {
                ptr: None,
                parts: Vec::new(),
                tags: Vec::new(),
            }),
            Stmt::EndOfLine,
            Stmt::ChoiceSet(Box::new(nested)),
        ]);
        assert!(
            diverges(&body),
            "a trailing nested choice point must not make the outer choice look like a dead end"
        );
    }

    /// Explicit ruling, locked in: a choice body ending in a *tunnel call*
    /// (`-> target ->`) is deliberately **not** treated as diverging, matching
    /// `tail_from_stmts`'s documented contract that a tunnel call falls
    /// through once it pops. This must not regress if `terminates` grows
    /// more cases later.
    #[test]
    fn diverges_does_not_treat_a_trailing_tunnel_call_as_a_terminator() {
        let body = Block::from_stmts(vec![Stmt::TunnelCall(brink_ir::hir::TunnelCall {
            ptr: Provenance::synthetic(NodeClass::TunnelCall, synthetic_range()),
            targets: vec![DivertTarget {
                path: DivertPath::Path(Path {
                    segments: vec![Name {
                        text: "combat".to_string(),
                        range: synthetic_range(),
                    }],
                    range: synthetic_range(),
                }),
                args: Vec::new(),
            }],
        })]);
        assert!(
            !diverges(&body),
            "a tunnel call returns control to whatever follows once it pops — still falls through"
        );
    }

    /// A bare `return` counts exactly like a `->` — the module doc's
    /// "return counts like divert" rule, exercised through the same
    /// leading-terminator path as (b) above.
    #[test]
    fn diverges_treats_return_as_an_interchangeable_terminator() {
        let body = Block::from_stmts(vec![return_stmt()]);
        assert!(diverges(&body));
    }
}
