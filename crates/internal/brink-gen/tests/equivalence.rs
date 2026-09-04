//! Source-transformation equivalence over generated stories
//! (`docs/observable-semantics-spec.md` §4.1, the properties the generator
//! was built to serve):
//!
//! - `trace(P) = trace(fmt(P))` — the formatter is a *Safe* transformation
//!   (§5): observably equivalent over every explored run AND
//!   translation-identity preserving (§2.2, `line_identity_diff`), since a
//!   formatter that changed a line's identity would orphan its translations.
//! - `trace(P) = trace(respell(P))` — the ink → `.brink` respeller preserves
//!   behaviour across the surface switch. Identity is NOT required here: the
//!   respelled program is a different source text on a different surface,
//!   and its line hashes legitimately differ. A story the emitter refuses
//!   (`EmitError::Unsupported` — springs, and the other shapes its module
//!   doc lists) is a *skip*, counted and reported, never a failure: the
//!   refusal is the emitter's honest "no faithful spelling", not a
//!   divergence. A story it accepts must then compile and trace equal.
//!
//! Both are per-PR properties (`cargo test -p brink-gen`), sized like the
//! smoke suite; `PROPTEST_CASES` raises the count. Each comparison compiles
//! P and Q from scratch files through the harness's `compile_source_to_inkb`
//! and replays P's explored runs on Q with the trace oracle (`differential`).

// Integration-test convention across the workspace: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr
)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use brink_gen::{Profile, arb_story_with, print_ink};
use brink_respell::RespellError;
use brink_test_harness::corpus::compile_source_to_inkb;
use brink_test_harness::trace::{TraceConfig, differential, line_identity_diff};
use proptest::prelude::*;

const CASES: u32 = 48;

/// `ProptestConfig::with_cases` ignores the `PROPTEST_CASES` environment
/// variable (only `default()` reads it), so the override is applied by hand.
fn config() -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(CASES);
    ProptestConfig {
        cases,
        // A failing story that is expensive to check (an exhaustive
        // exploration near the episode budget runs about a second) must not
        // turn shrinking into a quarter-hour stall: cap the shrink phase.
        max_shrink_time: 60_000,
        ..ProptestConfig::default()
    }
}

/// The trace oracle's bounds for a generated story: deep enough to walk a
/// `Profile::DEFAULT` story's whole choice tree, capped so a pathological
/// shape cannot run away.
fn trace_config() -> TraceConfig {
    TraceConfig {
        max_depth: 24,
        max_runs: 256,
        ..TraceConfig::default()
    }
}

/// How one story fared under the respell property.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RespellOutcome {
    /// Respelled, compiled, traced equal.
    Equal,
    /// The emitter refused the story (`EmitError::Unsupported`), with the
    /// construct it named.
    Refused(String),
    /// Diverged, but on a shape `RESPELL_KNOWN_DIVERGENCES` lists.
    Known(Vec<&'static str>),
}

/// Recognises a known-divergent shape in a printed `.ink` source.
type SourcePredicate = fn(&str) -> bool;

/// Respell divergences brink is KNOWN to have, each keyed on its issue and
/// recognised by a predicate over the source — the differential's
/// `KNOWN_DIVERGENCES` discipline (`inkjs_differential.rs`): a failing story
/// that matches a predicate is counted and reported, not a failure; one
/// that matches none is a new finding. Remove an entry with its fix.
const RESPELL_KNOWN_DIVERGENCES: &[(&str, SourcePredicate)] = &[
    (
        // A text-less fallback choice is emitted as an `else` arm the native
        // parser rejects.
        "#3515 fallback choice respelled as an `else` arm",
        |src| {
            src.lines().any(|l| {
                let t = l.trim_start().trim_start_matches(['*', '+', ' ']);
                t.starts_with("-> ") && l.trim_start().starts_with(['*', '+'])
            })
        },
    ),
    (
        // `not` is emitted as a bare word, and its parenthesised operand is
        // flattened.
        "#3516 `not` respelled as a bare word / operand precedence lost",
        |src| src.contains("not "),
    ),
    (
        // An ink VAR respells to a module-private native `var`, which the
        // host cannot read — item 3 of the trace diverges. Needs a ruling
        // (emit `pub var`?); until then every story with a global matches.
        "#3517 ink VAR respelled as a private native var (host-readable globals)",
        |src| src.lines().any(|l| l.starts_with("VAR ")),
    ),
    (
        // A nested binary expression loses its parentheses (`0 - (0 + 1)`
        // → `0 - 0 + 1`). The generator's printer parenthesises every
        // binary node, so a nested one shows as an operator followed by
        // `(`, or as `((` — or, under a unary operator, as `-(`
        // (`-(0 + 1)` → `-0 + 1`, the first CI run's finding).
        "#3518 nested binary expression loses its parentheses",
        |src| {
            src.contains("((")
                || src.contains("-(")
                || [
                    "+", "-", "*", "/", "mod", "and", "or", "==", "!=", "<", "<=", ">", ">=",
                ]
                .iter()
                .any(|op| src.contains(&format!(" {op} (")))
        },
    ),
];

fn fail(msg: String) -> TestCaseError {
    TestCaseError::fail(msg)
}

fn fmt_preserves_trace_and_identity(story: &brink_gen::Story) -> Result<(), TestCaseError> {
    let src = print_ink(story);
    let formatted = brink_fmt::format(&src, &brink_fmt::FormatConfig::default());
    let (p_data, p) = compile_source_to_inkb("fmt-p", "story.ink", &src)
        .map_err(|e| fail(format!("P failed to compile: {e}\n--- source ---\n{src}")))?;
    let (q_data, q) = compile_source_to_inkb("fmt-q", "story.ink", &formatted).map_err(|e| {
        fail(format!(
            "fmt(P) failed to compile: {e}\n--- source ---\n{src}\n--- fmt(P) ---\n{formatted}"
        ))
    })?;
    let diff = differential(&p, &q, &trace_config())
        .map_err(|e| fail(format!("trace oracle error: {e}\n--- source ---\n{src}")))?;
    prop_assert!(
        diff.is_empty(),
        "fmt(P) diverges from P:\n{diff}\n--- source ---\n{src}\n--- fmt(P) ---\n{formatted}"
    );
    let identity = line_identity_diff(&p_data, &q_data);
    prop_assert!(
        identity.is_empty(),
        "fmt(P) changes line identity:\n{identity}\n--- source ---\n{src}\n--- fmt(P) ---\n{formatted}"
    );
    Ok(())
}

fn respell_preserves_trace(story: &brink_gen::Story) -> Result<RespellOutcome, TestCaseError> {
    let src = print_ink(story);
    let native = match brink_respell::respell_ink_source(&src) {
        Ok(native) => native,
        Err(RespellError::Emit(e)) => {
            // The emitter's honest refusal — reported, not a failure.
            return Ok(RespellOutcome::Refused(e.to_string()));
        }
        Err(e) => {
            return Err(fail(format!(
                "respell failed before emission: {e}\n--- source ---\n{src}"
            )));
        }
    };
    let (_, p) = compile_source_to_inkb("respell-p", "story.ink", &src)
        .map_err(|e| fail(format!("P failed to compile: {e}\n--- source ---\n{src}")))?;
    let (_, q) = compile_source_to_inkb("respell-q", "story.brink", &native).map_err(|e| {
        fail(format!(
            "respell(P) failed to compile: {e}\n--- source ---\n{src}\n--- respell(P) ---\n{native}"
        ))
    })?;
    let diff = differential(&p, &q, &trace_config())
        .map_err(|e| fail(format!("trace oracle error: {e}\n--- source ---\n{src}")))?;
    prop_assert!(
        diff.is_empty(),
        "respell(P) diverges from P:\n{diff}\n--- source ---\n{src}\n--- respell(P) ---\n{native}"
    );
    Ok(RespellOutcome::Equal)
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn formatter_is_safe(story in arb_story_with(Profile::PLAIN_INK)) {
        fmt_preserves_trace_and_identity(&story)?;
    }

}

/// [`respell_preserves_trace`] with the known-divergence allowance.
fn respell_or_known(story: &brink_gen::Story) -> Result<RespellOutcome, TestCaseError> {
    match respell_preserves_trace(story) {
        Ok(outcome) => Ok(outcome),
        Err(e) => {
            let src = print_ink(story);
            let known: Vec<&'static str> = RESPELL_KNOWN_DIVERGENCES
                .iter()
                .filter(|(_, matches)| matches(&src))
                .map(|(issue, _)| *issue)
                .collect();
            if known.is_empty() {
                Err(e)
            } else {
                Ok(RespellOutcome::Known(known))
            }
        }
    }
}

/// Run the respell property over `cases` stories of `profile` and return
/// the outcome tally: (equal, refused-by-reason, known-by-issue). Panics on
/// a genuine divergence, like any proptest failure.
fn respell_tally(
    profile: Profile,
    cases: u32,
) -> (
    usize,
    BTreeMap<String, usize>,
    BTreeMap<&'static str, usize>,
) {
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        cases,
        ..ProptestConfig::default()
    });
    let equal = AtomicUsize::new(0);
    let refused = std::sync::Mutex::new(BTreeMap::new());
    let known = std::sync::Mutex::new(BTreeMap::new());
    runner
        .run(&arb_story_with(profile), |story| {
            match respell_or_known(&story)? {
                RespellOutcome::Equal => {
                    equal.fetch_add(1, Ordering::Relaxed);
                }
                RespellOutcome::Refused(reason) => {
                    // The emitter suffixes the enclosing knot: fold that
                    // away so the tally is by construct.
                    let key = reason
                        .rsplit_once(" (")
                        .map_or(reason.as_str(), |(head, _)| head)
                        .to_owned();
                    *refused.lock().unwrap().entry(key).or_insert(0) += 1;
                }
                RespellOutcome::Known(issues) => {
                    for issue in issues {
                        *known.lock().unwrap().entry(issue).or_insert(0) += 1;
                    }
                }
            }
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{e}"));
    (
        equal.load(Ordering::Relaxed),
        refused.into_inner().unwrap(),
        known.into_inner().unwrap(),
    )
}

fn report(what: &str, cases: u32, tally: &(usize, BTreeMap<String, usize>, BTreeMap<&str, usize>)) {
    let (equal, refused, known) = tally;
    eprintln!("equivalence/{what}: {cases} stories — {equal} traced equal");
    for (reason, n) in refused {
        eprintln!("equivalence/{what}:   refused by the emitter ×{n}: {reason}");
    }
    for (issue, n) in known {
        eprintln!("equivalence/{what}:   known divergence ×{n}: {issue}");
    }
}

/// `trace(P) = trace(respell(P))` over the whole `plain_ink` profile. Most
/// of this profile is currently REFUSED by the native emitter (inline
/// conditionals in content are outside its supported subset, #1951 holes)
/// or carries a global (#3517), so the tally is printed for the record and
/// the property here only demands that nothing the emitter accepts and no
/// unlisted shape diverges. Coverage that is not vacuous is asserted by the
/// structure-tier property below.
#[test]
fn respeller_preserves_the_trace_plain_ink() {
    let cases = config().cases;
    let tally = respell_tally(Profile::PLAIN_INK, cases);
    report("plain_ink", cases, &tally);
}

/// `Profile::RESPELLABLE` — structure only, no inline conditionals — is the
/// subset the emitter supports today: here the property must actually run.
/// A non-vacuity floor of one story in ten tracing equal guards against the
/// respell route regressing into refusing everything; measured 2026-09-04
/// at 72 of 300, with the rest refused for a spring (#1976) or hitting a
/// listed divergence (#3515, #3516, #3518). Raise the floor as those close.
#[test]
fn respeller_preserves_the_trace_respellable_tier() {
    let cases = config().cases;
    let tally = respell_tally(Profile::RESPELLABLE, cases);
    report("respellable", cases, &tally);
    let equal = tally.0;
    let floor = usize::try_from(cases / 10).unwrap_or(0);
    assert!(
        equal >= floor,
        "only {equal} of {cases} respellable-tier stories traced equal through the respeller \
         (floor {floor}) — the property is close to vacuous; see the tally above"
    );
}
