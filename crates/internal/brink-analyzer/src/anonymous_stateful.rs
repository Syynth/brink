//! Anonymous-container state lint: unnamed once-only choices and unnamed
//! stateful sequences (`E157`, issue #1674, gap 4 of the identity cluster —
//! RULED 2026-07-27, PR #1670, "the proportionate response").
//!
//! # The exposure this lints against
//!
//! A save's `visit`/`turn` counts key on a scope's compiled `DefinitionId`
//! (`brink_format::VisitEntry`, `brink_ir::hir::stamp`). A **named** scope
//! (a knot, stitch, or a choice/gather carrying an author `(label)`) hashes
//! its id from that name — stable across a content edit anywhere else in the
//! project. An **anonymous** scope (an unlabeled once-only choice's target
//! container, or a sequence's wrapper container — `stamp::stamp_stmt`'s
//! `.c-{N}`/`.s-{N}` positional counters) hashes its id from *position*
//! instead: inserting or removing a sibling construct earlier in the same
//! **weave block** shifts every later counter, and with it every later
//! anonymous id. (Counters are weave-block-local since the 2026-08-29
//! re-scoping ruling — before it the `b-`/`s-` counter was knot-global, so
//! the blast radius was the whole scope; now it is the containing block.
//! A `(label)` additionally anchors its entire subtree — see
//! `stamp.rs`'s "Counter scoping and edit stability".)
//!
//! `brink_runtime::save::load_state` already reports this (issue #1674's
//! other deliverable, [`brink_format::LoadReport::anonymous_states_dropped`])
//! when a saved anonymous visit/turn count can no longer be placed. This
//! module is the *upstream* half: a compile-time lint an author can act on
//! **before** a save ever goes stale, rather than only learning about it from
//! a load report after the fact. Per the ruling, naming is the opt-in fix —
//! this lint exists to make that opt-in discoverable rather than folklore.
//!
//! # The exact shapes flagged
//!
//! - **A once-only choice with no `(label)`.** [`brink_ir::hir::Choice`]'s
//!   `is_sticky == false` (the `*` bullet, not `+`) means
//!   `brink-runtime`'s `handle_begin_choice` gates it on
//!   `context.visit_count(choice.container_id)` (`flags.once_only`: once
//!   visited, the choice is never offered again) — genuine durable state.
//!   `choice.label.is_none()` means that `container_id` is
//!   `stamp::stamp_stmt`'s positional `alloc_address(&format!("{scope}.c{N}"))`
//!   rather than a label lookup — anonymous.
//! - **A sequence** (`{cycle: …}` / `{stopping: …}` / `{once: …}` /
//!   `{shuffle: …}`, and shuffle-once/shuffle-stopping combinations) **with
//!   at least two branches, or exactly one `once`-flagged branch.**
//!   `brink-codegen-inkb`'s `emit_sequence` always derives a sequence's
//!   branch index from `Opcode::CurrentVisitCount` — the *current*
//!   container's own visit count, which for a sequence is always its
//!   dedicated wrapper container (`lir::lower::mod.rs`'s `hir::Stmt::Sequence`
//!   arm: `CountingFlags::VISITS`, `EnterContainer(wrapper_id)`) — and that
//!   wrapper id is *always* `stamp::stamp_stmt`'s positional `s-{N}` (there
//!   is no label syntax for a sequence at all, unlike a choice). See
//!   [`is_stateful_sequence`] for the single-branch exclusion.
//!
//! # Precision over recall
//!
//! - **A `+` sticky (repeatable) choice is never flagged.** It carries no
//!   "already chosen" state to begin with — `flags.once_only` is never set
//!   for it, so its container's visit count is never even consulted by
//!   choice gating. Flagging it would be a false positive on stateless
//!   syntax, exactly what the ruling calls "worse than a miss".
//! - **A fallback (`else`) choice is never flagged.** `is_sticky` is
//!   documented as meaningless for one (`hir::lower_native::choice`'s
//!   `lower_fallback_choice`: "native's fallback has no bullet, so
//!   `is_sticky` is meaningless here and defaults `false`") — treating that
//!   default as "once-only" would be reading a signal that was never set
//!   with this lint's question in mind.
//! - **A single-branch, non-`once` sequence is never flagged**
//!   ([`is_stateful_sequence`]): `{stopping: a}`/`{cycle: a}`/`{shuffle: a}`
//!   with exactly one branch computes the same index — `0` — on every visit
//!   regardless of the container's visit count (`min(vc, 0)`, `vc % 1`, and
//!   Fisher-Yates over a single element all collapse to the sole branch).
//!   The construct is syntactically an alternation but genuinely stateless;
//!   only `{once: a}` still varies (shown once, then nothing).
//! - Reachability/loop analysis (whether an anonymous scope can actually be
//!   *re-entered* in a given story's shape) is out of scope — a purely
//!   structural check, matching every other lint in this crate.
//!
//! # Severity: off/info by default, tier-able through `[lints]`
//!
//! `DiagnosticCode::E157`'s own default severity is `Severity::Info` — RULED
//! "off or info by default: a single-shot project should not be nagged,
//! while a team doing live-ops or shipping UGC can raise it to warn". Unlike
//! every diagnostic code before it, this is **not** a `Warning`-base code,
//! so reaching it through `[lints]` needed `brink_analyzer::strict`'s
//! `effective_severity`/`validate_lint_code` widened past their previous
//! `Warning`-only overridable set (see that module's doc) — otherwise a
//! project trying to `[lints] E157 = "warn"` would have silently gotten a
//! `ConfigWarning` instead.
//!
//! # Wiring: both frontends, per file
//!
//! Structural and file-local — no cross-file symbol resolution needed — so
//! this runs at the same lowering seam `brink-db`'s per-file `E151`/`E156`
//! checks do (`lower_file` for ink, `lower_native_file` for native), rather
//! than the whole-project analysis layer. Both frontends parse `(label)`
//! choice labels into `Choice::label` before this runs (`hir::lower::choice`
//! / `hir::lower_native::choice`), so the check needs no later
//! `stamp_container_ids` pass — the author-facing "did you name it" signal
//! is available immediately after lowering.

use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::hir::{Choice, Sequence, SequenceType};
use brink_ir::{Diagnostic, DiagnosticCode, FileId, HirFile};

fn diag(file: FileId, range: rowan::TextRange, message: String) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message,
        code: DiagnosticCode::E157,
    }
}

/// Run the lint over one already-lowered file's [`HirFile`]. Both `lower_file`
/// (ink) and `lower_native_file` (native) in `brink-db` call this per file —
/// see the module doc's "Wiring" section.
#[must_use]
pub fn check(file_id: FileId, hir: &HirFile) -> Vec<Diagnostic> {
    let mut v = AnonymousStatefulVisitor {
        file: file_id,
        diagnostics: Vec::new(),
    };
    visit::visit(hir, &mut v);
    v.diagnostics
}

struct AnonymousStatefulVisitor {
    file: FileId,
    diagnostics: Vec<Diagnostic>,
}

impl HirVisitor for AnonymousStatefulVisitor {
    fn enter_choice(&mut self, choice: &Choice) {
        if is_anonymous_once_only(choice) {
            self.diagnostics.push(diag(
                self.file,
                choice.ptr.text_range(),
                "this once-only choice has no name, so its 'already chosen' \
                 state is anonymous — inserting or removing an earlier \
                 sibling in the same weave block shifts its compiled \
                 identity and makes it reappear as if never chosen; give it \
                 a stable identity with `(label)`, which also anchors \
                 everything inside its body"
                    .to_owned(),
            ));
        }
    }

    fn enter_sequence(&mut self, seq: &Sequence) {
        if is_stateful_sequence(seq) {
            self.diagnostics.push(diag(
                self.file,
                seq.ptr.text_range(),
                "this sequence's position state is anonymous, so inserting \
                 or removing an earlier sibling in the same weave block \
                 shifts its compiled identity and makes it restart from its \
                 first branch; place it under a `(label)`ed choice or \
                 block, or in its own stably-named stitch, so nothing can \
                 renumber it"
                    .to_owned(),
            ));
        }
    }
}

/// A `*` (non-sticky, non-fallback) choice with no author `(label)` — see the
/// module doc's "The exact shapes flagged" and "Precision over recall"
/// sections for why `is_sticky`/`is_fallback` gate this the way they do.
fn is_anonymous_once_only(choice: &Choice) -> bool {
    !choice.is_sticky && !choice.is_fallback && choice.label.is_none()
}

/// Whether a sequence's own branch-selection output can ever vary with its
/// wrapper container's visit count — see the module doc's "Precision over
/// recall" section for the single-branch stateless exclusion this
/// implements. A sequence is never "named" the way a choice can be (no label
/// syntax exists for one), so unlike [`is_anonymous_once_only`] this has no
/// separate anonymity check — every stateful sequence is anonymous by
/// construction.
fn is_stateful_sequence(seq: &Sequence) -> bool {
    seq.branches.len() >= 2 || seq.kind.contains(SequenceType::ONCE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::FileId;
    use brink_syntax::parse;

    fn lower_ink(src: &str) -> HirFile {
        let parsed = parse(src);
        let tree = parsed.tree();
        let (hir, _, _) = brink_ir::hir::lower::lower(FileId(0), &tree);
        hir
    }

    fn lower_native(src: &str) -> HirFile {
        let parsed = brink_syntax_native::parse(src);
        let tree = parsed.tree();
        let (hir, _, _) = brink_ir::hir::lower_native::lower(FileId(0), &tree);
        hir
    }

    fn codes(diags: &[Diagnostic]) -> Vec<DiagnosticCode> {
        diags.iter().map(|d| d.code).collect()
    }

    // ── Once-only choices (ink) ──────────────────────────────────────

    #[test]
    fn unlabeled_once_only_choice_is_flagged() {
        let hir = lower_ink("=== knot ===\n* [pick] -> DONE\n");
        let diags = check(FileId(0), &hir);
        assert_eq!(codes(&diags), vec![DiagnosticCode::E157], "{diags:?}");
    }

    #[test]
    fn labeled_once_only_choice_is_not_flagged() {
        let hir = lower_ink("=== knot ===\n* (mine) [pick] -> DONE\n");
        let diags = check(FileId(0), &hir);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn sticky_choice_is_never_flagged() {
        let hir = lower_ink("=== knot ===\n+ [pick] -> DONE\n");
        let diags = check(FileId(0), &hir);
        assert!(diags.is_empty(), "a `+` choice carries no state: {diags:?}");
    }

    // ── Sequences (ink) ──────────────────────────────────────────────

    #[test]
    fn multi_branch_sequence_is_flagged() {
        let hir = lower_ink("=== knot ===\n{a|b|c}\n-> DONE\n");
        let diags = check(FileId(0), &hir);
        assert_eq!(codes(&diags), vec![DiagnosticCode::E157], "{diags:?}");
    }

    #[test]
    fn single_branch_stopping_sequence_is_not_flagged() {
        // A single-branch alternation's computed index is `0` on every
        // visit regardless of the container's visit count — genuinely
        // stateless despite the syntax. `$` (stopping) forces sequence
        // classification even with only one alternative.
        let hir = lower_ink("=== knot ===\n{$a}\n-> DONE\n");
        let diags = check(FileId(0), &hir);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn single_branch_once_only_alternation_is_flagged() {
        // `!` still varies with visit count even at one branch: shown once,
        // then nothing.
        let hir = lower_ink("=== knot ===\n{!a}\n-> DONE\n");
        let diags = check(FileId(0), &hir);
        assert_eq!(codes(&diags), vec![DiagnosticCode::E157], "{diags:?}");
    }

    #[test]
    fn cycle_sequence_with_two_branches_is_flagged() {
        let hir = lower_ink("=== knot ===\n{&a|b}\n-> DONE\n");
        let diags = check(FileId(0), &hir);
        assert_eq!(codes(&diags), vec![DiagnosticCode::E157], "{diags:?}");
    }

    #[test]
    fn story_with_no_stateful_anonymous_constructs_is_clean() {
        let hir = lower_ink("=== knot ===\nHello.\n-> DONE\n");
        let diags = check(FileId(0), &hir);
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── Native surface ───────────────────────────────────────────────
    // Fixtures mirror `brink-syntax-native`'s own parser test corpus
    // (`parser/tests/choice.rs`), which already proves this exact concrete
    // syntax parses.

    #[test]
    fn native_unlabeled_once_only_choice_is_flagged() {
        let hir = lower_native("flow f() {\n  {?\n    * [Look] You look around.\n  }\n}\n");
        let diags = check(FileId(0), &hir);
        assert_eq!(codes(&diags), vec![DiagnosticCode::E157], "{diags:?}");
    }

    #[test]
    fn native_labeled_once_only_choice_is_not_flagged() {
        let hir = lower_native("flow f() {\n  {?\n    * (mine) [Look] You look around.\n  }\n}\n");
        let diags = check(FileId(0), &hir);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn native_sticky_choice_is_never_flagged() {
        let hir =
            lower_native("flow f() {\n  {?\n    + (again) [Look again] Still a garden.\n  }\n}\n");
        let diags = check(FileId(0), &hir);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn native_fallback_choice_is_never_flagged() {
        let hir = lower_native(
            "flow f() {\n  {?\n    * [Look] You look around.\n    + (again) [Look again] Still a garden.\n    else { Nothing left to do. }\n  }\n}\n",
        );
        let diags = check(FileId(0), &hir);
        assert_eq!(
            codes(&diags),
            vec![DiagnosticCode::E157],
            "only the unlabeled once-only `Look` choice, never the sticky \
             `Look again` or the fallback: {diags:?}"
        );
    }
}
