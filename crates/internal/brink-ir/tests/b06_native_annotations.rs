//! The native `@[…]` per-declaration annotation channel (issue #1563).
//!
//! Before this slice only the file-level `@[was("old::path")]` record
//! lowered; every other annotation — `@[effects(pure)]` above a `fn` most of
//! all — hit `lower_native`'s catch-all and hard-failed the compile with
//! `E129`. These tests pin the wired behaviour: the ruled `effects` tenant
//! populates `Knot`/`Stitch::effects_assertion` at both container levels,
//! and every unruled or misplaced annotation is diagnosed exactly once and
//! never lowered to content.
//!
//! Integration test rather than an in-`src` `mod tests` for the same reason
//! `b06_native_declarations.rs` is one: it links the already-built
//! `brink-ir` rlib, so `brink-analyzer` (a dev-dependency that depends back
//! on `brink-ir`) stays type-compatible.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_ir::hir::lower_native;
use brink_ir::{DiagnosticCode, EffectsAssertion, FileId, HirFile};

fn lower(src: &str) -> (HirFile, Vec<brink_ir::Diagnostic>) {
    let parse = brink_syntax_native::parse(src);
    assert!(
        parse.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parse.errors()
    );
    let (hir, _manifest, diags) = lower_native::lower(FileId(0), &parse.tree());
    (hir, diags)
}

fn codes(diags: &[brink_ir::Diagnostic]) -> Vec<DiagnosticCode> {
    diags.iter().map(|d| d.code).collect()
}

/// The assertion on the file's first knot, which must exist.
fn knot_assertion(src: &str) -> EffectsAssertion {
    let (hir, diags) = lower(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    hir.knots[0]
        .effects_assertion
        .clone()
        .expect("the declaration carries an `@[effects(…)]` assertion")
}

// ── The ruled tenant: `@[effects(…)]` ────────────────────────────────

/// The headline regression: `@[effects(pure)]` above an `fn` used to be a
/// hard `E129` compile failure. It now lowers to the assertion.
#[test]
fn effects_above_a_fn_lowers_instead_of_failing() {
    let a = knot_assertion("@[effects(pure)]\nfn heal(amount) {\n  return;\n}\n");
    assert!(a.pure);
    assert!(!a.silent && !a.total);
    assert!(a.reads.is_empty() && a.writes.is_empty() && a.calls.is_empty());
}

/// The same channel on a `flow` head, and every clause the ruled paren
/// grammar admits — `reads`/`writes`/`calls` each naming several cells, in
/// source order, alongside the bare flags.
#[test]
fn effects_above_a_flow_carries_flags_and_every_clause() {
    let a = knot_assertion(
        "@[effects(silent, total, reads(gold, hp), writes(mood), calls(sfx))]\n\
         flow garden() {\n  Petals.\n}\n",
    );
    assert!(!a.pure);
    assert!(a.silent && a.total);
    assert_eq!(a.reads, ["gold", "hp"]);
    assert_eq!(a.writes, ["mood"]);
    assert_eq!(a.calls, ["sfx"]);
}

/// A nested `flow` becomes a `Stitch`, and its own annotation attaches to
/// it — the second container level, not just the top one.
#[test]
fn effects_above_a_nested_flow_attaches_to_the_stitch() {
    let (hir, diags) = lower(
        "flow garden() {\n\
         \x20 Petals.\n\
         \x20 @[effects(total)]\n\
         \x20 flow gate() {\n    Creak.\n  }\n\
         }\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(
        hir.knots[0].effects_assertion.is_none(),
        "the enclosing knot must not absorb its stitch's annotation"
    );
    let stitch = &hir.knots[0].stitches[0];
    assert_eq!(stitch.name.text, "gate");
    assert!(stitch.effects_assertion.as_ref().expect("present").total);
}

/// Doc comments and blank lines between the annotation and the head do not
/// break attachment (Rust's attribute rules, the native surface's north
/// star), and the doc still lands on the container.
#[test]
fn doc_comments_and_blank_lines_do_not_break_attachment() {
    let (hir, diags) = lower("@[effects(pure)]\n\n/// Heals.\nfn heal() {\n  return;\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(
        hir.knots[0]
            .effects_assertion
            .as_ref()
            .expect("present")
            .pure
    );
    assert!(hir.knots[0].doc.is_some(), "the `///` doc still attaches");
}

/// An unannotated declaration carries no assertion — the channel is opt-in,
/// and nothing is fabricated.
#[test]
fn an_unannotated_declaration_has_no_assertion() {
    let (hir, diags) = lower("fn heal() {\n  return;\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(hir.knots[0].effects_assertion.is_none());
}

// ── Grammar diagnostics ──────────────────────────────────────────────

/// A bare `@[effects]` and an empty `@[effects()]` both assert nothing
/// (`E100`), matching the ink channel.
#[test]
fn an_empty_assertion_is_e100() {
    for src in [
        "@[effects]\nfn heal() {\n  return;\n}\n",
        "@[effects()]\nfn heal() {\n  return;\n}\n",
    ] {
        let (hir, diags) = lower(src);
        assert_eq!(codes(&diags), vec![DiagnosticCode::E100], "{src:?}");
        assert!(hir.knots[0].effects_assertion.is_none(), "{src:?}");
    }
}

/// Every malformed-argument shape the ruled grammar rejects is `E101`: an
/// unknown flag, an unknown clause name, a `pure` contradicted by a clause
/// that grants state atoms, and non-identifier clause/flag payloads.
#[test]
fn malformed_arguments_are_e101() {
    for src in [
        // Unknown bare flag.
        "@[effects(clean)]\nfn heal() {\n  return;\n}\n",
        // Unknown clause name (flags never take parentheses).
        "@[effects(touches(gold))]\nfn heal() {\n  return;\n}\n",
        // `pure` asserts the EMPTY state row — a clause contradicts it.
        "@[effects(pure, reads(gold))]\nfn heal() {\n  return;\n}\n",
        // A literal in flag position.
        "@[effects(80)]\nfn heal() {\n  return;\n}\n",
        // A literal inside a clause.
        "@[effects(reads(80))]\nfn heal() {\n  return;\n}\n",
        // A `::`-path in flag position (the `@[was]` arg shape).
        "@[effects(story::old)]\nfn heal() {\n  return;\n}\n",
        // A clause nested inside a clause.
        "@[effects(reads(writes(gold)))]\nfn heal() {\n  return;\n}\n",
    ] {
        let (hir, diags) = lower(src);
        assert_eq!(codes(&diags), vec![DiagnosticCode::E101], "{src:?}");
        assert!(hir.knots[0].effects_assertion.is_none(), "{src:?}");
    }
}

/// A second `@[effects]` on one declaration is `E048`; the first wins.
#[test]
fn a_duplicate_assertion_is_e048_and_the_first_wins() {
    let (hir, diags) = lower("@[effects(pure)]\n@[effects(silent)]\nfn heal() {\n  return;\n}\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E048]);
    let a = hir.knots[0].effects_assertion.as_ref().expect("present");
    assert!(a.pure && !a.silent);
}

// ── The reserved-namespace rule ──────────────────────────────────────

/// An unrecognized annotation name is `E111` wherever it appears — the
/// `@` namespace is fully reserved (`docs/directive-annotations-spec.md`
/// §1.1), so a typo can never become a silent no-op. `@[element]` and
/// `@[style]` are ruled *features* but have no lowering yet, and land here
/// too rather than being invented.
#[test]
fn an_unknown_annotation_name_is_e111() {
    for src in [
        "@[bogus]\nfn heal() {\n  return;\n}\n",
        "@[element(challenge)]\nfn heal() {\n  return;\n}\n",
        "@[style(uppercase)]\nfn heal() {\n  return;\n}\n",
        // The tag-channel directive names do not alias into this channel.
        "@[local]\nvar hp = 10\n",
    ] {
        let (_hir, diags) = lower(src);
        assert_eq!(codes(&diags), vec![DiagnosticCode::E111], "{src:?}");
    }
}

/// A recognized name outside its placement is `E112`: `effects` on anything
/// that is not a `flow`/`fn` head or with nothing following it at all, and
/// the file-level-only `was` record anywhere but file level.
#[test]
fn a_misplaced_annotation_is_e112() {
    for src in [
        // Not a callable head.
        "@[effects(pure)]\nvar hp = 10\n",
        "@[effects(pure)]\nstruct Npc {\n  hp: int\n}\n",
        // Nothing follows at all.
        "fn heal() {\n  return;\n}\n@[effects(pure)]\n",
        // Loose in a body, attached to nothing.
        "flow garden() {\n  @[effects(pure)]\n  Petals.\n}\n",
        // `@[was]` is the module record — file level only.
        "flow garden() {\n  @[was(story::old)]\n  Petals.\n}\n",
    ] {
        let (_hir, diags) = lower(src);
        assert_eq!(codes(&diags), vec![DiagnosticCode::E112], "{src:?}");
    }
}

/// The file-level `@[was]` record still lowers, and is not caught by the
/// new chokepoint — the shipped tenant must not regress.
#[test]
fn the_file_level_was_record_is_still_consumed() {
    let (hir, diags) = lower("@[was(\"story::old::path\")]\nflow main() {\n  Hi.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(
        hir.module.expect("module record").was.expect("was").0,
        "story::old::path"
    );
}

// ── Erasure ──────────────────────────────────────────────────────────

/// An annotation line is *consumed*, never content: neither a well-placed
/// one nor a misplaced one may contribute a `Stmt` to the body it sits in.
#[test]
fn an_annotation_line_never_lowers_to_content() {
    let (hir, _diags) = lower(
        "flow garden() {\n\
         \x20 @[effects(pure)]\n\
         \x20 @[bogus]\n\
         \x20 Petals.\n\
         }\n",
    );
    let rendered = format!("{:?}", hir.knots[0].body);
    assert!(
        !rendered.contains("effects") && !rendered.contains("bogus"),
        "annotation text leaked into the body: {rendered}"
    );
}
