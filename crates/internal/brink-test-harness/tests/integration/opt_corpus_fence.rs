//! The optimizer fence (`docs/optimizer-spec.md` §5.1).
//!
//! For every corpus case and native golden: compile once, optimize a copy, and
//! require the optimized artifact to satisfy every obligation in
//! `brink_test_harness::opt_fence::Obligations`.
//!
//! The control is **the input artifact itself, byte for byte** — no second
//! compilation, no config threaded through two compile roads, no parallel gate.
//! That is the fence this placement buys, and it is strictly stronger than what
//! a LIR-stage optimizer could have had.
//!
//! # This is a sibling, not a duplicate
//!
//! Three sweeps now share the tier-0 oracle, differing only in what `post` is:
//! `trace_corpus_selfcheck.rs` (a second compile), `trace_mutation_study.rs`
//! (a mutant), and this one (`opt(pre)`). Same bounds, same `Sweep` shape.
//!
//! # Why it is not vacuous
//!
//! Four things carry this file:
//!
//! 1. **`changed` and `bytes_identical` must disagree.** The road is
//!    `read_inkb → optimize → write_inkb`. A case no pass touched asserts
//!    `write_inkb ∘ read_inkb == id` over a real corpus artifact — which nothing
//!    else in the tree checks — and a case a pass claims to have changed must
//!    actually come out as different bytes, or the pass is lying about its
//!    outcome.
//! 2. **The change floor.** The resident pass set (`brink_opt::EmitLineNl`)
//!    fuses a shape most stories carry, so a sweep in which nothing changed
//!    means the pass has silently stopped matching. `Floors::changed_cases`
//!    requires a real fraction of the corpus to have been rewritten.
//! 3. **The content floors.** 300 *empty* artifacts would compare clean, so the
//!    sweep sums `ArtifactStats` and requires the corpus to have actually held
//!    something.
//! 4. **`opt_negative_control.rs`** drives the identical `judge()` seam with
//!    deliberately-wrong passes and requires each obligation to go red. That is
//!    what makes greenness here evidence rather than an absence of evidence.
//!
//! The oracle ratchet is untouched and cannot move: the corpus compiles without
//! the optimizer (spec §5.3). Conformance and optimization stop sharing a
//! number, which is a feature of the placement rather than a gap.

use std::path::{Path, PathBuf};

use brink_opt::{ArtifactStats, OptConfig};
use brink_test_harness::corpus::{
    collect_test_cases, compile_entry_to_inkb, has_empty_source, is_compile_error_case,
    native_case_names,
};
use brink_test_harness::opt_fence::judge;
use brink_test_harness::trace::TraceConfig;

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
}

/// Broad and shallow, matching `trace_corpus_selfcheck.rs` exactly: this runs
/// in `cargo test --workspace`, and its job is every shape in the corpus a few
/// branches deep, not deep exploration of any one.
fn config() -> TraceConfig {
    TraceConfig {
        max_steps: 2_000,
        max_depth: 3,
        max_runs: 12,
        ..TraceConfig::default()
    }
}

/// Content floors — the ones specific to this fence.
///
/// Case count alone does not save you: if the compile helper regressed to
/// producing empty artifacts, 300 empty stories would still be "compared" and
/// every obligation would hold trivially. These assert the corpus actually had
/// content to preserve, and are this fence's first real *use* of
/// `ArtifactStats` rather than merely reporting it.
///
/// Per-sweep, because the native corpus is an order of magnitude smaller than
/// tier1-3 and a shared floor would be slack for one and wrong for the other.
struct Floors {
    cases: usize,
    /// Cases the resident pass set must have rewritten (see the module doc).
    changed_cases: usize,
    line_entries: usize,
    containers: usize,
    bytecode_bytes: usize,
}

/// tier1-3. Well under the measured totals — a floor, not a ratchet.
const INK_FLOORS: Floors = Floors {
    cases: 300,
    changed_cases: 150,
    line_entries: 2_000,
    containers: 2_000,
    bytecode_bytes: 50_000,
};

/// tier1-native, which is much smaller.
const NATIVE_FLOORS: Floors = Floors {
    cases: 25,
    changed_cases: 10,
    line_entries: 100,
    containers: 100,
    bytecode_bytes: 2_000,
};

/// Running totals across a sweep, for the content floors.
#[derive(Default)]
struct Totals {
    line_entries: usize,
    containers: usize,
    bytecode_bytes: usize,
}

impl Totals {
    fn add(&mut self, stats: ArtifactStats) {
        self.line_entries += stats.line_entries;
        self.containers += stats.containers;
        self.bytecode_bytes += stats.bytecode_bytes;
    }
}

struct Sweep {
    compared: usize,
    changed: usize,
    skipped: Vec<String>,
    failures: Vec<String>,
    totals: Totals,
}

impl Sweep {
    fn new() -> Self {
        Self {
            compared: 0,
            changed: 0,
            skipped: Vec::new(),
            failures: Vec::new(),
            totals: Totals::default(),
        }
    }

    /// Compile `entry`, optimize it, and require every obligation.
    fn check(&mut self, label: &str, entry: &Path) {
        let (pre_data, pre) = match compile_entry_to_inkb(entry) {
            Ok(pair) => pair,
            Err(e) => {
                self.skipped.push(format!("{label}: {e}"));
                return;
            }
        };

        let verdict = match judge(&pre_data, &pre, &OptConfig::defaults(), &config()) {
            Ok(v) => v,
            Err(e) => {
                self.failures.push(format!("{label}: fence error: {e}"));
                return;
            }
        };

        if !verdict.trace_clean {
            self.failures.push(format!(
                "{label}: optimized artifact diverges:\n{}",
                verdict.detail
            ));
        }
        if !verdict.identity_clean {
            self.failures.push(format!(
                "{label}: optimized artifact orphans translations:\n{}",
                verdict.detail
            ));
        }
        if !verdict.idempotent {
            self.failures
                .push(format!("{label}: opt(opt(A)) != opt(A)"));
        }
        if !verdict.stable {
            self.failures.push(format!(
                "{label}: two optimizer runs over the same input produced different bytes"
            ));
        }
        // Both directions, per the module doc: an untouched case must round-trip
        // byte-for-byte, and a case a pass reports as changed must differ.
        if !verdict.changed && !verdict.bytes_identical {
            self.failures.push(format!(
                "{label}: no pass reported a change, yet the bytes differ. \
                 This is a brink-format round-trip failure (write_inkb . read_inkb != id), \
                 NOT an optimizer failure — file it against brink-format."
            ));
        }
        if verdict.changed && verdict.bytes_identical {
            self.failures.push(format!(
                "{label}: a pass reported a change but the artifact bytes are identical"
            ));
        }
        if verdict.changed {
            self.changed += 1;
        }

        self.totals.add(verdict.before);
        self.compared += 1;
    }

    fn summary(&self, what: &str) -> String {
        format!(
            "{what}: compared {} case(s) ({} rewritten), skipped {} — {} line entries, {} containers, {} bytecode bytes",
            self.compared,
            self.changed,
            self.skipped.len(),
            self.totals.line_entries,
            self.totals.containers,
            self.totals.bytecode_bytes,
        )
    }

    fn assert_clean(&self, what: &str, floors: &Floors) {
        assert!(
            self.failures.is_empty(),
            "{what}: {} case(s) failed an optimizer obligation:\n{}",
            self.failures.len(),
            self.failures.join("\n")
        );
        assert!(
            self.compared >= floors.cases,
            "{what}: only {} case(s) compared (floor {}); {} skipped:\n{}",
            self.compared,
            floors.cases,
            self.skipped.len(),
            self.skipped.join("\n")
        );
        assert!(
            self.changed >= floors.changed_cases,
            "{what}: the resident passes rewrote only {} of {} case(s) (floor {}) — \
             the pass set has stopped matching the corpus",
            self.changed,
            self.compared,
            floors.changed_cases
        );
        assert!(
            self.totals.line_entries >= floors.line_entries,
            "{what}: only {} line entries swept (floor {}) — the sweep reached \
             cases but they carried no translatable content",
            self.totals.line_entries,
            floors.line_entries
        );
        assert!(
            self.totals.containers >= floors.containers,
            "{what}: only {} containers swept (floor {})",
            self.totals.containers,
            floors.containers
        );
        assert!(
            self.totals.bytecode_bytes >= floors.bytecode_bytes,
            "{what}: only {} bytecode bytes swept (floor {})",
            self.totals.bytecode_bytes,
            floors.bytecode_bytes
        );
    }
}

#[test]
fn ink_corpus_survives_the_optimizer() {
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
    sweep.assert_clean("tier1-3 .ink corpus", &INK_FLOORS);
}

#[test]
fn native_corpus_survives_the_optimizer() {
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
    sweep.assert_clean("tier1-native corpus", &NATIVE_FLOORS);
}
