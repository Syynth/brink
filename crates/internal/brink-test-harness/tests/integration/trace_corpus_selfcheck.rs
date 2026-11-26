//! Tier-0 corpus differential (issue #3376,
//! `docs/observable-semantics-spec.md` §4 tier 0).
//!
//! `trace_diff(compile(story), compile(story))` over `tests/tier1`–`tier3`
//! and `tests/tier1-native/` must be empty. Two things are being asserted at
//! once:
//!
//! 1. **The capture is deterministic.** Every observable the trace records
//!    has to be a function of the story, not of allocation addresses, hash
//!    iteration order, or wall-clock time. A `HashMap` iterated somewhere in
//!    the pipeline shows up here as a flapping corpus case.
//! 2. **The compile is deterministic.** The two sides are two *independent*
//!    compiles of the same source, not one compile linked twice, so a
//!    nondeterministic compiler would fail this too.
//!
//! Line-table identity (§2.2) is diffed in the same sweep, since a
//! nondeterministic scope id or text hash is the same class of bug.
//!
//! The bounds are deliberately tighter than the C# oracle sweep's: this test
//! runs in `cargo test --workspace`, and its job is broad shallow coverage of
//! every shape in the corpus, not deep exploration of any one of them.

use std::path::{Path, PathBuf};

use brink_test_harness::corpus::{
    collect_test_cases, compile_entry_to_inkb, has_empty_source, is_compile_error_case,
    native_case_names,
};
use brink_test_harness::trace::{TraceConfig, differential, line_identity_diff};

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
}

/// Broad and shallow: every corpus shape, a few branches deep each.
fn config() -> TraceConfig {
    TraceConfig {
        max_steps: 2_000,
        max_depth: 3,
        max_runs: 12,
        ..TraceConfig::default()
    }
}

/// Floors, so an empty or broken sweep cannot pass vacuously. Deliberately
/// well under the real counts (the corpus grows; this is a floor, not a
/// ratchet — it is not the oracle ratchet and has nothing to do with
/// `RATCHET_EPISODE_COUNT`).
const MIN_INK_CASES: usize = 300;
const MIN_NATIVE_CASES: usize = 25;

struct Sweep {
    compared: usize,
    skipped: Vec<String>,
    failures: Vec<String>,
}

impl Sweep {
    fn new() -> Self {
        Self {
            compared: 0,
            skipped: Vec::new(),
            failures: Vec::new(),
        }
    }

    /// Compile `entry` twice and require both the trace diff and the
    /// line-identity diff to be empty.
    fn check(&mut self, label: &str, entry: &Path) {
        let (p_data, p) = match compile_entry_to_inkb(entry) {
            Ok(pair) => pair,
            Err(e) => {
                self.skipped.push(format!("{label}: {e}"));
                return;
            }
        };
        let (q_data, q) = match compile_entry_to_inkb(entry) {
            Ok(pair) => pair,
            Err(e) => {
                self.failures.push(format!(
                    "{label}: second compile failed but first succeeded: {e}"
                ));
                return;
            }
        };

        match differential(&p, &q, &config()) {
            Ok(diff) if diff.is_empty() => {}
            Ok(diff) => self.failures.push(format!("{label}: {diff}")),
            Err(e) => self
                .failures
                .push(format!("{label}: trace_diff error: {e}")),
        }

        let identity = line_identity_diff(&p_data, &q_data);
        if !identity.is_empty() {
            self.failures
                .push(format!("{label}: line identity: {identity}"));
        }

        self.compared += 1;
    }

    /// A one-line tally the caller prints — `--nocapture` shows how much of
    /// the corpus the sweep actually reached.
    fn summary(&self, what: &str) -> String {
        format!(
            "{what}: compared {} case(s), skipped {}",
            self.compared,
            self.skipped.len()
        )
    }

    fn assert_clean(&self, what: &str, floor: usize) {
        assert!(
            self.failures.is_empty(),
            "{what}: {} case(s) diverged between two compiles of the same source:\n{}",
            self.failures.len(),
            self.failures.join("\n")
        );
        assert!(
            self.compared >= floor,
            "{what}: only {} case(s) compared (floor {floor}); {} skipped:\n{}",
            self.compared,
            self.skipped.len(),
            self.skipped.join("\n")
        );
    }
}

#[test]
fn ink_corpus_is_self_equivalent_under_two_independent_compiles() {
    let root = tests_dir();
    let mut sweep = Sweep::new();
    for tier in ["tier1", "tier2", "tier3"] {
        for case_dir in collect_test_cases(&root.join(tier)) {
            if has_empty_source(&case_dir) || is_compile_error_case(&case_dir) {
                continue;
            }
            let label = case_dir
                .strip_prefix(&root)
                .unwrap_or(&case_dir)
                .display()
                .to_string();
            sweep.check(&label, &case_dir.join("story.ink"));
        }
    }
    println!("{}", sweep.summary("tier1-3 .ink corpus"));
    sweep.assert_clean("tier1-3 .ink corpus", MIN_INK_CASES);
}

#[test]
fn native_corpus_is_self_equivalent_under_two_independent_compiles() {
    let root = tests_dir().join("tier1-native");
    let mut sweep = Sweep::new();
    for name in native_case_names(&root) {
        let entry = root.join(&name).join("story.brink");
        if !entry.exists() {
            continue;
        }
        sweep.check(&format!("tier1-native/{name}"), &entry);
    }
    println!("{}", sweep.summary("tier1-native corpus"));
    sweep.assert_clean("tier1-native corpus", MIN_NATIVE_CASES);
}
