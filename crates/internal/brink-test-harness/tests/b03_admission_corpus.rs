//! B0.3 exit criterion: the entire existing oracle corpus is
//! admission-clean (docs/hir-admission-contract.md §4.2,
//! docs/b0-sequencing.md §B0.3, issue #1172).
//!
//! "This run is the proof the checks encode reality, not aspiration — if a
//! check trips on real corpus code, the CHECK is wrong (or reality differs
//! from the contract — flag it), do not 'fix' the corpus" (the sequencing
//! doc's own words). Scope: the same 390-case oracle corpus
//! `collect_oracle_cases` feeds to `oracle_snapshots` — the corpus this
//! project's own vocabulary means by "the entire existing corpus" (CLAUDE.md
//! "390 cases total"), not the much larger scraped `tests/tests_github`
//! ink-in-the-wild collection (which is not oracle-gated and out of this
//! slice's scope).
//!
//! Every `.ink` file reachable from each case directory is loaded into a
//! `ProjectDb` and checked independently — admission is a per-file
//! invariant (it never reads cross-file state), so no `INCLUDE`
//! graph/entry-point wiring is needed, just every file present.
//!
//! Also carries the NF-6 salsa hot-path perf measurement: `validate_admission`
//! runs on every lowering (always-on), so its cost as a function of file
//! size must stay linear (or log-linear) — see `perf_scales_linearly_with_file_size`.

#![allow(
    clippy::cast_precision_loss,
    reason = "us/byte and ratio math for a printed perf figure — losing bits past 2^52 in a \
              microsecond count or byte length that never approaches that scale is irrelevant"
)]

use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

use brink_db::ProjectDb;
use brink_test_harness::corpus::collect_oracle_cases;

fn tests_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
}

/// Every `.ink` file directly inside `dir` (corpus case directories are
/// flat — `story.ink` plus any sibling `INCLUDE`d files — no nested `.ink`
/// subdirectories in the tier1/2/3 corpus).
fn ink_files_in(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ink"))
        .collect();
    out.sort();
    out
}

#[test]
fn entire_oracle_corpus_is_admission_clean() {
    let root = tests_dir();
    let cases = collect_oracle_cases(&root);
    assert!(
        !cases.is_empty(),
        "expected to find oracle cases under {root:?}"
    );

    let mut db = ProjectDb::new();
    let mut failures: Vec<String> = Vec::new();
    let mut files_checked = 0usize;

    for case_dir in &cases {
        for ink_path in ink_files_in(case_dir) {
            let Ok(source) = std::fs::read_to_string(&ink_path) else {
                continue;
            };
            // Unique path per file: relative to the corpus root so
            // same-named files in different case dirs don't collide in the
            // single shared `ProjectDb`.
            let rel = ink_path
                .strip_prefix(&root)
                .unwrap_or(&ink_path)
                .to_string_lossy()
                .into_owned();
            let file_id = db.set_file(&rel, source);
            files_checked += 1;
            let diags = db.admission_diagnostics(file_id).unwrap_or(&[]);
            if !diags.is_empty() {
                failures.push(format!(
                    "{}: {} admission diagnostic(s): {:?}",
                    ink_path.display(),
                    diags.len(),
                    diags
                        .iter()
                        .map(|d| (d.code.as_str(), d.range))
                        .collect::<Vec<_>>()
                ));
            }
        }
    }

    assert!(
        !failures.is_empty() || files_checked > 0,
        "checked zero files — corpus discovery is broken"
    );
    assert!(
        failures.is_empty(),
        "{} of {} corpus files are NOT admission-clean:\n{}",
        failures.len(),
        files_checked,
        failures.join("\n")
    );

    let case_count = cases.len();
    eprintln!(
        "B0.3 admission corpus gate: {files_checked} files across {case_count} oracle cases, 0 diagnostics"
    );
}

/// NF-6 perf budget: `validate_admission` is always-on (runs on every
/// lowering, including every editor keystroke via `lowered_query`), so its
/// cost must scale linearly (or log-linearly) with file size, not
/// quadratically. Generates synthetic sources at increasing knot counts,
/// times only `validate_admission` itself (parsing/lowering excluded — this
/// isolates the pass's own added cost on the hot path), and asserts the
/// per-knot cost stays roughly flat rather than growing with input size.
#[test]
fn perf_scales_linearly_with_file_size() {
    fn synthetic_source(knots: usize) -> String {
        let mut s = String::from("VAR counter = 0\n");
        for i in 0..knots {
            let _ = write!(
                s,
                "== knot_{i} ==\n\
                 ~ counter += 1\n\
                 Some narrative content mentioning {{counter}} for knot {i}.\n\
                 {{counter > 0: -> knot_{i}.stitch_a | -> knot_{i}.stitch_a}}\n\
                 = stitch_a\n\
                 More content in the stitch.\n\
                 * [Choice A] -> knot_{i}.stitch_a\n\
                 * [Choice B] -> END\n\
                 -> END\n"
            );
        }
        s
    }

    fn timed_admission_micros(knots: usize) -> (u128, usize) {
        let src = synthetic_source(knots);
        let parsed = brink_syntax::parse(&src);
        let tree = parsed.tree();
        let (hir, manifest, _diags) = brink_ir::hir::lower(brink_ir::FileId(0), &tree);
        let file_len = parsed.syntax().text_range().end();

        // Warm up (page faults, allocator) then take the min of several
        // runs to reduce noise — this is a budget check, not a benchmark.
        let mut best = u128::MAX;
        for _ in 0..5 {
            let start = Instant::now();
            let diags =
                brink_analyzer::validate_admission(brink_ir::FileId(0), &hir, &manifest, file_len);
            let elapsed = start.elapsed().as_micros();
            assert!(
                diags.is_empty(),
                "synthetic source should be admission-clean"
            );
            best = best.min(elapsed);
        }
        (best, src.len())
    }

    let sizes = [50usize, 200, 800, 3200];
    let mut results = Vec::new();
    for &n in &sizes {
        let (micros, bytes) = timed_admission_micros(n);
        results.push((n, bytes, micros));
        eprintln!(
            "B0.3 perf: {n} knots, {bytes} bytes source -> validate_admission {micros} us \
             ({:.3} us/byte)",
            f64::from(u32::try_from(micros).unwrap_or(u32::MAX)) / bytes as f64
        );
    }

    // O(n)/O(n log n) budget check: from the smallest to the largest size
    // (64x more knots), wall-clock time must not grow super-linearly beyond
    // a generous constant-factor allowance for noise/log-linear terms
    // (BTreeSet/HashSet-free — this pass has no sorting step at all, so
    // linear is the honest expectation; the multiplier just absorbs
    // measurement jitter on a shared/virtualized CI box).
    let (small_n, _small_bytes, small_us) = results[0];
    let (large_n, _large_bytes, large_us) = results[results.len() - 1];
    let size_ratio = large_n as f64 / small_n as f64;
    let time_ratio = (large_us.max(1)) as f64 / (small_us.max(1)) as f64;
    assert!(
        time_ratio < size_ratio * 4.0,
        "validate_admission does not look linear: {size_ratio:.1}x the input took \
         {time_ratio:.1}x the time (small={small_us}us large={large_us}us) — \
         budget is a 4x allowance over linear"
    );
}
