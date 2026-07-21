//! B0.7 exit-criterion tests: native `.brink` prose-dialect **body**
//! lowering (`docs/b0-sequencing.md` §B0.7, issue #1176).
//!
//! Lives as an integration test for the same reason `b06_native_declarations.rs`
//! does (see that file's module doc): admission checking needs
//! `brink-analyzer`, a dev-dependency that itself depends on `brink-ir`, so
//! an in-`lib` unit test would produce two non-interchangeable `brink_ir`
//! instances.
//!
//! # The gate, honestly
//!
//! The flagship "respelled-differential episode test" the B0.7 slice spec
//! asks for needs a native CST → HIR → LIR → codegen → runtime path.
//! `crates/brink-compiler/Cargo.toml` does not depend on
//! `brink-syntax-native` at all — the native front end is not wired into
//! the compiler driver (that wiring is its own slice; grepped to confirm
//! before writing this file). So the episode-level differential is
//! **blocked**, not attempted here. What this file *does* prove, per the
//! B0.7 spec's own fallback instruction ("build the HIR-differential gate,
//! and clearly report... episode-level differential is blocked"):
//!
//! 1. `admission_clean_for_a_body_exercising_every_construct_group` — a
//!    single fixture exercising every construct group B0.7 owns (content/
//!    glue/interpolation/tags, conditional, sequence, choice set/fallback/
//!    splice, dissolved gather, diverts/tunnels/labels, return/tunnel-
//!    redirect) lowers with zero diagnostics AND passes the B0.3 admission
//!    validator with zero diagnostics.
//! 2. `cross_frontend_choice_shape_matches_ink` /
//!    `cross_frontend_conditional_shape_matches_ink` — true differential
//!    tests: the same semantic content, authored once in ink and once in
//!    its native respelling, lowered through each frontend's own `lower_*`
//!    entry point, produces the same *shape* (variant sequence, sticky/
//!    once flags, branch counts, condition presence) modulo provenance
//!    (`Provenance`/`ptr`/`container_id`/`gather_id` are frontend-specific
//!    or pipeline-stamped-later, so they're excluded from the comparison,
//!    not because they don't matter but because they're not this test's
//!    job — `docs/hir-admission-contract.md` §1.3's "no dialect tag on any
//!    HIR node").
//! 3. `fogg_passage_exhibit_lowers_and_is_admission_clean` — the charter's
//!    own named exhibit (`tests/tier1-brink-respell/exhibit-fogg-passage/story.brink`)
//!    parses, lowers, and passes admission with zero diagnostics.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_ir::hir::lower_native;
use brink_ir::{FileId, Stmt};

fn lower_native_fixture(
    src: &str,
) -> (
    brink_ir::HirFile,
    brink_ir::SymbolManifest,
    Vec<brink_ir::Diagnostic>,
) {
    let parse = brink_syntax_native::parse(src);
    assert!(
        parse.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parse.errors()
    );
    let tree = parse.tree();
    lower_native::lower(FileId(0), &tree)
}

/// A single, real-shaped `flow` body exercising every B0.7 construct group.
const RICH_BODY_FIXTURE: &str = "\
flow start() {
  -> travel
}

flow travel() {
  We set out. <> #mood: hopeful
  {?
    * {if true} (rich) The scenic route. -> arrived
    + Stay on the highway.
    <- detour()
    else { Wing it. }
  }
  We continue on.
  (checkpoint)
  Reconverged here.
  {if true {
    All is well.
  } else {
    Something's wrong.
  }}
  {~ A passing bird. | A gust of wind.}
  -> onward ->
  return -> travel
}

flow detour() {
  {?
    * A detour choice.
  }
}

flow onward() {
  return
}
";

#[test]
fn admission_clean_for_a_body_exercising_every_construct_group() {
    let (hir, manifest, diags) = lower_native_fixture(RICH_BODY_FIXTURE);
    assert!(
        diags.is_empty(),
        "unexpected lowering diagnostics: {diags:?}"
    );

    let file_len = rowan::TextSize::of(RICH_BODY_FIXTURE);
    let admission_diags = brink_analyzer::validate_admission(FileId(0), &hir, &manifest, file_len);
    assert!(
        admission_diags.is_empty(),
        "native body HIR must pass B0.3 admission with zero diagnostics: {admission_diags:?}"
    );
}

#[test]
fn fogg_passage_exhibit_lowers_and_is_admission_clean() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/tier1-brink-respell/exhibit-fogg-passage/story.brink"
    ))
    .expect("exhibit-fogg-passage/story.brink must exist");

    let (hir, manifest, diags) = lower_native_fixture(&src);
    // The fixture opens with a top-level `-> fogg_wager` entry divert — a
    // native story-entry *convention*, not yet a ruled construct (its own
    // manifest.toml: "confirm as the ruled entry convention in the
    // G-batch, #1106"). `walk_top_level` (B0.6) has no dispatch for a bare
    // divert at file-root position — native's `root_content` equivalent is
    // hard `Block::default()` by contract, so this diagnoses E129 rather
    // than vanishing. That is the one diagnostic B0.7 legitimately
    // produces for this fixture; everything else (the three flow bodies,
    // the choice point, the dissolved gather, the `{if}`/`else`
    // conditional) must be clean.
    assert_eq!(
        diags.len(),
        1,
        "expected exactly the known top-level-entry-divert E129: {diags:?}"
    );
    assert_eq!(diags[0].code, brink_ir::DiagnosticCode::E129);
    let file_len = rowan::TextSize::of(src.as_str());
    let admission_diags = brink_analyzer::validate_admission(FileId(0), &hir, &manifest, file_len);
    assert!(
        admission_diags.is_empty(),
        "the Fogg passage exhibit must be admission-clean: {admission_diags:?}"
    );

    // A structural sanity check on the exhibit's own centerpiece: the
    // dissolved gather. `fogg_wager`'s body is `[Content, ChoiceSet]` (no
    // sibling after the choice point) with two choices, each diverting out
    // — the flagship shape the charter names for this exhibit.
    let fogg_wager = hir
        .knots
        .iter()
        .find(|k| k.name.text == "fogg_wager")
        .expect("fogg_wager knot");
    let choice_set = fogg_wager
        .body
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::ChoiceSet(cs) => Some(cs.as_ref()),
            _ => None,
        })
        .expect("fogg_wager must contain a ChoiceSet");
    assert_eq!(choice_set.choices.len(), 2);
}

// ─── Cross-frontend structural differential ───────────────────────────
//
// The same semantic content, authored once in ink and once in its native
// respelling. Compares *shape* (see the module doc's point 2) — this is
// the honest substitute for a full episode-identity check while the
// native front end isn't wired into the compiler driver.

fn stmt_kind(s: &Stmt) -> &'static str {
    match s {
        Stmt::Content(_) => "Content",
        Stmt::Divert(_) => "Divert",
        Stmt::TunnelCall(_) => "TunnelCall",
        Stmt::ThreadStart(_) => "ThreadStart",
        Stmt::TempDecl(_) => "TempDecl",
        Stmt::Assignment(_) => "Assignment",
        Stmt::Return(_) => "Return",
        Stmt::ChoiceSet(_) => "ChoiceSet",
        Stmt::LabeledBlock(_) => "LabeledBlock",
        Stmt::Conditional(_) => "Conditional",
        Stmt::Sequence(_) => "Sequence",
        Stmt::ExprStmt(_) => "ExprStmt",
        Stmt::EndOfLine => "EndOfLine",
        Stmt::LogicBlock(_) => "LogicBlock",
        Stmt::Await(_) => "Await",
    }
}

fn stmt_shape(stmts: &[Stmt]) -> Vec<&'static str> {
    stmts.iter().map(stmt_kind).collect()
}

fn lower_ink_knot(src: &str) -> brink_ir::Knot {
    use brink_syntax::ast::AstNode as _;

    let parse = brink_syntax::parse(src);
    assert!(
        parse.errors().is_empty(),
        "ink fixture must parse cleanly: {:?}",
        parse.errors()
    );
    let tree = parse.tree();
    let knot_ast = tree
        .syntax()
        .children()
        .find_map(brink_syntax::ast::KnotDef::cast)
        .expect("ink fixture must contain one knot");
    let (knot, diags) = brink_ir::hir::lower::lower_single_knot(FileId(0), &knot_ast);
    assert!(
        diags.is_empty(),
        "unexpected ink lowering diagnostics: {diags:?}"
    );
    knot.expect("knot must lower")
}

#[test]
fn cross_frontend_choice_shape_matches_ink() {
    let ink_src = "\
== choices ==
Pick one.
* Once choice.
+ Sticky choice.
* -> elsewhere
- (again) Reconverged.
-> END

== elsewhere ==
Elsewhere.
-> END
";
    let native_src = "\
flow choices() {
  Pick one.
  {?
    * Once choice.
    + Sticky choice.
    else { -> elsewhere }
  }
  (again) Reconverged.
}

flow elsewhere() {
  Elsewhere.
}
";
    let ink_knot = lower_ink_knot(ink_src);
    let (native_hir, _m, diags) = lower_native_fixture(native_src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let native_knot = native_hir
        .knots
        .iter()
        .find(|k| k.name.text == "choices")
        .expect("choices knot");

    // Both: [Content, EndOfLine, ChoiceSet].
    assert_eq!(
        stmt_shape(&ink_knot.body.stmts),
        stmt_shape(&native_knot.body.stmts)
    );

    let ink_cs = ink_knot
        .body
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::ChoiceSet(cs) => Some(cs.as_ref()),
            _ => None,
        })
        .expect("ink ChoiceSet");
    let native_cs = native_knot
        .body
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::ChoiceSet(cs) => Some(cs.as_ref()),
            _ => None,
        })
        .expect("native ChoiceSet");

    // The third choice is a FALLBACK on both sides, reached through different
    // surfaces: ink's implicit-fallback idiom (`* -> x`, a choice with no
    // display text that only diverts → is_fallback: true) corresponds to
    // native's explicit `else { -> x }` (also is_fallback: true). Native does
    // not overload bare `* -> x` as fallback — it uses `else`. The is_fallback
    // assertion below guards this correspondence (#1176 review).
    assert_eq!(ink_cs.choices.len(), native_cs.choices.len());
    for (ink_c, native_c) in ink_cs.choices.iter().zip(native_cs.choices.iter()) {
        assert_eq!(
            ink_c.is_sticky, native_c.is_sticky,
            "sticky/once flag must match: ink={ink_c:?} native={native_c:?}"
        );
        assert_eq!(
            ink_c.is_fallback, native_c.is_fallback,
            "fallback flag must match: ink={ink_c:?} native={native_c:?}"
        );
    }
    // Both gathers are labeled "again" and attach directly to the
    // continuation (not a nested LabeledBlock) on both frontends.
    assert_eq!(
        ink_cs.continuation.label.as_ref().map(|n| n.text.as_str()),
        Some("again")
    );
    assert_eq!(
        native_cs
            .continuation
            .label
            .as_ref()
            .map(|n| n.text.as_str()),
        Some("again")
    );
}

/// Cross-frontend finding (see `cond::lower_conditional`'s doc comment): a
/// simple native `{if cond {…} else {…}}` must compare against ink's own
/// natural spelling of the same shape — `{cond: body - else: body2}`
/// (`ConditionalWithExpr` + a branchless first body) — which is what a
/// real writer authors for "if X then A else B" and what B0.8b's future
/// mechanical converter would actually emit. That ink shape lowers to
/// `CondKind::InitialCondition`, not `IfElse` (`IfElse` is reserved for
/// ink's independently-chained 3+-condition form, which native's `if`/
/// `else`-only grammar has no way to produce at all — no `else if`). This
/// test is what caught that and pinned the corrected mapping.
#[test]
fn cross_frontend_conditional_shape_matches_ink() {
    let ink_src = "\
== weather ==
{ raining:
    It is raining.
- else:
    It is dry.
}
-> END
";
    let native_src = "\
var raining = true
flow weather() {
  {if raining {
    It is raining.
  } else {
    It is dry.
  }}
}
";
    let ink_knot = lower_ink_knot(ink_src);
    let (native_hir, _m, diags) = lower_native_fixture(native_src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let native_knot = native_hir
        .knots
        .iter()
        .find(|k| k.name.text == "weather")
        .expect("weather knot");

    let ink_cond = ink_knot
        .body
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::Conditional(c) => Some(c),
            _ => None,
        })
        .expect("ink Conditional");
    let native_cond = native_knot
        .body
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::Conditional(c) => Some(c),
            _ => None,
        })
        .expect("native Conditional");

    assert_eq!(ink_cond.kind, brink_ir::CondKind::InitialCondition);
    assert_eq!(native_cond.kind, brink_ir::CondKind::InitialCondition);
    assert_eq!(ink_cond.branches.len(), 2);
    assert_eq!(native_cond.branches.len(), 2);
    assert!(ink_cond.branches[0].condition.is_some());
    assert!(native_cond.branches[0].condition.is_some());
    assert!(ink_cond.branches[1].condition.is_none());
    assert!(native_cond.branches[1].condition.is_none());
}
