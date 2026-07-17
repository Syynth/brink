//! T2-4 tier1-brink effects corpus wing (docs/effects-spec.md §10, issue
//! #863 — the T2 compiler-half tail, tracked from #859). Exercises the whole
//! effects author surface *end-to-end* — inference, the `#@effects` assertion
//! contract, and the exceedance error (`E103`) — through the real compiler +
//! `ProjectDb` salsa layer + runtime, the tier1-brink way (compile a brink
//! program and run it to completion).
//!
//! This complements, rather than duplicates, the lower layers:
//!
//! - `brink-analyzer::infer::effects` unit tests pin the lattice/fixpoint;
//! - `brink-db/tests/t2_1_effect_rows.rs` pins per-def `effects(def)` rows;
//! - `brink-db/tests/t2_2_effects_assertions.rs` pins the `E103` exceedance
//!   diagnostic at the db-query layer;
//! - `t2_ground_truth_effects.rs` (feature-gated) pins no-under-report
//!   against the instrumented VM.
//!
//! What is proven *only here* is the top-to-bottom author story: a brink
//! program carrying a *satisfied* `#@effects(…)` bound compiles clean AND
//! runs to completion with the exact output an unannotated equivalent would
//! produce (the directive is runtime-inert — advisory metadata, spec §10),
//! while an *exceeding* bound is a real compile error through
//! `brink_compiler::compile_with_options`, not merely a db-query diagnostic.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_compiler::{AnalysisOptions, Dialect};
use brink_db::ProjectDb;
use brink_format::DefinitionId;
use brink_ir::SymbolKind;
use brink_runtime::{DotNetRng, Line, Story};

fn brink_opts() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    }
}

/// Build a single-file `ProjectDb` from `source` under the brink dialect —
/// enough to query the advisory `effects(def)` rows (no entry point needed:
/// `effects` is a per-def query, not a whole-project compile).
fn build_db(source: &str) -> ProjectDb {
    let mut db = ProjectDb::new();
    db.set_analysis_options(brink_opts());
    db.set_file("main.ink", source.to_owned());
    db
}

/// The `DefinitionId` of the knot/stitch named `name` in `db`.
fn callable_id(db: &ProjectDb, name: &str) -> DefinitionId {
    let index = db.symbol_index();
    index
        .by_name
        .get(name)
        .and_then(|ids| {
            ids.iter().copied().find(|id| {
                index
                    .symbols
                    .get(id)
                    .is_some_and(|s| matches!(s.kind, SymbolKind::Knot | SymbolKind::Stitch))
            })
        })
        .unwrap_or_else(|| panic!("no knot/stitch named `{name}`"))
}

/// The set of cell *names* an effect row's `reads`/`writes` component holds
/// (resolved through the symbol index), for readable assertions.
fn cell_names(db: &ProjectDb, ids: &std::collections::BTreeSet<DefinitionId>) -> Vec<String> {
    let index = db.symbol_index();
    let mut names: Vec<String> = ids
        .iter()
        .map(|id| {
            index
                .symbols
                .get(id)
                .map_or_else(|| format!("{id:?}"), |s| s.name.clone())
        })
        .collect();
    names.sort();
    names
}

/// Maximum `continue_single` calls per case before aborting. Guards against
/// an infinite-output program (e.g. a self-looping knot) spinning this test
/// forever — each call is itself bounded by `Story::STEP_LIMIT`, but that
/// only caps a single call, not the outer drive loop across calls. Matches
/// `explorer.rs`'s `STEP_LIMIT` convention.
const STEP_LIMIT: usize = 10_000;

/// Compile `source` (brink dialect) and run it to completion, returning the
/// concatenated output. Panics on any compile/runtime error, a choice —
/// every case here is a choice-free straight-line program — or exceeding
/// `STEP_LIMIT` lines.
fn run_brink(source: &str) -> String {
    let output =
        brink_compiler::compile_with_options("main.ink", |_| Ok(source.to_owned()), brink_opts())
            .unwrap_or_else(|e| panic!("compile: {e:?}\n--- source ---\n{source}"));
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    let mut out = String::new();
    let mut step_count = 0;
    loop {
        step_count += 1;
        assert!(
            step_count <= STEP_LIMIT,
            "case exceeded {STEP_LIMIT} lines without completing \
             (must be straight-line and terminating):\n{source}"
        );
        match story.continue_single().expect("runtime error") {
            Line::Text { text, .. } => out.push_str(&text),
            Line::Done { text, .. } | Line::End { text, .. } => {
                out.push_str(&text);
                break;
            }
            Line::Choices { .. } => panic!("unexpected choices:\n{source}"),
        }
    }
    out
}

/// Compile `source` and return the error diagnostic codes (empty if it
/// compiled clean).
fn error_codes(source: &str) -> Vec<brink_compiler::DiagnosticCode> {
    match brink_compiler::compile_with_options("main.ink", |_| Ok(source.to_owned()), brink_opts())
    {
        Ok(_) => Vec::new(),
        Err(brink_compiler::CompileError::Diagnostics(diags)) => {
            diags.iter().map(|d| d.code).collect()
        }
        Err(other) => panic!("unexpected compile error shape: {other:?}"),
    }
}

// ── Inference (spec §2/§4) ──────────────────────────────────────────────

#[test]
fn effect_row_is_inferred_end_to_end() {
    // A knot that reads + writes a global and calls an external — its row must
    // list exactly those atoms.
    let db = build_db(
        "VAR gold = 10\nEXTERNAL play_sfx(x)\n\
         === function spend(cost) ===\n\
         ~ gold = gold - cost\n~ play_sfx(cost)\n~ return gold\n",
    );
    let row = db
        .effects(callable_id(&db, "spend"))
        .expect("spend has a row");
    assert!(!row.opaque, "concrete body, not opaque: {row:?}");
    assert_eq!(cell_names(&db, &row.reads), vec!["gold"]);
    assert_eq!(cell_names(&db, &row.writes), vec!["gold"]);
    assert_eq!(
        row.calls.iter().cloned().collect::<Vec<_>>(),
        vec!["play_sfx".to_string()]
    );
}

#[test]
fn transitive_call_effects_flow_through_inference() {
    // `outer` is pure on its own body but calls `inner`, which writes `gold`
    // and calls the external — the per-SCC fixpoint (spec §4) must pull both
    // atoms up into `outer`'s row.
    let db = build_db(
        "VAR gold = 0\nEXTERNAL play_sfx(x)\n\
         === function outer() ===\n~ return inner()\n\
         === function inner() ===\n~ gold = gold + 1\n~ play_sfx(1)\n~ return gold\n",
    );
    let row = db
        .effects(callable_id(&db, "outer"))
        .expect("outer has a row");
    assert!(!row.opaque, "{row:?}");
    assert_eq!(cell_names(&db, &row.writes), vec!["gold"]);
    assert_eq!(
        row.calls.iter().cloned().collect::<Vec<_>>(),
        vec!["play_sfx".to_string()]
    );
}

#[test]
fn call_through_a_function_value_is_opaque() {
    // Dispatch through a `#fn` value escapes the static call graph — the row
    // degrades to the pessimal `opaque` top element (spec §3/§4), the sound
    // floor.
    let db = build_db(
        "VAR total = 0\n\
         === function apply(cb) ===\n~ return cb()\n\
         === function bump() ===\n~ total = total + 1\n~ return total\n\
         === main ===\n~ temp f = #fn(bump)\n~ temp x = apply(f)\n{total}\n-> END\n",
    );
    let row = db
        .effects(callable_id(&db, "apply"))
        .expect("apply has a row");
    assert!(
        row.opaque,
        "call through a fn value must be opaque: {row:?}"
    );
}

// ── The `#@effects` contract (spec §10) ─────────────────────────────────

#[test]
fn satisfied_effects_assertion_compiles_and_runs_inert() {
    // A correct upper bound: `go` reads + writes `gold`. The assertion must
    // compile clean, and the program must run to the exact output an
    // unannotated equivalent produces — the directive is runtime-inert
    // advisory metadata. (No external in the *run* path: an unbound external
    // faults at runtime independently of effects; the `calls` clause is
    // covered by the compile-only exceedance cases below.)
    let annotated = "VAR gold = 10\n-> go\n\
         === go ===\n#@effects(reads: gold, writes: gold)\n\
         ~ gold = gold - 3\nGold is {gold}.\n-> END\n";
    let plain = "VAR gold = 10\n-> go\n\
         === go ===\n\
         ~ gold = gold - 3\nGold is {gold}.\n-> END\n";
    assert!(
        error_codes(annotated).is_empty(),
        "a satisfied #@effects bound must compile clean: {:?}",
        error_codes(annotated)
    );
    assert_eq!(
        run_brink(annotated),
        run_brink(plain),
        "the #@effects directive must not change runtime behavior"
    );
    assert_eq!(run_brink(annotated).trim(), "Gold is 7.");
}

#[test]
fn pure_sugar_satisfied_by_a_pure_knot_runs() {
    let source = "-> go\n=== go ===\n#@effects(pure)\nHello.\n-> END\n";
    assert!(error_codes(source).is_empty(), "{:?}", error_codes(source));
    assert_eq!(run_brink(source).trim(), "Hello.");
}

#[test]
fn over_declaring_a_wider_bound_never_warns() {
    // The bound is strictly wider than what's inferred (declares a write +
    // call the body never performs). Per the sitting-2 ruling there is no
    // drift policy — over-declaring is silent, no `E103`, no warning.
    let source = "VAR gold = 0\nVAR silver = 0\nEXTERNAL play_sfx(x)\n-> go\n\
         === go ===\n#@effects(reads: gold, writes: silver, calls: play_sfx)\n\
         Value {gold}.\n-> END\n";
    assert!(
        error_codes(source).is_empty(),
        "over-declaring must be silent (no drift policy): {:?}",
        error_codes(source)
    );
    assert_eq!(run_brink(source).trim(), "Value 0.");
}

// ── Exceedance (E103) end-to-end (spec §10) ─────────────────────────────

#[test]
fn exceedance_is_a_real_compile_error_end_to_end() {
    // `#@effects(pure)` on a knot that writes `gold` — exceedance through the
    // full compiler entry point, not just the db-query diagnostic layer.
    let source = "VAR gold = 0\n-> go\n\
         === go ===\n#@effects(pure)\n~ gold = gold + 1\nGold {gold}.\n-> END\n";
    assert!(
        error_codes(source).contains(&brink_compiler::DiagnosticCode::E103),
        "expected E103 exceedance, got {:?}",
        error_codes(source)
    );
}

#[test]
fn exceedance_on_an_undeclared_call_is_e103() {
    let source = "VAR gold = 0\nEXTERNAL play_sfx(x)\n-> go\n\
         === go ===\n#@effects(reads: gold)\n~ play_sfx(1)\nHi {gold}.\n-> END\n";
    assert!(
        error_codes(source).contains(&brink_compiler::DiagnosticCode::E103),
        "expected E103, got {:?}",
        error_codes(source)
    );
}

#[test]
fn a_bound_cannot_cover_an_opaque_row_e103() {
    // A knot whose row is opaque (dispatch through a fn value) can never be
    // bounded by a concrete `#@effects` assertion — the exceedance message's
    // opaque branch (spec §3).
    let source = "VAR total = 0\n\
         === function bump() ===\n~ total = total + 1\n~ return total\n\
         === go ===\n#@effects(reads: total)\n\
         ~ temp f = #fn(bump)\n~ temp x = f()\n{total}\n-> END\n";
    assert!(
        error_codes(source).contains(&brink_compiler::DiagnosticCode::E103),
        "expected E103 for an unbounded (opaque) row, got {:?}",
        error_codes(source)
    );
}

// ── `run_brink` outer-loop step cap ──────────────────────────────────────

#[test]
#[should_panic(expected = "exceeded")]
fn run_brink_bounds_the_outer_drive_loop_on_infinite_output() {
    // A knot that unconditionally diverts to itself emits one `Line::Text`
    // per `continue_single` call forever — each call is under
    // `Story::STEP_LIMIT`, so nothing faults, but the outer loop across
    // calls must still be capped or this spins forever (see `STEP_LIMIT`
    // above, matching `explorer.rs`'s convention).
    let source = "-> loop\n=== loop ===\nx\n-> loop\n";
    run_brink(source);
}
