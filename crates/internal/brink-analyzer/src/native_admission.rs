//! The native `.brink` **accept-list** admission gate (Q6(b),
//! `docs/hir-admission-contract.md` §4.4/§5 Q6, `docs/b0-sequencing.md`
//! §B0.9, issue #1179).
//!
//! The inverse of the ink [`crate::dialect_gate`]: that gate is a
//! **reject-list** — "these brink-extension nodes are illegal under
//! strict-ink" — layered on top of a shared grammar every ink file already
//! satisfies by construction. The native surface has no such shared grammar
//! to lean on: it is its own frontend (B0.5–B0.8), producing the same
//! frontend-agnostic HIR the ink frontend does, and nothing at the type
//! level stops a bug in `hir::lower_native` from emitting a shape only the
//! ink frontend should ever produce. [`validate_native_accept_list`] closes
//! that gap: it enumerates the HIR shapes a well-formed native lowering is
//! allowed to produce and **refuses** — loudly, never silently — anything
//! else, at the same non-suppressible seam `brink-db`'s `lower_native_file`
//! runs [`crate::validate_admission`] at (B0.3, NF-6's always-on posture).
//!
//! Keyed off the producing frontend **at the pipeline level** (the caller —
//! `brink-db`'s `Language::Native` dispatch — decides when to run this),
//! never from a tag carried on the tree itself (F-I#10): this pass is
//! written against native's own documented HIR shapes and must never run
//! against ink-produced HIR (ink's own `root_content`/`includes` usage is
//! exactly the baggage this gate exists to reject).
//!
//! # What is checked
//!
//! 1. **`root_content` ink-only baggage** (`E133`) — native's tree-is-the-
//!    universe model has no root weave the way ink's pre-first-knot content
//!    does; `hir::lower_native::lower` stamps `root_content` empty for
//!    every file *except* one documented, narrow exception: the `flow
//!    main()` entry convention (maintainer-ruled 2026-07-21,
//!    `docs/decision-log.md` "Native story-entry convention") synthesizes a
//!    single, unconditional, `ptr: None` divert to `main` there. That
//!    synthesized shape — and only that shape — is accepted; any other
//!    `root_content` (real weave content, more than one statement, a
//!    source-backed divert) is ink-only baggage a native file must never
//!    carry.
//!
//!    **Documentation note**: `docs/b0-sequencing.md` §B0.6 and
//!    `docs/hir-admission-contract.md` §4.4 both describe this slice's
//!    exit bar as simply "`root_content` = `Block::default()` … always" —
//!    written 2026-07-19, before the 2026-07-21 entry-convention ruling
//!    introduced the one legitimate exception. This gate follows landed
//!    reality (the ruling postdates, and narrows, the older wording)
//!    rather than the now-stale text; flagged in the issue #1179 PR for a
//!    documentation fix rather than silently diverging from the written
//!    contract.
//! 2. **`IncludeSite` ink-only baggage** (`E134`) — native has no textual
//!    `INCLUDE` graph (charter §13.2, "the tree is the compilation
//!    universe"); `hir::lower_native::lower` always leaves `includes`
//!    empty, so any entry here is ink-only baggage that reached native HIR
//!    some other way.
//! 3. **Ambient `ThreadStart`** (`E135`) — B0.7's splice lowering
//!    (`hir::lower_native::choice::lower_choice_point`) places a `<-
//!    flow(args)` splice's `ThreadStart` in exactly two legal positions:
//!    immediately preceding the `ChoiceSet` it preambles (a splice reached
//!    before any choice line), or as the trailing statement(s) of a
//!    `Choice`'s own body (a splice reached after a choice line). A
//!    `ThreadStart` anywhere else in the tree is "ambient" — exactly the
//!    shape charter §11's scoped-splices-only narrowing rules out — and is
//!    refused.
//! 4. **Non-neutral weave-fold values** (`E136`) — native has no weave fold
//!    to report `depth`/`context` from; every native `ChoiceSet` stamps the
//!    B0.7-documented neutral values uniformly (`depth = 0`, `context =
//!    Inline` — `docs/hir-admission-contract.md` §3 D4). Any other value on
//!    a native `ChoiceSet` means a real weave-fold concept leaked in from
//!    somewhere it shouldn't have.
//!
//! # What is deliberately NOT checked here
//!
//! Everything B0.3's [`crate::validate_admission`] already covers (manifest
//! ⇄ HIR agreement, range well-formedness, name conventions, control-flow
//! classification, provenance-kind consistency) — this gate is native-only
//! and additive, not a replacement; `brink-db` runs both at the same seam.
//! Not-yet-lowered constructs at the CST→HIR seam itself (a lambda
//! expression, a `module { … }` block, depth-3 `flow` nesting, an
//! out-of-position `struct`/`extern`/`use`/`import`, …) are already loud at
//! that seam (`E129`/`E130`, §4.4's "parse, emit ruled-but-not-yet-lowered,
//! never drop" posture): by the time this gate runs, such constructs have
//! either produced their own diagnostic and been skipped, or don't exist as
//! a distinguishable HIR shape left to check for post hoc.

use rowan::{TextRange, TextSize};

use brink_ir::hir::{Block, ChoiceSet, ChoiceSetContext, DivertPath, HirFile, Stmt};
use brink_ir::{Diagnostic, DiagnosticCode, FileId, Provenance};

/// Run the B0.9 accept-list over one native file's already-lowered
/// [`HirFile`]. Non-suppressible, mirroring [`crate::validate_admission`] —
/// callers must not route the result through `apply_suppressions`. Only
/// meaningful for HIR the native frontend produced; never call this against
/// ink-produced HIR (see the module doc).
#[must_use]
pub fn validate_native_accept_list(file_id: FileId, hir: &HirFile) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    check_root_content(file_id, &hir.root_content, &mut diags);

    for site in &hir.includes {
        diags.push(diag(file_id, site.ptr.text_range(), DiagnosticCode::E134));
    }

    // Ambient `ThreadStart` (check 3) and non-neutral weave-fold values
    // (check 4) both require a tree walk, so they share one pass.
    walk_block(file_id, &hir.root_content, false, &mut diags);
    for knot in &hir.knots {
        walk_block(file_id, &knot.body, false, &mut diags);
        for stitch in &knot.stitches {
            walk_block(file_id, &stitch.body, false, &mut diags);
        }
    }

    diags
}

fn diag(file: FileId, range: TextRange, code: DiagnosticCode) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: code.title().to_string(),
        code,
    }
}

/// Check 1: `root_content` is either empty, or exactly the synthesized
/// `flow main()` entry divert `hir::lower_native::entry_root_content`
/// produces — see the module doc's documentation note.
fn check_root_content(file_id: FileId, block: &Block, diags: &mut Vec<Diagnostic>) {
    let legal = match block.stmts.as_slice() {
        [] => true,
        [Stmt::Divert(d)] => {
            d.ptr.is_none()
                && d.target.args.is_empty()
                && matches!(d.target.path, DivertPath::Path(_))
        }
        _ => false,
    };
    if !legal {
        let range = block
            .stmts
            .first()
            .and_then(best_effort_range)
            .unwrap_or_else(|| TextRange::new(TextSize::from(0), TextSize::from(0)));
        diags.push(diag(file_id, range, DiagnosticCode::E133));
    }
}

/// Best-effort source range for a `root_content` diagnostic's position —
/// diagnostic-quality only, never correctness (an unrecognized shape still
/// falls back to a zero-length range at the file start rather than
/// suppressing the check).
fn best_effort_range(stmt: &Stmt) -> Option<TextRange> {
    match stmt {
        Stmt::Content(c) => c.ptr.as_ref().map(Provenance::text_range),
        Stmt::Divert(d) => d.ptr.as_ref().map(Provenance::text_range),
        Stmt::ChoiceSet(cs) => cs.choices.first().map(|c| c.ptr.text_range()),
        _ => None,
    }
}

/// Checks 3 (ambient `ThreadStart`) and 4 (non-neutral weave-fold values)
/// over one block's statements, recursing into every nested block a native
/// lowering can produce: a `ChoiceSet`'s continuation and each choice's own
/// body, a `LabeledBlock`, and each `Conditional`/`Sequence` branch.
/// `in_choice_body` is `true` only when `block` is a `Choice`'s own body —
/// the one position where a *trailing* run of `ThreadStart`s is legal.
fn walk_block(file_id: FileId, block: &Block, in_choice_body: bool, diags: &mut Vec<Diagnostic>) {
    let stmts = &block.stmts;
    for (i, stmt) in stmts.iter().enumerate() {
        match stmt {
            Stmt::ThreadStart(ts) => {
                if !thread_start_is_legal(stmts, i, in_choice_body) {
                    diags.push(diag(file_id, ts.ptr.text_range(), DiagnosticCode::E135));
                }
            }
            Stmt::ChoiceSet(cs) => {
                check_weave_fold(file_id, cs, diags);
                walk_block(file_id, &cs.continuation, false, diags);
                for choice in &cs.choices {
                    walk_block(file_id, &choice.body, true, diags);
                }
            }
            Stmt::LabeledBlock(b) => walk_block(file_id, b, false, diags),
            Stmt::Conditional(c) => {
                for branch in &c.branches {
                    walk_block(file_id, &branch.body, false, diags);
                }
            }
            Stmt::Sequence(s) => {
                for branch in &s.branches {
                    walk_block(file_id, &branch.body, false, diags);
                }
            }
            Stmt::Content(_)
            | Stmt::Divert(_)
            | Stmt::TunnelCall(_)
            | Stmt::TempDecl(_)
            | Stmt::Assignment(_)
            | Stmt::Return(_)
            | Stmt::ExprStmt(_)
            | Stmt::EndOfLine
            | Stmt::LogicBlock(_)
            | Stmt::Await(_)
            | Stmt::AttachElement(_)
            | Stmt::EndElementRun => {}
        }
    }
}

/// Legitimate iff the `ThreadStart` at `stmts[i]` either heads a run that
/// leads straight into a `ChoiceSet` (the splice-preamble position:
/// `hir::lower_native::choice::lower_choice_point`'s `preamble` field), or
/// `in_choice_body` and every remaining statement from `i` onward is itself
/// a `ThreadStart` (the splice-after-a-choice-line position: that
/// function's `choices.last_mut()` append arm).
fn thread_start_is_legal(stmts: &[Stmt], i: usize, in_choice_body: bool) -> bool {
    let mut j = i;
    while matches!(stmts.get(j), Some(Stmt::ThreadStart(_))) {
        j += 1;
    }
    if matches!(stmts.get(j), Some(Stmt::ChoiceSet(_))) {
        return true;
    }
    in_choice_body && stmts[i..].iter().all(|s| matches!(s, Stmt::ThreadStart(_)))
}

/// Check 4: a native `ChoiceSet` must carry the B0.7-documented neutral
/// weave-fold values uniformly.
fn check_weave_fold(file_id: FileId, cs: &ChoiceSet, diags: &mut Vec<Diagnostic>) {
    if cs.context != ChoiceSetContext::Inline || cs.depth != 0 {
        let range = cs.choices.first().map_or_else(
            || TextRange::new(TextSize::from(0), TextSize::from(0)),
            |c| c.ptr.text_range(),
        );
        diags.push(diag(file_id, range, DiagnosticCode::E136));
    }
}

// ─── Tests ──────────────────────────────────────────────────────────
//
// This crate has no dependency on `brink-syntax-native` (only `brink-ir`
// does), so tests that need to lower real `.brink` source through
// `hir::lower_native::lower` before checking it here live as an
// integration test in `brink-ir/tests/b09_native_accept_list.rs` instead —
// the same reason `b06_native_declarations.rs`/`b07_native_body.rs`/
// `b08_native_control_flow.rs` do (see those files' module docs):
// `brink-ir` already dev-depends on `brink-analyzer`, so the reverse
// dependency direction this needs is free there, not here.
