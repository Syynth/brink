//! T2 ground-truth effect-completeness harness (issue #870,
//! `docs/effects-spec.md` §2/§3/§4). The effects analogue of the oracle:
//! runs each program through the *real*, instrumented VM
//! (`brink-runtime`'s bench/test-only `effect-trace` feature — see
//! `brink_runtime::effect_trace`'s module docs for the exact attribution
//! model) and asserts the statically-inferred `effects(def)` row
//! (`brink_db::ProjectDb::effects`) covers every atom actually observed for
//! every def the run executed.
//!
//! This closes the gap issue #870 was filed over: the pre-existing
//! `conservative_total_*` property tests in `brink-analyzer::infer::
//! effects` only check *inter-row structural consistency* (a caller's row
//! ⊇ its callees' rows) — the #866 ref-param-write bug passed that check
//! while both the caller's and the callee's row silently under-reported the
//! *same* real write. A structurally-consistent set of wrong rows satisfies
//! ⊇-consistency; only running the bytecode and comparing against what it
//! actually touched can catch that. This test is that independent oracle.
//!
//! Feature-gated exactly like `bench-counters` (issue #821): `cargo test
//! --workspace` (no extra flags) never builds this file at all —
//! `required-features = ["effect-trace"]` in `Cargo.toml` keeps it out of
//! the default test set entirely, so the ground-truth harness costs nothing
//! until a CI job explicitly opts in with `--features effect-trace`.

#![cfg(feature = "effect-trace")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use brink_compiler::{AnalysisOptions, Dialect};
use brink_db::ProjectDb;
use brink_driver::Driver;
use brink_format::DefinitionId;
use brink_runtime::effect_trace::{self, ObservedRow};
use brink_test_harness::explorer::{ExploreConfig, explore};

/// `brink_runtime::effect_trace`'s `OBSERVED` map is a single process-wide
/// static (by design — see its module docs), and `cargo test` runs every
/// `#[test]` in this file on its own thread by default. Without this lock,
/// two tests' `observe()` calls interleave on the *same* recorder: test A's
/// `reset()` can land between test B's `reset()` and `snapshot()` and wipe
/// atoms B is mid-way through recording, or B's VM run can deposit atoms
/// that A's snapshot then picks up as its own — a false "under-report"
/// attributed to the wrong def. Reproduced locally: `ref_param_write_
/// ground_truth_matches_the_866_regression_shape` and a sibling test both
/// failed under ordinary parallel `cargo test`, passed every time run
/// `--test-threads=1` or in isolation. Serializing the whole reset→explore
/// →snapshot cycle here (not just individual recorder calls, which
/// `effect_trace`'s own `Mutex` already does) is what actually fixes it —
/// the race is between *cycles*, not between individual map mutations.
static OBSERVE_LOCK: Mutex<()> = Mutex::new(());

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
        .join("tier1-brink")
}

/// Discover+analyze `entry` into a `ProjectDb` under `options` — the same
/// `Driver::discover` + `set_entry` path `brink-compiler`'s own driver uses
/// internally (`brink-compiler/src/driver.rs::prepare_driver`), just
/// keeping the `ProjectDb` around afterward instead of only pulling
/// `StoryData` out of it. Using *one* db for both the static `effects()`
/// query and the compiled `StoryData` the VM runs is what guarantees the
/// `DefinitionId`s on both sides of the ground-truth comparison agree —
/// building two separate dbs even from identical source has no such
/// guarantee.
fn build_db<F>(entry: &str, read_file: F, options: AnalysisOptions) -> ProjectDb
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    let mut driver = Driver::new();
    driver.set_analysis_options(options);
    driver
        .discover(entry, read_file)
        .unwrap_or_else(|e| panic!("discover {entry}: {e}"));
    driver
        .db_mut()
        .set_entry(entry)
        .unwrap_or_else(|| panic!("entry not found after discovery: {entry}"));
    driver.into_db()
}

/// Compile `db`'s project to `StoryData`, link, reset the ground-truth
/// recorder, explore every reachable branch (bounded — house rule: no
/// unbounded accumulation), and snapshot what got recorded. Panics on a
/// compile/link error — every case this harness runs is expected to
/// succeed cleanly (a corpus/fixture that's *supposed* to fail compilation
/// has nothing to ground-truth).
fn observe(label: &str, db: &ProjectDb) -> BTreeMap<DefinitionId, ObservedRow> {
    let product = db
        .story_data()
        .unwrap_or_else(|| panic!("{label}: story_data() — no entry set"));
    let story_data = product
        .story
        .clone()
        .unwrap_or_else(|| panic!("{label}: compile errors: {:?}", product.errors));
    let (program, line_tables) =
        brink_runtime::link(&story_data).unwrap_or_else(|e| panic!("{label}: link: {e}"));

    // Hold the lock across the whole reset→explore→snapshot cycle — see
    // `OBSERVE_LOCK`'s docs for why a per-call recorder lock isn't enough.
    let _guard = OBSERVE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    effect_trace::reset();
    let _episodes = explore(
        Arc::new(program),
        line_tables,
        &ExploreConfig {
            max_depth: 40,
            max_episodes: 2_000,
        },
    );
    effect_trace::snapshot()
}

/// The core assertion (issue #870's whole point): every def the run
/// actually executed must have every observed atom covered by its
/// statically-inferred row. Fails with the def's name, the atom kind, and
/// the missing cell/external — actionable, per the issue's own ask.
fn assert_ground_truth(
    label: &str,
    db: &ProjectDb,
    observed: &BTreeMap<DefinitionId, ObservedRow>,
) {
    let index = db.symbol_index();
    let resolve_name = |id: &DefinitionId| {
        index
            .symbols
            .get(id)
            .map_or_else(|| format!("{id:?}"), |s| s.name.clone())
    };

    for (def, row) in observed {
        let Some(static_row) = db.effects(*def) else {
            // Not a knot/stitch/function (e.g. root-level loose content, or
            // a label with no def-level row of its own) — `effects()`
            // deliberately has nothing to say about it (contract:
            // `ProjectDb::effects`'s docs, "None for a def with no
            // inferable body"), so there is nothing to ground-truth here.
            continue;
        };
        if static_row.opaque {
            continue; // Opaque covers everything by construction (spec §3).
        }
        let def_name = resolve_name(def);

        for cell in &row.reads {
            assert!(
                static_row.reads.contains(cell),
                "{label}: def `{def_name}` observed a READ of `{}` the \
                 static effects() row never lists — under-report (issue #870)",
                resolve_name(cell),
            );
        }
        for cell in &row.writes {
            assert!(
                static_row.writes.contains(cell),
                "{label}: def `{def_name}` observed a WRITE of `{}` the \
                 static effects() row never lists — under-report (issue #870)",
                resolve_name(cell),
            );
        }
        for kind in &row.calls {
            assert!(
                static_row.calls.contains(kind),
                "{label}: def `{def_name}` observed a CALL to external \
                 `{kind}` the static effects() row never lists — \
                 under-report (issue #870)",
            );
        }
        // ── NS-A2 (issue #1108): the output/fault dimensions. The observed
        // side may under-approximate (capture-channel output is skipped),
        // never over-approximate — so observed ⊆ declared is exactly the
        // sound direction to assert.
        if row.emits {
            assert!(
                static_row.emits,
                "{label}: def `{def_name}` observed visible OUTPUT the \
                 static effects() row's `emits` dimension never admits — \
                 under-report (issue #1108)",
            );
        }
        if row.tags {
            assert!(
                static_row.tags,
                "{label}: def `{def_name}` observed a TAG the static \
                 effects() row's `tags` dimension never admits — \
                 under-report (issue #1108)",
            );
        }
        if row.faults {
            assert!(
                static_row.faults,
                "{label}: def `{def_name}` observed a tracked FAULT the \
                 static effects() row's `faults` dimension never admits — \
                 under-report (issue #1108)",
            );
        }
    }
}

/// Ground-truth one `tests/tier1-brink/<name>/story.ink` case.
///
/// Formerly skipped `knapsack-01`/`longest-common-subsequence`/
/// `memoized-fibonacci`'s `writes` assertion via a `KNOWN_MUTATOR_WRITE_GAP_
/// CASES` list — the gap this harness *discovered* while being built (issue
/// #870's own job): `insert(cell, key, val)`/`push`/`remove` (the T1b-3
/// stdlib mutators, `docs/t1b-surface-spec.md` §5) write their first
/// (lvalue) argument back through the *same* codegen path a normal
/// assignment uses (`brink-ir`'s `writeback_lvalue_container_chain`), but
/// `brink-analyzer::infer::body`'s effect harvester had no `record_write`
/// case for a mutator *call* at all. Fixed by issue #880 (`infer_intrinsic`'s
/// `push`/`insert`/`remove` arms now call `record_write` against the lvalue
/// argument, the same call-site pattern `record_ref_param_writes` uses for a
/// `ref` parameter) — the skip list and its canary
/// (`known_mutator_write_gap_cases_still_actually_have_the_gap`) are removed
/// per the unskip protocol; the three cases now run the full, unfiltered
/// ground-truth check like every other corpus case.
fn check_corpus_case(dir: &Path) {
    let ink_path = dir.join("story.ink");
    let entry = ink_path.to_string_lossy().into_owned();
    // NS-A9 (strict brink default): this harness ground-truths *effect rows*
    // — a regime-independent subject — over fixtures and inline sources
    // written in the gradual idiom (unannotated params, `VAR x = 0`
    // placeholders). Explicit gradual, same as `algorithms_ground_truth`'s
    // helper contract.
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(brink_compiler::TypePolicy::Gradual),
        ..AnalysisOptions::default()
    };
    let db = build_db(&entry, |p| std::fs::read_to_string(p), options);
    let observed = observe(&entry, &db);
    assert_ground_truth(&entry, &db, &observed);
}

/// Ground-truth one in-memory single-file `source` under the brink
/// dialect, labeled `label` for failure messages.
fn check_source(label: &str, source: &str) {
    // NS-A9 (strict brink default): this harness ground-truths *effect rows*
    // — a regime-independent subject — over fixtures and inline sources
    // written in the gradual idiom (unannotated params, `VAR x = 0`
    // placeholders). Explicit gradual, same as `algorithms_ground_truth`'s
    // helper contract.
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(brink_compiler::TypePolicy::Gradual),
        ..AnalysisOptions::default()
    };
    let db = build_db("main.ink", |_| Ok(source.to_owned()), options);
    let observed = observe(label, &db);
    assert_ground_truth(label, &db, &observed);
}

/// Every `tests/tier1-brink/<name>/story.ink` case (flat, non-nested
/// entries only — `algorithms/` is a nested sub-corpus with its own
/// `story.ink`-bearing leaf directories, walked the same way
/// `tier1_brink.rs`/`algorithms_corpus.rs` do it independently; both are
/// covered here via a directory walk rather than a hand-maintained name
/// list, since this harness only needs "run it and compare", not a
/// per-case `#[test]` fn).
#[test]
fn tier1_brink_corpus_never_under_reports_effects() {
    // Descent via the shared `brink_source_tree::Walk` (issue #1433) rather
    // than a hand-written `read_dir` recursion, so this enumerator can't be
    // the next one to forget the ignored-directory prune.
    fn collect_story_ink_dirs(dir: &Path, out: &mut Vec<PathBuf>) {
        if dir.join("story.ink").is_file() {
            out.push(dir.to_path_buf());
        }
        for entry in brink_source_tree::Walk::new(dir).flatten() {
            if entry.is_dir() && entry.path().join("story.ink").is_file() {
                out.push(entry.into_path());
            }
        }
    }

    let mut cases = Vec::new();
    collect_story_ink_dirs(&corpus_dir(), &mut cases);
    assert!(
        !cases.is_empty(),
        "expected at least one tests/tier1-brink case"
    );
    for case in cases {
        check_corpus_case(&case);
    }
}

/// Regression pin for issue #880: the three cases that used to need
/// `KNOWN_MUTATOR_WRITE_GAP_CASES` now ground-truth cleanly through the
/// normal, unfiltered corpus walk (`tier1_brink_corpus_never_under_reports_
/// effects` above already covers this — every `tests/tier1-brink/algorithms`
/// case is discovered and walked — but this test names the three explicitly
/// so a future regression on this exact class fails with their names in the
/// test name, not buried in the corpus walk's generic assertion).
#[test]
fn formerly_known_mutator_write_gap_cases_now_ground_truth_cleanly() {
    for name in [
        "knapsack-01",
        "longest-common-subsequence",
        "memoized-fibonacci",
    ] {
        let dir = corpus_dir().join("algorithms").join(name);
        let ink_path = dir.join("story.ink");
        let entry = ink_path.to_string_lossy().into_owned();
        // NS-A9 (strict brink default): this harness ground-truths *effect rows*
        // — a regime-independent subject — over fixtures and inline sources
        // written in the gradual idiom (unannotated params, `VAR x = 0`
        // placeholders). Explicit gradual, same as `algorithms_ground_truth`'s
        // helper contract.
        let options = AnalysisOptions {
            dialect: Dialect::Brink,
            types: Some(brink_compiler::TypePolicy::Gradual),
            ..AnalysisOptions::default()
        };
        let db = build_db(&entry, |p| std::fs::read_to_string(p), options);
        let observed = observe(&entry, &db);
        assert_ground_truth(&entry, &db, &observed);
    }
}

// ── Purpose-built fixtures (issue #870's explicit ask) ───────────────────

/// The exact #866 regression shape (`brink-db`'s
/// `t2_1_effect_rows.rs::effects_query_writes_through_a_ref_param_at_the_
/// call_site` proves the *static* half of the fix; this proves the
/// *ground-truth* half — real bytecode execution — against the same
/// shape): a knot calls `inc(val)` into `inc`'s `ref x` parameter. `knot`'s
/// row must cover the write to `val` (recorded, per `effect_trace`'s
/// attribution model, at `Opcode::PushVarPointer`'s construction site
/// inside `knot`'s own bytecode); `inc`'s own row is never touched by this
/// atom at all — `inc`'s body only ever sees the parameter `x`.
#[test]
fn ref_param_write_ground_truth_matches_the_866_regression_shape() {
    check_source(
        "ref_param_write",
        "VAR val = 5\n-> knot\n\n\
         === knot ===\n~ inc(val)\n{val}\n-> END\n\n\
         === function inc(ref x) ===\n~ x = x + 1\n",
    );
}

/// A nested-path `ref` argument (T1e projections, `Opcode::MakeProjection`)
/// — the same call-site-attribution rule as a bare `ref` global, this time
/// through `Value::Projection` rather than `Value::VariablePointer`.
///
/// The call site spells the projection with an **explicit** `ref`
/// (`heal(ref hero.hp)`) — the only spelling that actually reaches the T1e
/// projection machinery (`hir::Expr::RefArg` →
/// `expr::lower_ref_projection_arg` → `lir::CallArg::RefProjection` →
/// `Opcode::MakeProjection`). As originally written (PR #2690) this test
/// used the *implicit* spelling `heal(hero.hp)`, which has no projection
/// lowering at all: `lower_ref_path_call_arg` resolved the whole path to
/// its ROOT symbol (TM-4b fallback) and emitted `RefGlobal(hero)` — the
/// whole record — so the program faulted at runtime (`type error: cannot
/// apply Add to Record and Int`) on the very first step and the test
/// passed **vacuously** (an `Error` episode with zero steps records
/// nothing to ground-truth). Issue #2185's non-suppressible `E074` now
/// refuses that implicit spelling at compile time (it is the same
/// whole-record misroute the issue fixes for `pop(a.items)`/`a.count++`),
/// which surfaced the vacuity as a compile failure here. With the explicit
/// `ref`, the run is real: `{hero.hp}` prints 15 and the write-back lands
/// in the record field through `Value::Projection`.
#[test]
fn ref_param_write_through_a_path_projection_ground_truth() {
    check_source(
        "ref_param_projection_write",
        "STRUCT Hero = #{\n    hp: int,\n}\n\
         VAR hero = 0\n-> knot\n\n\
         === knot ===\n\
         ~ hero = Hero#{hp: 10}\n\
         ~ heal(ref hero.hp)\n{hero.hp}\n-> END\n\n\
         === function heal(ref hp) ===\n~ hp = hp + 5\n",
    );
}

/// A `ref` argument passed at *root* (story-level) scope, not inside a
/// named knot — `effects()` has no row for root content at all (it isn't a
/// knot/stitch/function), so this is a coverage/regression check that the
/// harness's "skip when `effects()` is `None`" contract (mirroring
/// `ProjectDb::effects`'s own documented contract) doesn't crash or
/// misfire on that shape — root content ends up correctly outside the
/// ground-truth check's scope, not incorrectly flagged.
#[test]
fn ref_param_write_at_root_scope_has_nothing_to_ground_truth() {
    check_source(
        "ref_param_write_root",
        "VAR gold = 100\n\
         ~ heal(gold)\n{gold}\n-> END\n\
         === function heal(ref hp) ===\n~ hp = hp + 5\n",
    );
}

// ── NS-A2 (issue #1108): emits / tags / faults ground truth ─────────────

/// The #1087 motivating shape: a state-pure knot that produces dialogue
/// only *transitively*, through a function call — `teller`'s own bytecode
/// runs the function whose body emits, and the emit atom is observed in
/// `speak`'s scope while `knot`'s static row must cover it transitively.
/// Both defs' rows must admit `emits` or the assertion fails.
#[test]
fn transitive_function_emission_ground_truth() {
    check_source(
        "transitive_emits",
        "-> teller\n\n\
         === teller ===\n~ temp x = speak()\n-> END\n\n\
         === function speak() ===\nThe function narrates.\n~ return 1\n",
    );
}

/// A tag-only definition: tags observed, no emits — the two output
/// dimensions are independent (the 2026-07-18 ruling: a flow that only
/// annotates isn't speaking). The static row must carry `tags`.
#[test]
fn tag_only_line_ground_truth() {
    check_source(
        "tag_only",
        "-> marker\n\n\
         === marker ===\n# checkpoint\nSome text after. # mood: grim\n-> END\n",
    );
}

/// Choice text is host-rendered content: the choice list's fragments record
/// as emits, and choice tags as tags.
#[test]
fn choice_text_and_tags_ground_truth() {
    check_source(
        "choice_output",
        "-> hub\n\n\
         === hub ===\nPick.\n\
         * [Go north] # brave\n    North it is.\n    -> END\n\
         * [Go south]\n    South it is.\n    -> END\n",
    );
}

/// A def that faults (array OOB read) during exploration: the observed
/// fault must be admitted by the static `faults` dimension (harvested from
/// the indexing construct). The story is a straight line into the fault —
/// the explorer records the error episode; the recorder attributes the
/// fault to `boom`'s scope before the turn unwinds.
#[test]
fn oob_index_fault_ground_truth() {
    check_source(
        "oob_fault",
        "-> boom\n\n\
         === boom ===\n~ temp a = #[1, 2]\n~ temp x = a[5]\n{x}\n-> END\n",
    );
}

/// Division by zero — the `/` construct's fault path, observed and covered.
#[test]
fn division_by_zero_fault_ground_truth() {
    check_source(
        "div_fault",
        "VAR d = 0\n-> boom\n\n\
         === boom ===\n~ temp x = 10 / d\n{x}\n-> END\n",
    );
}

/// A silent, total, pure function: its observed row must stay empty on all
/// three new dimensions (regression guard against over-observation — e.g.
/// recording string-eval capture output as an emit).
#[test]
fn pure_silent_total_function_observes_nothing_new() {
    // NS-A9 (strict brink default): this harness ground-truths *effect rows*
    // — a regime-independent subject — over fixtures and inline sources
    // written in the gradual idiom (unannotated params, `VAR x = 0`
    // placeholders). Explicit gradual, same as `algorithms_ground_truth`'s
    // helper contract.
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(brink_compiler::TypePolicy::Gradual),
        ..AnalysisOptions::default()
    };
    let db = build_db(
        "main.ink",
        |_| {
            Ok("-> knot\n\n\
                === knot ===\n~ temp x = add(1, 2)\n{x}\n-> END\n\n\
                === function add(a, b) ===\n~ return a + b\n"
                .to_owned())
        },
        options,
    );
    let observed = observe("pure_fn", &db);
    assert_ground_truth("pure_fn", &db, &observed);
    let index = db.symbol_index();
    let add_id = index
        .by_name
        .get("add")
        .and_then(|ids| ids.first())
        .copied()
        .expect("add def");
    if let Some(row) = observed.get(&add_id) {
        assert!(
            !row.emits && !row.tags && !row.faults,
            "a pure arithmetic function must observe no output/tag/fault \
             atoms, got {row:?}"
        );
    }
}

/// `#fn`/`bind`/`call` indirect dispatch (T1c) — the static analyzer marks
/// any def performing a call *through* a function value as opaque (`spec
/// §3/§4`; `effects_query_is_pessimal_for_a_call_through_a_function_value`
/// pins the pure-function side), and an opaque row covers everything by
/// construction (`EffectRow::covers`), so `apply`'s row trivially passes
/// here. The interesting ground-truth coverage is `bar` — the *concrete*
/// def dispatched to indirectly still gets a real, non-opaque row checked
/// normally, proving the harness follows an indirect call through to a
/// real callee rather than only ever seeing the opaque wrapper.
#[test]
fn fn_value_indirect_call_ground_truth_checks_the_concrete_callee() {
    check_source(
        "fn_value_indirect_call",
        "VAR total = 0\n-> knot\n\n\
         === knot ===\n\
         ~ temp f = #fn(bar)\n\
         ~ temp x = apply(f)\n\
         {total}\n-> END\n\n\
         === function apply(cb) ===\n~ return cb()\n\n\
         === function bar() ===\n~ total = total + 1\n~ return total\n",
    );
}

/// Issue #1755 — the `#fn`-**creation**-site `ref` binding, the aliasing
/// channel `docs/effects-spec.md` §6.1a enumerates as channel 5. `#fn(heal,
/// player_hp)` binds `heal`'s `ref hp` parameter to the cell `player_hp` at
/// creation, a grammar position distinct from a *call* site's `ref` argument
/// (channel 4), and the write happens for real when the created value is
/// later invoked. Attribution is the same construction-site rule this
/// module's own doc states: the `PushVarPointer` for the bound cell is
/// emitted inside `knot`'s bytecode at the `#fn` literal, so `knot` is the
/// def the ground truth charges — and the def whose static row must
/// therefore admit the write. Before #1755's fix `record_ref_param_writes`
/// was never called from `infer_fn_literal`, so `knot`'s static row carried
/// an empty `writes` set while this run wrote `player_hp` — precisely the
/// both-rows-silently-agree-on-too-small under-report this harness exists to
/// catch, and the same shape as #866's original ref-param regression one
/// grammar position over.
#[test]
fn fn_creation_site_ref_binding_ground_truth() {
    check_source(
        "fn_creation_site_ref_binding",
        "VAR player_hp = 10\n-> knot\n\n\
         === knot ===\n\
         ~ temp f = #fn(heal, player_hp)\n\
         ~ temp x = f(5)\n\
         {player_hp}\n-> END\n\n\
         === function heal(ref hp, amount) ===\n\
         ~ hp = hp + amount\n~ return hp\n",
    );
}
