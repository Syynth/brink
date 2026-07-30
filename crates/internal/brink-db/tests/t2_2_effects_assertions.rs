//! T2-2 `@[effects(…)]` assertion surface + exceedance error, exercised
//! end-to-end through `ProjectDb`'s salsa query layer (docs/effects-spec.md
//! §10, sitting 2 — 2026-07-14; issue #861 — tracked from #859). Builds on
//! T2-1's `effects(def)` substrate (`t2_1_effect_rows.rs`).
//!
//! Per the sitting-2 ruling, EXCEEDANCE (`E103`) is the *only* diagnostic
//! this surface produces — an inferred row narrower than its declared
//! `@[effects(…)]` bound is silent; there is no drift policy. These tests
//! cover one exceedance fixture per atom class (reads, writes, calls, and
//! the ref-param-indirect write class PR #866 fixed), the `pure` sugar, and
//! assertion-satisfied silence.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{AnalysisOptions, Dialect};
use brink_db::ProjectDb;
use brink_ir::DiagnosticCode;

fn brink_opts() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    }
}

fn analyze(source: &str) -> Vec<brink_ir::Diagnostic> {
    let mut db = ProjectDb::new();
    db.set_analysis_options(brink_opts());
    db.set_file("main.ink", source.to_owned());
    db.analysis().diagnostics.clone()
}

fn codes(diags: &[brink_ir::Diagnostic]) -> Vec<DiagnosticCode> {
    diags.iter().map(|d| d.code).collect()
}

#[test]
fn exceedance_on_an_extra_read() {
    // `@[effects(pure)]` declares the empty row; `spend` actually reads
    // `gold` — exceedance.
    let diags = analyze(
        "VAR gold = 0\n\
         === function spend() ===\n@[effects(pure)]\n~ return gold\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E103], "{diags:?}");
    assert!(diags[0].message.contains("reads gold"), "{diags:?}");
}

#[test]
fn exceedance_on_an_extra_write() {
    // Declares only `reads: gold`; the body also writes it.
    let diags = analyze(
        "VAR gold = 0\n\
         === function spend(cost) ===\n@[effects(reads(gold))]\n~ gold = gold - cost\n~ return gold\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E103], "{diags:?}");
    assert!(diags[0].message.contains("writes gold"), "{diags:?}");
}

#[test]
fn exceedance_on_an_extra_call() {
    // Declares `reads: gold` only; the body also calls the external.
    let diags = analyze(
        "VAR gold = 0\nEXTERNAL play_sfx(x)\n\
         === function spend(cost: int) ===\n@[effects(reads(gold))]\n\
         ~ temp before = gold\n~ play_sfx(cost)\n~ return before\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E103], "{diags:?}");
    assert!(diags[0].message.contains("calls play_sfx"), "{diags:?}");
}

#[test]
fn exceedance_via_ref_param_indirect_write() {
    // The PR #866 bug class: `inc`'s own body assigns a `Param`, never a
    // `Variable` — the write to `val` only becomes visible at `knot`'s own
    // call site, where `val` is passed into `inc`'s `ref x` slot (mirrors
    // `t2_1_effect_rows.rs`'s `effects_query_writes_through_a_ref_param_at_the_call_site`
    // fixture, now checked through the assertion surface). The assertion
    // declares only `reads: val` — the indirect write must still exceed it.
    let diags = analyze(
        "VAR val = 5\n\
         === knot ===\n@[effects(reads(val))]\n~ inc(val)\n{val}\n->->\n\
         === function inc(ref x) ===\n~ x = x + 1\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E103], "{diags:?}");
    assert!(diags[0].message.contains("writes val"), "{diags:?}");
}

#[test]
fn pure_sugar_is_satisfied_by_a_genuinely_pure_body() {
    let diags = analyze("=== function double(x) ===\n@[effects(pure)]\n~ return x * 2\n");
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn assertion_satisfied_is_silent_even_when_strictly_wider_than_inferred() {
    // Over-declaring (an assertion listing more than the body actually
    // touches) is explicitly NOT diagnosed — "no drift policy" (sitting 2).
    let diags = analyze(
        "VAR gold = 0\nVAR hp = 10\n\
         === function spend(cost) ===\n@[effects(reads(gold), writes(gold), writes(hp))]\n\
         ~ gold = gold - cost\n~ return gold\n",
    );
    assert!(
        diags.is_empty(),
        "over-declaring must stay silent: {diags:?}"
    );
}

#[test]
fn unknown_cell_name_in_assertion_is_e102() {
    let diags = analyze(
        "VAR gold = 0\n=== function spend() ===\n@[effects(reads(nonexistent))]\n~ return gold\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E102], "{diags:?}");
}

#[test]
fn unknown_external_name_in_assertion_is_e102() {
    let diags = analyze(
        "VAR gold = 0\n=== function spend() ===\n@[effects(calls(nonexistent))]\n~ return gold\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E102], "{diags:?}");
}

#[test]
fn strict_ink_never_runs_the_exceedance_check() {
    // Default dialect is `StrictInk` — `#@effects` is rejected whole
    // (`E051`) and the exceedance check never runs (no double diagnostic).
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "VAR gold = 0\n=== function spend() ===\n@[effects(pure)]\n~ return gold\n".to_owned(),
    );
    let diags = db.analysis().diagnostics.clone();
    assert_eq!(codes(&diags), vec![DiagnosticCode::E051], "{diags:?}");
}

#[test]
fn unannotated_project_produces_no_effects_diagnostics() {
    let diags = analyze(
        "VAR gold = 0\nEXTERNAL play_sfx(x)\n\
         === function spend(cost) ===\n~ gold = gold - cost\n~ play_sfx(cost)\n~ return gold\n",
    );
    assert!(diags.is_empty(), "{diags:?}");
}

// ── Import-scoped `resolve_cell`/`external_declared` (issue #881, the T2
// follow-up to M-2d/#790) ───────────────────────────────────────────────
//
// Two declared modules each publicly export a VAR named `gold`. Before this
// fix, `resolve_cell` picked an arbitrary (flat, smallest-id) same-named
// candidate regardless of which module the asserting file actually
// imported, while the assertion's own def's *inferred* row (via the real
// import-scoped reference resolver) read whichever module's `gold` the
// import actually bound — a mismatch between the two ids produced a
// spurious `E103` exceedance (or, by luck of id ordering, silently matched)
// independent of which module was really imported.

fn analyze_files(files: &[(&str, &str)]) -> Vec<brink_ir::Diagnostic> {
    let mut db = ProjectDb::new();
    db.set_analysis_options(brink_opts());
    for &(path, source) in files {
        db.set_file(path, source.to_owned());
    }
    db.analysis().diagnostics.clone()
}

const QUEST_A: &str = "#@module(quest_a)\n#@public\nVAR gold = 100\n";
const QUEST_B: &str = "#@module(quest_b)\n#@public\nVAR gold = 200\n";

#[test]
fn effects_row_attributes_the_assertion_to_the_actually_imported_modules_cell() {
    // `main` bare-imports quest_a's `gold` — the assertion's declared
    // `reads: gold` must resolve to quest_a's cell, matching the body's own
    // (import-scoped) read, so the assertion is satisfied.
    let diags = analyze_files(&[
        ("quest_a.ink", QUEST_A),
        ("quest_b.ink", QUEST_B),
        (
            "main.ink",
            "IMPORT { gold } FROM quest_a\n\
             === function spend() ===\n@[effects(reads(gold))]\n~ return gold\n",
        ),
    ]);
    assert!(
        diags
            .iter()
            .all(|d| d.code != DiagnosticCode::E103 && d.code != DiagnosticCode::E102),
        "importing quest_a's `gold` must satisfy `reads: gold` with no spurious \
         exceedance or unknown-name diagnostic: {diags:?}"
    );
}

#[test]
fn effects_row_attributes_the_assertion_to_the_other_importers_cell() {
    // Same fixture, but `main` imports quest_b's `gold` instead — the
    // opposite module wins this time. A flat (import-blind) `resolve_cell`
    // can bind at most one of these two directions correctly by luck of id
    // ordering; both directions must independently be silent.
    let diags = analyze_files(&[
        ("quest_a.ink", QUEST_A),
        ("quest_b.ink", QUEST_B),
        (
            "main.ink",
            "IMPORT { gold } FROM quest_b\n\
             === function spend() ===\n@[effects(reads(gold))]\n~ return gold\n",
        ),
    ]);
    assert!(
        diags
            .iter()
            .all(|d| d.code != DiagnosticCode::E103 && d.code != DiagnosticCode::E102),
        "importing quest_b's `gold` must satisfy `reads: gold` with no spurious \
         exceedance or unknown-name diagnostic: {diags:?}"
    );
}

#[test]
fn unimported_cross_module_reference_attributes_consistently_with_resolution() {
    // `main` imports neither module's `gold` — the bare body reference and
    // the assertion's clause both fall back to the exact same (import-blind)
    // flat first-winner `lookup_by_name` already uses elsewhere (M-2d), so
    // they can never disagree on *which* module's cell is meant, even
    // though the reference itself is separately flagged `E025`
    // (import-required) by the cross-module module gate. The point under
    // test is that this un-imported case produces no exceedance —
    // `resolve_cell` and the body's own resolution are the same function
    // call, so they cannot diverge.
    let diags = analyze_files(&[
        ("quest_a.ink", QUEST_A),
        ("quest_b.ink", QUEST_B),
        (
            "main.ink",
            "=== function spend() ===\n@[effects(reads(gold))]\n~ return gold\n",
        ),
    ]);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E025),
        "the un-imported bare cross-module reference must still be gated: {diags:?}"
    );
    assert!(
        diags
            .iter()
            .all(|d| d.code != DiagnosticCode::E103 && d.code != DiagnosticCode::E102),
        "the assertion and the body reference must resolve to the same cell \
         (both via the same import-blind fallback), so no exceedance or \
         unknown-name diagnostic can fire: {diags:?}"
    );
}

// ── NS-A2 (issue #1108): the `silent`/`total` assertion args ────────────

#[test]
fn silent_exceedance_on_a_content_line_is_e108() {
    let diags = analyze("-> talker\n\n=== talker ===\n@[effects(silent)]\nHello there.\n-> END\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E108], "{diags:?}");
}

#[test]
fn silent_exceedance_through_a_transitive_callee_is_e108() {
    // The #1087 motivating shape: the asserted def never contains a content
    // line itself — it calls a function that narrates.
    let diags = analyze(
        "=== function outer() ===\n@[effects(silent)]\n~ return speak()\n\n\
         === function speak() ===\nDialogue!\n~ return 1\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E108], "{diags:?}");
}

#[test]
fn tag_only_line_does_not_exceed_silent() {
    // The 2026-07-18 ruling: tags are the metadata channel, not narration —
    // a flow that only annotates isn't speaking, so `silent` holds.
    let diags = analyze("-> marker\n\n=== marker ===\n@[effects(silent)]\n# checkpoint\n-> END\n");
    assert_eq!(codes(&diags), Vec::<DiagnosticCode>::new(), "{diags:?}");
}

#[test]
fn total_exceedance_on_an_indexing_construct_is_e109() {
    let diags = analyze(
        "=== function pick_first(a: Array<int>): int ===\n@[effects(total)]\n~ return a[0]\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E109], "{diags:?}");
}

#[test]
fn total_exceedance_on_division_is_e109() {
    let diags =
        analyze("=== function ratio(a: int, b: int): int ===\n@[effects(total)]\n~ return a / b\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E109], "{diags:?}");
}

#[test]
fn total_exceedance_on_a_faulting_stdlib_verb_is_e109() {
    // `min` carries `NotOrderable`/`StdlibWrongType` fault paths — §4b's
    // "orderings carry faults unconditionally" (mode-independent rows).
    let diags = analyze(
        "=== function lowest(a: Array<int>) ===\n@[effects(total)]\n~ return min(a) or 0\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E109], "{diags:?}");
}

#[test]
fn satisfied_silent_and_total_are_silent() {
    let diags = analyze(
        "=== function add(a: int, b: int): int ===\n@[effects(pure, silent, total)]\n~ return a + b\n",
    );
    assert_eq!(codes(&diags), Vec::<DiagnosticCode>::new(), "{diags:?}");
}

#[test]
fn emitting_def_without_silent_assertion_is_legal() {
    // Exceedance-only posture: the dimensions are inferred metadata; only
    // an explicit assertion can be exceeded.
    let diags = analyze("-> talker\n\n=== talker ===\n@[effects(total)]\nHello.\n-> END\n");
    assert_eq!(codes(&diags), Vec::<DiagnosticCode>::new(), "{diags:?}");
}

#[test]
fn opaque_row_exceeds_both_silent_and_total() {
    // A call through a function value is unbounded on every dimension.
    // `cb` is annotated `fn(): int` — the value's *type* is known under the
    // Brink strict default, but a call through a function value stays
    // unbounded on the effects dimensions regardless (the row depends on
    // which function flows in), which is exactly the subject here.
    let diags = analyze(
        "=== function apply(cb: fn(): int) ===\n@[effects(silent, total)]\n~ return cb()\n",
    );
    assert_eq!(
        codes(&diags),
        vec![DiagnosticCode::E108, DiagnosticCode::E109],
        "{diags:?}"
    );
}

// ── Fork A (docs/decision-log.md 2026-07-28, issue #1726): the opaque
// floor collapses to a real row when every reaching fn value was created
// in-project ─────────────────────────────────────────────────────────────
//
// These are the end-to-end proof that the analyzer-level narrowing reaches a
// *user* through the real salsa pipeline: the row is what `E103` judges an
// `@[effects(…)]` bound against, so a collapsed row is directly visible as a
// diagnostic that no longer fires.

#[test]
fn two_known_fn_origins_collapse_to_the_joined_row_instead_of_the_opaque_floor() {
    // `f` is written twice, each write a known `#fn` creation site. Before
    // Fork A the write-once rule refused to narrow a twice-written local, so
    // the row was the pessimal floor and *every* bound was exceeded. Now the
    // row is the join over both targets — `bar`'s (total) and `baz`'s
    // (extra) — which this bound declares exactly, so the check is silent.
    //
    // The two origins touch *different* globals on purpose: a shared global
    // would still pass if only one edge were followed, proving nothing about
    // the join.
    let diags = analyze(
        "VAR total = 0\nVAR extra = 0\n\
         === function bar(): int ===\n~ total = total + 1\n~ return total\n\
         === function baz(): int ===\n~ extra = extra + 100\n~ return extra\n\
         === function user(cond: int): int ===\n\
         @[effects(reads(total), reads(extra), writes(total), writes(extra))]\n\
         ~ temp f = #fn(bar)\n{cond:\n  ~ f = #fn(baz)\n}\n~ return f()\n",
    );
    assert_eq!(codes(&diags), Vec::<DiagnosticCode>::new(), "{diags:?}");
}

#[test]
fn the_joined_row_still_reports_a_bound_that_names_only_one_origin() {
    // The collapse is not a free pass: narrowing to the join means the bound
    // must cover *both* targets. Declaring only `bar`'s cell leaves `baz`'s
    // write uncovered — still an exceedance, and the message names the atom
    // the join contributed, not a generic "calls through a function value".
    let diags = analyze(
        "VAR total = 0\nVAR extra = 0\n\
         === function bar(): int ===\n~ total = total + 1\n~ return total\n\
         === function baz(): int ===\n~ extra = extra + 100\n~ return extra\n\
         === function user(cond: int): int ===\n\
         @[effects(reads(total), writes(total))]\n\
         ~ temp f = #fn(bar)\n{cond:\n  ~ f = #fn(baz)\n}\n~ return f()\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E103], "{diags:?}");
    assert!(diags[0].message.contains("extra"), "{diags:?}");
}

#[test]
fn one_untraced_write_keeps_the_opaque_floor_end_to_end() {
    // The guard Fork A keeps. `f`'s second write comes from a parameter, so
    // the reaching value could have been created anywhere — including a host
    // callback (docs/effects-spec.md §6.2). The row stays pessimal, and the
    // `silent`/`total` dimensions it is unbounded on still report, exactly
    // as `opaque_row_exceeds_both_silent_and_total` pins for the plain case.
    let diags = analyze(
        "VAR total = 0\n\
         === function bar(): int ===\n~ total = total + 1\n~ return total\n\
         === function user(cond: int, cb: fn(): int): int ===\n\
         @[effects(silent, total)]\n\
         ~ temp f = #fn(bar)\n{cond:\n  ~ f = cb\n}\n~ return f()\n",
    );
    assert_eq!(
        codes(&diags),
        vec![DiagnosticCode::E108, DiagnosticCode::E109],
        "{diags:?}"
    );
}

#[test]
fn deprecated_hash_spelling_reaches_per_file_diagnostics_as_e110_warning() {
    // E110 is a *lowering* diagnostic (the directive recognizer), so it
    // surfaces on the per-file layer, not the cross-file analysis layer.
    let mut db = ProjectDb::new();
    db.set_analysis_options(brink_opts());
    let id = db.set_file(
        "main.ink",
        "=== function add(a: int, b: int): int ===\n#@effects(pure)\n~ return a + b\n".to_owned(),
    );
    let diags = db.diagnostics(id).expect("file known").to_vec();
    assert_eq!(codes(&diags), vec![DiagnosticCode::E110], "{diags:?}");
    assert_eq!(
        diags[0].code.severity(),
        brink_ir::Severity::Warning,
        "the alias is a warning, not an error"
    );
}

// ── NS-A6 (issue #1112, docs/stdlib-spec.md §7): the RNG cell in the
// assertion surface — draws are ordinary writes ──────────────────────────

#[test]
fn pure_assertion_exceeded_by_a_draw() {
    // `@[effects(pure)]` asserts rng-freedom (the ruled free consequence):
    // a draw-bearing body writes the RNG cell, exceeding the empty bound.
    let diags = analyze("=== function coin() ===\n@[effects(pure)]\n~ return chance(0.5)\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E103], "{diags:?}");
    assert!(
        diags[0].message.contains("rng"),
        "the exceedance names the rng cell: {diags:?}"
    );
}

#[test]
fn writes_rng_clause_covers_a_draw_bearing_def() {
    // The compiler-owned cell is nameable in a `writes` clause as `rng`,
    // so a draw-bearing def can carry a covering bound.
    let diags = analyze("=== function coin() ===\n@[effects(writes(rng))]\n~ return chance(0.5)\n");
    assert_eq!(codes(&diags), Vec::<DiagnosticCode>::new(), "{diags:?}");
}

#[test]
fn pure_assertion_exceeded_by_the_frozen_ink_spelling_too() {
    // One cell, two surfaces: ink's RANDOM writes the same cell and
    // exceeds `pure` identically. The `: int` return annotation is needed
    // because `infer_intrinsic` has no typing arm for the frozen ink
    // spellings (they fall through to `Unknown`) — a pre-existing strict-
    // inference gap, visible under the Brink dialect's strict default.
    // (The fixture was named `roll` until NS-A7 claimed that name for the
    // `Weighted` draw verb — the E035 shadow warning this fixture would
    // now also carry is the shadowing machinery working as designed, but
    // it is not what this test is about, so the def is `d6` now.)
    let diags = analyze("=== function d6(): int ===\n@[effects(pure)]\n~ return RANDOM(1, 6)\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E103], "{diags:?}");
    assert!(diags[0].message.contains("rng"), "{diags:?}");
}

#[test]
fn user_var_named_rng_shadows_the_cell_name_in_clauses() {
    // A user VAR `rng` wins the clause-name lookup (stdlib shadowing rule);
    // the def writes only that VAR, and the bound covers it — silence. The
    // draw-free body never touches the compiler cell, so no exceedance.
    let diags = analyze(
        "VAR rng = 0\n\
         === function bump() ===\n@[effects(reads(rng), writes(rng))]\n~ rng = rng + 1\n~ return rng\n",
    );
    assert_eq!(codes(&diags), Vec::<DiagnosticCode>::new(), "{diags:?}");
}

// ── The native `.brink` surface (issue #1563) ────────────────────────────
//
// A per-declaration `@[effects(…)]` above a native `fn`/`flow` used to be a
// hard `E129` compile failure in `hir::lower_native`, so this whole surface
// was unreachable from a `.brink` file. Now that the annotation channel
// lowers, the *same* frontend-agnostic exceedance check that judges ink
// assertions judges native ones — these tests are the end-to-end proof that
// the channel reaches a user through the real salsa pipeline, not just the
// HIR unit tests in `brink-ir`.

/// `.brink` files route through `lower_native`; the fixtures below need
/// exactly the same `Dialect::Brink` posture `analyze` already sets (which
/// also resolves `types` to `Strict`, native's requirement — `E137`).
fn analyze_native(source: &str) -> Vec<brink_ir::Diagnostic> {
    let mut db = ProjectDb::new();
    db.set_analysis_options(brink_opts());
    db.set_file("main.brink", source.to_owned());
    db.analysis().diagnostics.clone()
}

#[test]
fn native_pure_assertion_is_exceeded_by_a_global_read() {
    let diags =
        analyze_native("var gold = 0\n\n@[effects(pure)]\nfn spend() {\n  return gold;\n}\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E103], "{diags:?}");
    assert!(diags[0].message.contains("reads gold"), "{diags:?}");
}

#[test]
fn native_assertion_that_covers_the_body_is_silent() {
    let diags = analyze_native(
        "var gold = 0\n\n@[effects(reads(gold))]\nfn spend() {\n  return gold;\n}\n",
    );
    assert_eq!(codes(&diags), Vec::<DiagnosticCode>::new(), "{diags:?}");
}

#[test]
fn native_writes_clause_exceedance_names_the_written_cell() {
    let diags = analyze_native(
        "var gold = 0\n\n@[effects(reads(gold))]\nfn spend(cost) {\n  gold = gold - cost;\n  return gold;\n}\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E103], "{diags:?}");
    assert!(diags[0].message.contains("writes gold"), "{diags:?}");
}

#[test]
fn native_unknown_cell_name_in_an_assertion_is_e102() {
    let diags = analyze_native(
        "var gold = 0\n\n@[effects(reads(nonexistent))]\nfn spend() {\n  return gold;\n}\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E102], "{diags:?}");
}

#[test]
fn native_stitch_silent_assertion_is_exceeded_by_an_emitting_nested_flow() {
    // The `Stitch` half of the channel (`container::lower_stitch` calling
    // `annotation::effects_assertion`) needs its own end-to-end proof that
    // the analyzer resolves it. Every other native fixture in this file
    // attaches `@[effects(…)]` to a top-level `fn`/`flow`, which is
    // satisfied identically whether or not
    // `effects_assertions::find_def_id(..., SymbolKind::Stitch, ...)` ever
    // finds the nested def — the golden corpus case has the same gap (its
    // stitch assertion is satisfied, not exceeded). A nested `flow` whose
    // body emits content is not `silent`, so this can only go green if the
    // stitch path actually reaches the exceedance checker.
    let diags = analyze_native(
        "flow main() {\n  @[effects(silent)]\n  flow tally() {\n    Gold falls.\n  }\n  -> tally\n}\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E108], "{diags:?}");
}

#[test]
fn native_silent_assertion_is_exceeded_by_a_flow_that_emits() {
    // The NS-A2 output dimension, on the native surface: a `flow` whose
    // body writes a line is not `silent`.
    let diags = analyze_native("@[effects(silent)]\nflow garden() {\n  Petals fall.\n}\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E108], "{diags:?}");
}
