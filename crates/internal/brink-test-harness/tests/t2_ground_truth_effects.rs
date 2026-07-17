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
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let db = build_db(&entry, |p| std::fs::read_to_string(p), options);
    let observed = observe(&entry, &db);
    assert_ground_truth(&entry, &db, &observed);
}

/// Ground-truth one in-memory single-file `source` under the brink
/// dialect, labeled `label` for failure messages.
fn check_source(label: &str, source: &str) {
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
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
    fn collect_story_ink_dirs(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        if dir.join("story.ink").is_file() {
            out.push(dir.to_path_buf());
        }
        let mut subdirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|ft| ft.is_dir()))
            .map(|e| e.path())
            .collect();
        subdirs.sort();
        for sub in subdirs {
            collect_story_ink_dirs(&sub, out);
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
        let options = AnalysisOptions {
            dialect: Dialect::Brink,
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
#[test]
fn ref_param_write_through_a_path_projection_ground_truth() {
    check_source(
        "ref_param_projection_write",
        "STRUCT Hero = #{\n    hp: int,\n}\n\
         VAR hero = 0\n-> knot\n\n\
         === knot ===\n\
         ~ hero = Hero#{hp: 10}\n\
         ~ heal(hero.hp)\n{hero.hp}\n-> END\n\n\
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
