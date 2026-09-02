//! Mutation-sensitivity study for the equivalence oracle (issue #3376,
//! `docs/observable-semantics-spec.md` §4 tier 3a).
//!
//! Small semantic mutations are applied to programs and
//! [`differential`] must detect **every** one. A surviving mutant is a blind
//! spot in §2's definition or in the instrumentation that computes it — the
//! spec's standing instruction is to fix the oracle, and if the *definition*
//! is wrong, to stop and report rather than weaken the test.
//!
//! Two halves, because they buy different things:
//!
//! - **`fixture_mutants`** covers all seven mutation classes the spec names,
//!   on purpose-built `.ink` and `.brink` fixtures where the mutation is
//!   grounded by construction. This is the part that can cover
//!   `flip-condition`, `reorder-list` and `remove-random-draw`, which no
//!   mechanical text mutator can ground reliably.
//! - **`corpus_mutants`** runs the four mechanically-groundable mutators over
//!   the real `tests/tier1` corpus. Breadth over shapes nobody wrote a
//!   fixture for.
//!
//! **Grounding** is what makes a survivor meaningful. A mutant whose site no
//! explored run ever reaches survives because bounded exploration never
//! looked — that says nothing about the definition. So every corpus mutant is
//! emitted only where the baseline trace demonstrably exercised the site (see
//! `brink_test_harness::mutate::Coverage`).
//!
//! Run with `--nocapture` to see the per-class survivor rate.

use std::collections::BTreeMap;
use std::path::PathBuf;

use brink_test_harness::corpus::{
    collect_test_cases, compile_source_to_inkb, has_empty_source, is_compile_error_case,
};
use brink_test_harness::mutate::{Coverage, MutationClass, grounded_mutants};
use brink_test_harness::trace::{LinkedProgram, TraceConfig, differential, explore_traces};

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

/// Per-class tally: how many mutants were built, how many compiled, how many
/// the oracle killed, and which ones survived.
#[derive(Default)]
struct Tally {
    built: usize,
    compiled: usize,
    killed: usize,
    survivors: Vec<String>,
}

struct Study {
    per_class: BTreeMap<MutationClass, Tally>,
}

impl Study {
    fn new() -> Self {
        Self {
            per_class: BTreeMap::new(),
        }
    }

    /// Compile `mutant` and require the oracle to tell it apart from
    /// `baseline`.
    fn judge(
        &mut self,
        class: MutationClass,
        label: &str,
        file: &str,
        baseline: &[u8],
        mutant_source: &str,
        config: &TraceConfig,
    ) {
        let tally = self.per_class.entry(class).or_default();
        tally.built += 1;
        let Ok((_, mutant)) = compile_source_to_inkb("mutant", file, mutant_source) else {
            // A mutant that does not compile is not a mutant — the mutators
            // are textual, so this is expected and is not a survivor.
            return;
        };
        tally.compiled += 1;
        match differential(baseline, &mutant, config) {
            Ok(diff) if diff.is_empty() => tally.survivors.push(label.to_owned()),
            Ok(_) => tally.killed += 1,
            Err(e) => tally.survivors.push(format!("{label}: oracle error: {e}")),
        }
    }

    /// The per-class survivor-rate table the caller prints. This is the
    /// number the PR body reports.
    fn report(&self, what: &str) -> String {
        use std::fmt::Write as _;
        let mut out = format!("\n=== mutation-sensitivity study: {what} ===\n");
        for class in MutationClass::ALL {
            let Some(tally) = self.per_class.get(&class) else {
                continue;
            };
            let rate = if tally.compiled == 0 {
                0.0
            } else {
                let survived = tally.compiled - tally.killed;
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "counts are small; this is a printed percentage"
                )]
                let rate = (survived as f64 / tally.compiled as f64) * 100.0;
                rate
            };
            let _ = writeln!(
                out,
                "  {:<20} built {:>4}  compiled {:>4}  killed {:>4}  survivor rate {rate:>5.1}%",
                class.label(),
                tally.built,
                tally.compiled,
                tally.killed
            );
        }
        out
    }

    /// Fail on any survivor.
    fn assert_no_survivors(&self, what: &str) {
        let mut survivors = Vec::new();
        for class in MutationClass::ALL {
            let Some(tally) = self.per_class.get(&class) else {
                continue;
            };
            for s in &tally.survivors {
                survivors.push(format!("{}: {s}", class.label()));
            }
        }
        assert!(
            survivors.is_empty(),
            "{what}: {} mutant(s) survived the oracle. Per \
             docs/observable-semantics-spec.md §4 tier 3a a survivor is a blind spot in the \
             definition or its instrumentation — fix the oracle, do not weaken this test:\n{}",
            survivors.len(),
            survivors.join("\n")
        );
    }

    /// Every class the study claims to cover must actually have produced at
    /// least one compiling mutant, or the report is a comfortable lie.
    fn assert_covers(&self, classes: &[MutationClass]) {
        for class in classes {
            let compiled = self.per_class.get(class).map_or(0, |t| t.compiled);
            assert!(
                compiled > 0,
                "mutation class {} produced no compiling mutant — the study does not \
                 actually cover it",
                class.label()
            );
        }
    }
}

/// One purpose-built fixture: a base program and a single semantic edit.
struct Fixture {
    class: MutationClass,
    label: &'static str,
    file: &'static str,
    base: &'static str,
    mutant: &'static str,
    /// Seeds the comparison runs under; `remove-random-draw` needs several,
    /// because equivalence must hold under every seed.
    seeds: &'static [i32],
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        class: MutationClass::SwapChoices,
        label: "ink/swap-two-choices",
        file: "story.ink",
        base: "Pick.\n* [North] You go north.\n    -> END\n* [South] You go south.\n    -> END\n",
        mutant: "Pick.\n* [South] You go south.\n    -> END\n* [North] You go north.\n    -> END\n",
        seeds: &[],
    },
    Fixture {
        class: MutationClass::DropLine,
        label: "ink/drop-a-text-line",
        file: "story.ink",
        base: "One.\nTwo.\nThree.\n-> END\n",
        mutant: "One.\nThree.\n-> END\n",
        seeds: &[],
    },
    Fixture {
        class: MutationClass::FlipCondition,
        label: "ink/flip-a-condition",
        file: "story.ink",
        base: "VAR gold = 7\n{gold > 5: Rich.|Poor.}\n-> END\n",
        mutant: "VAR gold = 7\n{gold < 5: Rich.|Poor.}\n-> END\n",
        seeds: &[],
    },
    Fixture {
        class: MutationClass::ReorderList,
        label: "ink/reorder-a-list-declaration",
        file: "story.ink",
        base: "LIST Dir = north, south, east\nVAR here = north\nValue {LIST_VALUE(here)}\n-> END\n",
        mutant: "LIST Dir = south, north, east\nVAR here = north\nValue {LIST_VALUE(here)}\n-> END\n",
        seeds: &[],
    },
    Fixture {
        class: MutationClass::ChangeLiteral,
        label: "ink/change-a-literal",
        file: "story.ink",
        base: "VAR gold = 3\nYou have {gold}.\n-> END\n",
        mutant: "VAR gold = 4\nYou have {gold}.\n-> END\n",
        seeds: &[],
    },
    Fixture {
        class: MutationClass::RemoveRandomDraw,
        label: "ink/remove-an-unused-random-draw",
        file: "story.ink",
        base: "~ temp junk = RANDOM(1, 6)\nYou rolled {RANDOM(1, 100)}.\n-> END\n",
        mutant: "You rolled {RANDOM(1, 100)}.\n-> END\n",
        seeds: &[1, 2, 3, 4, 5, 6, 7, 8],
    },
    Fixture {
        class: MutationClass::ChangeGlobalWrite,
        label: "ink/change-a-write-to-a-global",
        file: "story.ink",
        base: "VAR gold = 0\n~ gold = 5\nDone.\n-> END\n",
        mutant: "VAR gold = 0\n~ gold = 6\nDone.\n-> END\n",
        seeds: &[],
    },
    Fixture {
        class: MutationClass::DropLine,
        label: "brink/drop-a-text-line",
        file: "story.brink",
        base: "flow main() {\n  One.\n  Two.\n  Three.\n  -> END\n}\n",
        mutant: "flow main() {\n  One.\n  Three.\n  -> END\n}\n",
        seeds: &[],
    },
    Fixture {
        class: MutationClass::ChangeLiteral,
        label: "brink/change-a-public-global-literal",
        file: "story.brink",
        base: "pub var gold = 3\n\nflow main() {\n  Hello.\n  -> END\n}\n",
        mutant: "pub var gold = 4\n\nflow main() {\n  Hello.\n  -> END\n}\n",
        seeds: &[],
    },
    Fixture {
        class: MutationClass::ChangeGlobalWrite,
        label: "brink/change-a-write-to-a-public-global",
        file: "story.brink",
        base: "pub var gold = 0\n\nflow main() {\n  ~ gold = 5\n  Done.\n  -> END\n}\n",
        mutant: "pub var gold = 0\n\nflow main() {\n  ~ gold = 6\n  Done.\n  -> END\n}\n",
        seeds: &[],
    },
];

#[test]
fn fixture_mutants_are_all_detected() {
    let mut study = Study::new();
    for fixture in FIXTURES {
        let base = compile_source_to_inkb("base", fixture.file, fixture.base);
        assert!(
            base.is_ok(),
            "{}: base fixture must compile: {base:?}",
            fixture.label
        );
        let (_, base) = base.expect("just asserted the base fixture compiles");
        let config = TraceConfig {
            seeds: fixture.seeds.to_vec(),
            ..config()
        };
        study.judge(
            fixture.class,
            fixture.label,
            fixture.file,
            &base,
            fixture.mutant,
            &config,
        );
    }
    study.assert_covers(&MutationClass::ALL);
    println!(
        "{}",
        study.report("purpose-built fixtures (all seven classes)")
    );
    study.assert_no_survivors("purpose-built fixtures (all seven classes)");
}

/// Mutants per class per corpus case, and the ceiling on the whole sweep —
/// bounds on runtime, not on the study's meaning.
const PER_CLASS_PER_CASE: usize = 2;
const MAX_CORPUS_MUTANTS: usize = 400;

#[test]
fn grounded_corpus_mutants_are_all_detected() {
    let root = tests_dir();
    let mut study = Study::new();
    let mut total = 0usize;
    let mut cases_with_mutants = 0usize;

    for case_dir in collect_test_cases(&root.join("tier1")) {
        if total >= MAX_CORPUS_MUTANTS {
            break;
        }
        if has_empty_source(&case_dir) || is_compile_error_case(&case_dir) {
            continue;
        }
        let entry = case_dir.join("story.ink");
        let Ok(source) = std::fs::read_to_string(&entry) else {
            continue;
        };
        // INCLUDEs would not resolve from the scratch directory the mutants
        // are compiled in, so single-file cases only.
        if source.contains("INCLUDE ") {
            continue;
        }
        let Ok((_, baseline)) = compile_source_to_inkb("baseline", "story.ink", &source) else {
            continue;
        };
        let Ok(linked) = LinkedProgram::from_inkb(&baseline) else {
            continue;
        };
        let Ok(traces) = explore_traces(&linked, &config()) else {
            continue;
        };
        let coverage = Coverage::of(&traces);
        let mutants = grounded_mutants(&source, &coverage, PER_CLASS_PER_CASE);
        if mutants.is_empty() {
            continue;
        }
        cases_with_mutants += 1;
        let label_base = case_dir
            .strip_prefix(&root)
            .unwrap_or(&case_dir)
            .display()
            .to_string();
        for mutant in mutants {
            if total >= MAX_CORPUS_MUTANTS {
                break;
            }
            total += 1;
            study.judge(
                mutant.class,
                &format!("{label_base}: {}", mutant.description),
                "story.ink",
                &baseline,
                &mutant.source,
                &config(),
            );
        }
    }

    println!("corpus cases yielding grounded mutants: {cases_with_mutants}");
    study.assert_covers(&[
        MutationClass::SwapChoices,
        MutationClass::DropLine,
        MutationClass::ChangeLiteral,
    ]);
    println!("{}", study.report("grounded mutants over tests/tier1"));
    study.assert_no_survivors("grounded mutants over tests/tier1");
}
