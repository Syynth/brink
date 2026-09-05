//! The optimizer fence's negative control.
//!
//! `opt_corpus_fence.rs` is green. With an empty pass list, so would be a fence
//! that compared nothing at all. **This file is what tells those two apart.**
//!
//! Each control pass in `brink_opt::control` is a deliberately-wrong transform
//! targeting exactly one obligation. Every one is driven through the same
//! `opt_fence::judge()` seam the real fence uses, so a green fence and a red
//! control are statements about the same code path — which is the whole point.
//! A control that went red through some other route would prove only that the
//! diff functions work, not that the fence is wired to them.
//!
//! # The matrix
//!
//! | pass | trace | identity | idempotent | stable |
//! |---|---|---|---|---|
//! | *(none — the real fence)* | clean | clean | yes | yes |
//! | `control:retext` | **DIRTY** | clean | yes | yes |
//! | `control:rehash` | clean | **DIRTY** | yes | yes |
//! | `control:grow` | dirty | clean | **NO** | yes |
//! | `control:drift` | clean | clean | no | **NO** |
//!
//! Read the columns rather than the rows. Trace is tripped by `retext` and
//! `grow` and by nothing else; identity by `rehash` alone; stability by `drift`
//! alone. **Every column has a red cell**, so no assertion in the fence can be
//! quietly dead.
//!
//! `retext`/`rehash` are the pair that matters most: they prove the two
//! semantic oracles are *independently* wired. A single control tripping both
//! would prove neither, since either oracle could be doing all the work. They
//! are separable because `line_identity_diff` compares only
//! `(scope_id, index, source_hash)` and never reads `content`, while the
//! runtime reads `content` and never reads `source_hash`.
//!
//! `drift` is what justifies keeping byte-level checks alongside the semantic
//! oracles at all: it perturbs the artifact in a way neither can see.

use std::path::PathBuf;

use brink_opt::control;
use brink_test_harness::corpus::{
    collect_test_cases, compile_entry_to_inkb, compile_source_to_inkb, has_empty_source,
    is_compile_error_case,
};
use brink_test_harness::opt_fence::{Obligations, has_line_entries, is_line_text_grounded, judge};
use brink_test_harness::trace::{LineIdentityChange, TraceConfig};

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
}

fn config() -> TraceConfig {
    TraceConfig {
        max_steps: 2_000,
        max_depth: 3,
        max_runs: 12,
        ..TraceConfig::default()
    }
}

/// A control is only meaningfully "caught" on a case it could actually perturb.
/// Floor per control across the tier1 sweep, so "no survivors" cannot be
/// vacuously true because the control was never grounded anywhere.
const MIN_CONTROL_KILLS: usize = 100;

// ── Fixture half: the matrix, asserted exactly ──────────────────────────────

/// Three shapes, inline — no new files on disk. Between them they cover plain
/// text, a choice, and a global read.
const FIXTURES: [(&str, &str); 3] = [
    ("plain", "Hello there.\nAnd again.\n-> END\n"),
    ("choice", "-> k\n\n=== k ===\nA line.\n+ [Go on] -> k\n"),
    (
        "global",
        "VAR n = 3\n-> k\n\n=== k ===\nThe count is {n}.\n-> END\n",
    ),
];

/// The row of the matrix a control must produce.
///
/// Four bools by design — one per obligation, mirroring `Obligations` — so a
/// control declares its exact row rather than "something went red".
#[expect(
    clippy::struct_excessive_bools,
    reason = "one flag per obligation; this IS the matrix row"
)]
struct Row {
    trace_clean: bool,
    identity_clean: bool,
    idempotent: bool,
    stable: bool,
}

const ROWS: [(&str, Row); 4] = [
    (
        "control:retext",
        Row {
            trace_clean: false,
            identity_clean: true,
            idempotent: true,
            stable: true,
        },
    ),
    (
        "control:rehash",
        Row {
            trace_clean: true,
            identity_clean: false,
            idempotent: true,
            stable: true,
        },
    ),
    (
        "control:grow",
        Row {
            trace_clean: false,
            identity_clean: true,
            idempotent: false,
            stable: true,
        },
    ),
    (
        "control:drift",
        Row {
            trace_clean: true,
            identity_clean: true,
            idempotent: false,
            stable: false,
        },
    ),
];

fn describe(v: &Obligations) -> String {
    format!(
        "trace_clean={} identity_clean={} idempotent={} stable={}",
        v.trace_clean, v.identity_clean, v.idempotent, v.stable
    )
}

/// The document of "the obligations are independently wired".
///
/// Every control against every fixture must produce its exact row — not merely
/// "something went red". An `assert_eq!` per cell, so a control that starts
/// tripping an extra oracle is a failure rather than a silent widening.
#[test]
fn control_matrix_holds_on_fixtures() {
    let mut checked = 0;
    for (label, source) in FIXTURES {
        let (pre_data, pre) = compile_source_to_inkb("opt-control", "story.ink", source)
            .map_err(|e| format!("{label}: compile failed: {e}"))
            .expect("fixture compiles");

        // The baseline: no passes, everything clean. If this is not true the
        // rows below say nothing.
        let base = judge(
            &pre_data,
            &pre,
            &brink_opt::OptConfig::defaults(),
            &config(),
        )
        .map_err(|e| format!("{label}: fence error: {e}"))
        .expect("the fence runs");
        assert!(
            base.all_clean() && (base.changed != base.bytes_identical),
            "{label}: the resident pass set must be clean, and byte-identical \
             exactly when it reports no change, got {}",
            describe(&base)
        );

        for (name, row) in &ROWS {
            let v = judge(&pre_data, &pre, &control::config(name), &config())
                .map_err(|e| format!("{label}/{name}: fence error: {e}"))
                .expect("the fence runs");
            assert_eq!(
                v.trace_clean,
                row.trace_clean,
                "{label}/{name}: trace_clean — got {}",
                describe(&v)
            );
            assert_eq!(
                v.identity_clean,
                row.identity_clean,
                "{label}/{name}: identity_clean — got {}",
                describe(&v)
            );
            assert_eq!(
                v.idempotent,
                row.idempotent,
                "{label}/{name}: idempotent — got {}",
                describe(&v)
            );
            assert_eq!(
                v.stable,
                row.stable,
                "{label}/{name}: stable — got {}",
                describe(&v)
            );
            checked += 1;
        }
    }
    assert_eq!(
        checked,
        FIXTURES.len() * ROWS.len(),
        "every fixture must be judged against every control"
    );
}

/// `control:rehash` must produce `HashChanged` specifically — not a scope or
/// line appearing on one side only. "It tripped line identity" is not the
/// claim; "it moved the hash and nothing else about identity" is.
#[test]
fn rehash_moves_hashes_and_not_the_shape_of_the_line_table() {
    let (pre_data, pre) = compile_source_to_inkb(
        "opt-control-variant",
        "story.ink",
        "Hello there.\nAnd again.\n-> END\n",
    )
    .expect("compile");

    let v = judge(
        &pre_data,
        &pre,
        &control::config("control:rehash"),
        &config(),
    )
    .expect("fence");

    assert!(!v.identity_clean, "rehash must trip line identity");
    assert!(
        !v.identity.changes.is_empty(),
        "the diff must carry the changes it reported"
    );
    for change in &v.identity.changes {
        assert!(
            matches!(change, LineIdentityChange::HashChanged { .. }),
            "rehash must only produce HashChanged, got {change:?}"
        );
    }
}

// ── Corpus half: each control, swept over tier1 ─────────────────────────────

/// What a control did across the sweep.
#[derive(Default)]
struct Tally {
    built: usize,
    grounded: usize,
    killed: usize,
    inert: usize,
    survivors: Vec<String>,
}

impl Tally {
    fn report(&self, name: &str) -> String {
        format!(
            "{name}: built {}, grounded {}, killed {}, inert {}, survivors {}",
            self.built,
            self.grounded,
            self.killed,
            self.inert,
            self.survivors.len()
        )
    }

    fn assert_caught(&self, name: &str) {
        assert!(
            self.survivors.is_empty(),
            "{name}: {} case(s) were NOT caught by the fence — the obligation \
             this control targets is not wired:\n{}",
            self.survivors.len(),
            self.survivors.join("\n")
        );
        assert!(
            self.killed >= MIN_CONTROL_KILLS,
            "{name}: only {} case(s) actually exercised the control (floor \
             {MIN_CONTROL_KILLS}). \"No survivors\" is vacuous if the control \
             was never grounded — built {}, grounded {}, inert {}",
            self.killed,
            self.built,
            self.grounded,
            self.inert
        );
    }
}

/// How a control is grounded — the site it edits must be something the case
/// actually has, in the `mutate.rs` discipline. An ungrounded control survives
/// because nothing looked, which says nothing about the fence.
#[derive(Clone, Copy)]
enum Grounding {
    /// The case's explored runs emit text that a line-table entry supplies.
    /// Stricter than "emits any text" — see `is_line_text_grounded`.
    Text,
    /// The case has line-table entries at all (static and exact).
    LineEntries,
}

/// Sweep tier1 with one control, checking `caught` on every grounded case.
fn sweep(name: &str, grounding: Grounding, caught: fn(&Obligations) -> bool) -> Tally {
    let root = tests_dir();
    let mut tally = Tally::default();
    for case_dir in collect_test_cases(&root.join("tier1")) {
        if has_empty_source(&case_dir) || is_compile_error_case(&case_dir) {
            continue;
        }
        let label = case_dir
            .strip_prefix(&root)
            .unwrap_or(&case_dir)
            .display()
            .to_string();
        let Ok((pre_data, pre)) = compile_entry_to_inkb(&case_dir.join("story.ink")) else {
            continue;
        };
        tally.built += 1;

        let grounded = match grounding {
            Grounding::Text => is_line_text_grounded(&pre_data, &pre, &config()).unwrap_or(false),
            Grounding::LineEntries => has_line_entries(&pre_data),
        };
        if !grounded {
            tally.inert += 1;
            continue;
        }
        tally.grounded += 1;

        let Ok(verdict) = judge(&pre_data, &pre, &control::config(name), &config()) else {
            tally.inert += 1;
            continue;
        };
        // A control that found nothing to edit is inert, never a survivor.
        if !verdict.changed {
            tally.inert += 1;
        } else if caught(&verdict) {
            tally.killed += 1;
        } else {
            tally.survivors.push(label);
        }
    }
    tally
}

/// Grounding is decided by whole-run containment, not substring containment.
///
/// A generated story whose only rendered text was `beta` carried a one-letter
/// choice label `[a]` on a stitch its runs never reached; `"beta".contains("a")`
/// grounded it, the retext control could not be observed, and
/// `the_generator_produces_stories_the_oracle_can_distinguish` failed one run in
/// six. The predicate must say "ungrounded" here *before* the verdict — never
/// because the verdict came back clean.
#[test]
fn a_one_letter_label_does_not_ground_the_retext_control_by_substring() {
    let src = "-> k\n\n=== k ===\n{\"beta\"}\n-> END\n\n= s\n+ [a]\n    -> END\n";
    let (data, pre) =
        compile_source_to_inkb("grounding-substring", "story.ink", src).expect("compiles");
    assert!(has_line_entries(&data), "the label is a line-table entry");
    assert!(
        !is_line_text_grounded(&data, &pre, &config()).expect("explores"),
        "`a` inside `beta` is not the label being rendered"
    );
    // And the control does survive here — which is exactly why the predicate
    // must not have counted the story as grounded.
    let v = judge(&data, &pre, &control::config("control:retext"), &config()).expect("judge");
    assert!(
        v.trace_clean,
        "the runs never reach the label, so retext is unobservable: {}",
        describe(&v)
    );
}

#[test]
fn retext_control_is_caught_by_the_trace_and_not_by_line_identity() {
    let tally = sweep("control:retext", Grounding::Text, |v| {
        !v.trace_clean && v.identity_clean
    });
    println!("{}", tally.report("control:retext"));
    tally.assert_caught("control:retext");
}

#[test]
fn rehash_control_is_caught_by_line_identity_and_not_by_the_trace() {
    let tally = sweep("control:rehash", Grounding::LineEntries, |v| {
        !v.identity_clean && v.trace_clean
    });
    println!("{}", tally.report("control:rehash"));
    tally.assert_caught("control:rehash");
}

#[test]
fn grow_control_is_caught_by_the_idempotence_check() {
    let tally = sweep("control:grow", Grounding::Text, |v| !v.idempotent);
    println!("{}", tally.report("control:grow"));
    tally.assert_caught("control:grow");
}

/// The one that justifies the byte-level checks: `drift` perturbs the artifact
/// in a way **neither semantic oracle can see**, so only `stable` catches it.
#[test]
fn drift_control_is_caught_by_the_stability_check_alone() {
    let tally = sweep("control:drift", Grounding::LineEntries, |v| {
        !v.stable && v.trace_clean && v.identity_clean
    });
    println!("{}", tally.report("control:drift"));
    tally.assert_caught("control:drift");
}
