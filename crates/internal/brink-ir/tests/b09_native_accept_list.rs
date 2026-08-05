//! B0.9 exit-criterion tests: the native `.brink` **accept-list** admission
//! gate (`docs/hir-admission-contract.md` §4.4/§5 Q6, `docs/b0-sequencing.md`
//! §B0.9, issue #1179).
//!
//! Lives as an integration test for the same reason `b06_native_declarations.rs`/
//! `b07_native_body.rs`/`b08_native_control_flow.rs` do (see those files'
//! module docs): the gate under test (`brink_analyzer::validate_native_accept_list`)
//! needs `brink-analyzer`, a dev-dependency that itself depends on `brink-ir`.
//!
//! # Reachability, honestly
//!
//! Every fixture below is real `.brink` source lowered through the actual
//! `lower_native::lower` entry point (the same one `brink-db`'s
//! `lower_native_file` calls at the compile seam) — not a hand-built
//! `HirFile`. The "off-list" tests use the #672-A posture
//! (`brink-analyzer/src/admission.rs`'s own test module doc): lower real,
//! valid `.brink` source, then corrupt exactly the one field the check under
//! test cares about, so the corruption stands in for "a buggy frontend (or a
//! future B0.x slice) produced this shape by mistake" — the exact failure
//! mode a defense-in-depth admission gate exists to catch, distinct from
//! `E129`'s already-loud CST→HIR-seam rejection of constructs that never
//! reach HIR at all.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_ir::hir::lower_native;
use brink_ir::provenance::NodeClass;
use brink_ir::{
    Block, Content, Diagnostic, DiagnosticCode, Divert, DivertPath, DivertTarget, FileId, HirFile,
    IncludeSite, Name, Path, Provenance, Stmt, ThreadStart,
};

fn lower_native_fixture(src: &str) -> HirFile {
    let parse = brink_syntax_native::parse(src);
    assert!(
        parse.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parse.errors()
    );
    let (hir, _manifest, diags) = lower_native::lower(FileId(0), &parse.tree());
    assert!(
        diags.is_empty(),
        "unexpected lowering diagnostics: {diags:?}"
    );
    hir
}

fn codes(diags: &[Diagnostic]) -> Vec<DiagnosticCode> {
    diags.iter().map(|d| d.code).collect()
}

fn synthetic_range() -> rowan::TextRange {
    rowan::TextRange::new(0.into(), 1.into())
}

fn synthetic_target(name: &str) -> DivertTarget {
    let range = synthetic_range();
    DivertTarget {
        path: DivertPath::Path(Path {
            segments: vec![Name {
                text: name.to_string(),
                range,
            }],
            range,
            crosses_module_wall: false,
        }),
        args: Vec::new(),
    }
}

// ─── On-list constructs pass clean ─────────────────────────────────────

/// A plain native flow body — no `main` entry, no choices — is the baseline
/// on-list shape.
#[test]
fn clean_file_has_no_accept_list_diagnostics() {
    let hir = lower_native_fixture("flow greet() {\n  Hello, world.\n}\n");
    let diags = brink_analyzer::validate_native_accept_list(FileId(0), &hir);
    assert!(diags.is_empty(), "expected a clean file, got: {diags:?}");
}

/// The one documented `root_content` exception — the synthesized `flow
/// main()` entry divert (decision-log 2026-07-21 "Native story-entry
/// convention") — must also pass clean: proof the check distinguishes the
/// ruled exception from ink-only baggage rather than rejecting every
/// nonempty `root_content`.
#[test]
fn synthesized_main_entry_divert_is_on_the_accept_list() {
    let hir = lower_native_fixture("flow main() {\n  Hello, world.\n}\n");
    assert_eq!(
        hir.root_content.stmts.len(),
        1,
        "sanity: main() should synthesize a single root_content divert"
    );
    let diags = brink_analyzer::validate_native_accept_list(FileId(0), &hir);
    assert!(
        diags.is_empty(),
        "the synthesized main-entry divert must be accept-list-clean, got: {diags:?}"
    );
}

/// A native `{? … }` choice point's `ChoiceSet` (the on-list "selector"
/// shape B0.7 lowers to, stamped with the documented-neutral weave-fold
/// values) must pass clean.
#[test]
fn native_choice_point_is_on_the_accept_list() {
    let hir = lower_native_fixture(
        "flow pick() {\n  {?\n    * Left -> left\n    * Right -> right\n  }\n}\nflow left() {\n}\nflow right() {\n}\n",
    );
    let diags = brink_analyzer::validate_native_accept_list(FileId(0), &hir);
    assert!(diags.is_empty(), "expected a clean file, got: {diags:?}");
}

/// A splice trailing a choice's own body (`* Go\n<- elsewhere()`) is the
/// legal "after a choice line" `ThreadStart` position and must not be
/// flagged.
#[test]
fn thread_start_trailing_a_choice_body_is_legal() {
    let hir = lower_native_fixture(
        "flow pick() {\n  {?\n    * Go\n    <- elsewhere()\n  }\n}\nflow elsewhere() {\n}\n",
    );
    let Stmt::ChoiceSet(cs) = &hir.knots[0].body.stmts[0] else {
        panic!("expected a ChoiceSet, got: {:?}", hir.knots[0].body.stmts);
    };
    assert!(
        matches!(cs.choices[0].body.stmts.last(), Some(Stmt::ThreadStart(_))),
        "sanity: expected a trailing ThreadStart in the choice body: {:?}",
        cs.choices[0].body
    );
    let diags = brink_analyzer::validate_native_accept_list(FileId(0), &hir);
    assert!(diags.is_empty(), "expected a clean file, got: {diags:?}");
}

/// A splice preceding every choice line (`<- elsewhere()\n* Go`) is the
/// legal "preamble" `ThreadStart` position and must not be flagged.
#[test]
fn thread_start_preceding_a_choice_set_is_legal() {
    let hir = lower_native_fixture(
        "flow pick() {\n  {?\n    <- elsewhere()\n    * Go\n  }\n}\nflow elsewhere() {\n}\n",
    );
    assert!(
        matches!(hir.knots[0].body.stmts[0], Stmt::ThreadStart(_)),
        "sanity: expected a preamble ThreadStart before the ChoiceSet: {:?}",
        hir.knots[0].body.stmts
    );
    assert!(matches!(hir.knots[0].body.stmts[1], Stmt::ChoiceSet(_)));
    let diags = brink_analyzer::validate_native_accept_list(FileId(0), &hir);
    assert!(diags.is_empty(), "expected a clean file, got: {diags:?}");
}

// ─── Off-list constructs are refused, loudly ───────────────────────────
//
// #672-A posture: real, valid `.brink` source lowered through the real
// pipeline, then corrupted at exactly the one field under test.

/// Ink-only content injected into `root_content` (never a shape native
/// lowering itself produces) is refused, never silently accepted.
#[test]
fn e133_ink_only_content_in_root_content_is_refused() {
    let mut hir = lower_native_fixture("flow greet() {\n  Hello, world.\n}\n");
    hir.root_content = Block::from_stmts(vec![Stmt::Content(Content {
        ptr: None,
        parts: Vec::new(),
        tags: Vec::new(),
    })]);
    let diags = brink_analyzer::validate_native_accept_list(FileId(0), &hir);
    assert!(codes(&diags).contains(&DiagnosticCode::E133), "{diags:?}");
}

/// A source-backed divert (`ptr: Some(_)`) in `root_content` is not the
/// documented synthesized-entry shape and must still be refused, even
/// though it is structurally "just a Divert" like the legal case — proof
/// the check compares against the exact synthesized shape, not merely
/// "one Divert statement".
#[test]
fn e133_source_backed_divert_in_root_content_is_refused() {
    let mut hir = lower_native_fixture("flow greet() {\n  Hello, world.\n}\n");
    hir.root_content = Block::from_stmts(vec![Stmt::Divert(Divert {
        ptr: Some(Provenance::synthetic(NodeClass::Divert, synthetic_range())),
        target: synthetic_target("greet"),
    })]);
    let diags = brink_analyzer::validate_native_accept_list(FileId(0), &hir);
    assert!(codes(&diags).contains(&DiagnosticCode::E133), "{diags:?}");
}

/// A native `HirFile` carrying an `IncludeSite` — a shape `lower_native`
/// never produces (native has no textual `INCLUDE` graph) — is refused.
#[test]
fn e134_include_site_is_refused() {
    let mut hir = lower_native_fixture("flow greet() {\n  Hello, world.\n}\n");
    hir.includes.push(IncludeSite {
        file_path: "other.ink".to_string(),
        ptr: Provenance::synthetic(NodeClass::Divert, synthetic_range()),
    });
    let diags = brink_analyzer::validate_native_accept_list(FileId(0), &hir);
    assert!(codes(&diags).contains(&DiagnosticCode::E134), "{diags:?}");
}

/// An ambient `ThreadStart` — spliced directly into a knot body, neither
/// preceding a `ChoiceSet` nor trailing a `Choice`'s own body — is refused.
#[test]
fn e135_ambient_thread_start_is_refused() {
    let mut hir = lower_native_fixture("flow greet() {\n  Hello, world.\n}\n");
    hir.knots[0].body.stmts.push(Stmt::ThreadStart(ThreadStart {
        ptr: Provenance::synthetic(NodeClass::Divert, synthetic_range()),
        target: synthetic_target("greet"),
    }));
    let diags = brink_analyzer::validate_native_accept_list(FileId(0), &hir);
    assert!(codes(&diags).contains(&DiagnosticCode::E135), "{diags:?}");
}

/// A `ChoiceSet` stamped with a non-neutral weave-fold value (as a real ink
/// weave fold would produce, never native) is refused.
#[test]
fn e136_non_neutral_weave_fold_is_refused() {
    let mut hir =
        lower_native_fixture("flow pick() {\n  {?\n    * Left -> left\n  }\n}\nflow left() {\n}\n");
    let Stmt::ChoiceSet(cs) = &mut hir.knots[0].body.stmts[0] else {
        panic!("expected a ChoiceSet, got: {:?}", hir.knots[0].body.stmts);
    };
    cs.depth = 1;
    let diags = brink_analyzer::validate_native_accept_list(FileId(0), &hir);
    assert!(codes(&diags).contains(&DiagnosticCode::E136), "{diags:?}");
}
