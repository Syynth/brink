//! The inkjs differential (issue #3379, `docs/program-generator-spec.md`
//! §6): for generated `plain_ink` stories, brink's explored episodes must
//! match the reference's — `trace(brink(P)) == trace(inkjs(P))` over every
//! choice path, compared by `brink_test_harness::diff_oracle`, the same
//! comparison the corpus ratchet applies to a C# golden.
//!
//! The reference is `tools/inkjs-oracle` (a port of the C# crawler onto
//! inkjs with the .NET `System.Random` generator installed), which
//! `crates/internal/brink-test-harness/tests/inkjs_sanction.rs` proves
//! reproduces every checked-in C# golden before it is allowed to judge
//! anything here. Where brink and inkjs disagree, the C# runtime is the
//! tie-breaker (maintainer-local, `dotnet`): minimise the story and add it
//! to the corpus as an oracle case, per the 2026-09-02 directive.
//!
//! Opt-in: `BRINK_INKJS_ORACLE=1`; `PROPTEST_CASES` overrides the count.
//!
//! ```sh
//! BRINK_INKJS_ORACLE=1 cargo test -p brink-gen --test inkjs_differential -- --nocapture
//! ```

// Integration-test convention across the workspace: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr
)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use brink_gen::{Profile, arb_story_with, print_ink};
use brink_test_harness::{ExploreConfig, inkjs};
use proptest::prelude::*;

/// Default case count; the nightly lane raises it through `PROPTEST_CASES`
/// (§7). Each case spawns one `node` (~100ms) on top of brink's own
/// compile-and-explore.
const CASES: u32 = 32;

/// Divergences brink is KNOWN to have from the reference, each keyed on its
/// issue and recognised by a predicate over the printed `.ink` source. A
/// story that fails the differential AND matches a predicate is counted,
/// reported at the end, and does not fail the run; a story that fails and
/// matches none is a new finding and fails with the shrunk source. This is
/// `metadata.toml`'s `expected_mismatch` (#3402) applied to generated
/// stories: the shapes keep running through brink, and the entry is removed
/// (the run then finds nothing to count) once the issue is fixed. #3507 and
/// #3508, the first run's two findings, are fixed; of the functions tier's
/// six (2026-09-04), #3519, #3522, #3523 and #3525 are fixed and the rest
/// are listed below.
///
/// The cost is stated plainly: a story that matches a predicate could fail
/// for a DIFFERENT reason and be counted here — so keep predicates narrow,
/// and read the per-run tally as a signal, not as noise.
const KNOWN_DIVERGENCES: &[(&str, SourcePredicate)] = &[
    // Content (text or an interpolation), then an inline conditional whose
    // condition calls a function: the lift evaluates the condition (and
    // the call's output) before the prefix is emitted.
    ("#3521", prefix_then_conditional_calling_a_function),
    // A function printing two or more lines: called in an interpolation,
    // its output is one fragment, and the newline inside it never becomes
    // a line boundary.
    ("#3524", function_printing_several_lines),
    // A whole-line inline conditional with no else arm whose condition
    // calls a function: the lift emits no end-of-line on the untaken side,
    // and the call's output loses its newline.
    ("#3530", else_less_conditional_calling_a_function),
    // Glue somewhere in the story and a line that can render empty (a
    // list interpolation): ink's glue reaches across the blank line to
    // join the lines either side of it, brink's stops at it.
    ("#3535", glue_and_a_possibly_empty_line),
    // A shuffle sequence: its seed is the sequence container's path hash,
    // and brink's container paths are not inklecate's, so the two pick
    // different permutations.
    ("#3538", uses_a_shuffle),
];

/// A `{~…}` shuffle anywhere in the story (#3538). Predates this tier:
/// `tests/tier2/conditional/shuffle` is 0/1 against the C# oracle and
/// `tests/tier2/sequences/I107-shuffle-stack-muddying` 0/2.
fn uses_a_shuffle(src: &str) -> bool {
    src.contains("{~")
}

/// A story that both uses glue and has a line made only of `{…}` groups
/// while a `LIST` exists to make those groups render empty — a list value
/// is the only thing the generator can print as nothing, and a whole line
/// of them is a blank line the glue has to reach across.
fn glue_and_a_possibly_empty_line(src: &str) -> bool {
    if !src.contains("<>") || !src.lines().any(|l| l.trim_start().starts_with("LIST ")) {
        return false;
    }
    src.lines().any(|line| is_all_interpolations(line.trim()))
}

/// Is `line` nothing but `{…}` groups (with an optional trailing `<>`)?
/// Groups nest, so the scan counts braces rather than looking for the
/// first `}` — `{false:a}{LIST_MAX(l)}` is two groups, and CI found that
/// exact shape when the predicate only admitted one.
fn is_all_interpolations(line: &str) -> bool {
    let mut rest = line.strip_suffix("<>").unwrap_or(line).trim_end();
    if rest.is_empty() {
        return false;
    }
    while !rest.is_empty() {
        if !rest.starts_with('{') {
            return false;
        }
        let mut depth = 0usize;
        let mut end = None;
        for (i, c) in rest.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else { return false };
        rest = rest[end..].trim_start();
    }
    true
}

/// A line that is exactly one `{cond:then}` inline conditional with no `|`
/// arm, whose condition names a generated function.
fn else_less_conditional_calling_a_function(src: &str) -> bool {
    src.lines().any(|line| {
        let t = line.trim();
        t.starts_with('{')
            && t.ends_with('}')
            && t.matches('{').count() == 1
            && t.split_once(':')
                .is_some_and(|(cond, rest)| names_a_function(cond) && !rest.contains('|'))
    })
}

/// A content line with content (text or an earlier `{…}`) before a
/// `{cond:…}` whose condition names a generated function (`f<n>_`).
fn prefix_then_conditional_calling_a_function(src: &str) -> bool {
    src.lines().any(|line| {
        let mut rest = line;
        let mut seen_content = false;
        while let Some(open) = rest.find('{') {
            seen_content |= !rest[..open].trim().is_empty();
            let inner = &rest[open + 1..];
            let Some(close) = inner.find('}') else { break };
            let body = &inner[..close];
            if seen_content
                && body
                    .split_once(':')
                    .is_some_and(|(cond, _)| names_a_function(cond))
            {
                return true;
            }
            seen_content = true;
            rest = &inner[close + 1..];
        }
        false
    })
}

/// `f<digit>` — the generator's function names are `f{i}_{base}`.
fn names_a_function(s: &str) -> bool {
    s.as_bytes()
        .windows(2)
        .any(|w| w[0] == b'f' && w[1].is_ascii_digit())
}

/// A `=== function` section that may print two or more lines: each
/// content line (anything that is not logic, a block marker, or blank)
/// counts one, and every reference to a generated function (`f<n>_…`,
/// whose callee may print a line of its own) counts one more, whether it
/// is a statement call (`~ f0_a()`, CI's first find), a call inside a
/// logic line (`~ return f0_a()`, CI's second — the callee's line lands
/// in the caller's), or an interpolation.
fn function_printing_several_lines(src: &str) -> bool {
    let mut in_function = false;
    let mut may_print = 0;
    for line in src.lines() {
        let t = line.trim();
        if t.starts_with("=== function") {
            in_function = true;
            may_print = 0;
            continue;
        }
        if t.starts_with("===") {
            in_function = false;
            continue;
        }
        let is_content = !t.is_empty()
            && !t.starts_with('~')
            && !t.starts_with("{ ")
            && !t.starts_with('-')
            && !t.starts_with('}');
        if in_function {
            may_print += usize::from(is_content) + function_references(t);
            if may_print >= 2 {
                return true;
            }
        }
    }
    false
}

/// How many times `s` names a generated function (`f` followed by a digit).
fn function_references(s: &str) -> usize {
    s.as_bytes()
        .windows(2)
        .filter(|w| w[0] == b'f' && w[1].is_ascii_digit())
        .count()
}

/// Recognises a known-divergent shape in a printed `.ink` source.
type SourcePredicate = fn(&str) -> bool;

static KNOWN_HITS: AtomicUsize = AtomicUsize::new(0);

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

fn scratch_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("brink-gen-inkjs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// The C# crawler's defaults, which `tools/inkjs-oracle` reproduces; brink's
/// explorer is given the same budget so a depth-capped `InputsExhausted`
/// lands at the same step on both sides.
const EXPLORE: ExploreConfig = ExploreConfig {
    max_depth: 20,
    max_episodes: 1000,
};

fn render(story: &brink_gen::Story) -> Result<(), TestCaseError> {
    let src = print_ink(story);
    // A panic anywhere in the comparison — the compiler, the explorer, the
    // oracle bridge — is reported with the story that caused it. proptest
    // catches the unwind either way, but its report carries only the
    // shrunken `Story` debug dump, and the `.ink` is what anyone
    // reproducing it actually needs.
    let compared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| compare(&src)));
    let compared = match compared {
        Ok(result) => result,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("<non-string panic payload>");
            return Err(TestCaseError::fail(format!(
                "panic while comparing: {msg}\n--- source ---\n{src}"
            )));
        }
    };
    match compared {
        Ok(()) => Ok(()),
        Err(e) => {
            let known: Vec<&str> = KNOWN_DIVERGENCES
                .iter()
                .filter(|(_, matches)| matches(&src))
                .map(|(issue, _)| *issue)
                .collect();
            if known.is_empty() {
                Err(e)
            } else {
                KNOWN_HITS.fetch_add(1, Ordering::Relaxed);
                eprintln!(
                    "inkjs_differential: known divergence ({}) on:\n{src}",
                    known.join("; ")
                );
                Ok(())
            }
        }
    }
}

fn compare(src: &str) -> Result<(), TestCaseError> {
    let scratch = scratch_dir();
    let ink_path = scratch.join("gen.ink");
    std::fs::write(&ink_path, src).expect("write generated story");

    let (_, mut brink_eps) =
        brink_test_harness::corpus::compile_and_explore_from_ink(&ink_path, &EXPLORE)
            .map_err(|e| TestCaseError::fail(format!("brink: {e}\n--- source ---\n{src}")))?;
    for ep in &mut brink_eps {
        inkjs::normalize_brink_episode(ep);
    }

    let mut inkjs_eps = inkjs::explore_file(&ink_path, &scratch.join("inkjs"))
        .map_err(|e| TestCaseError::fail(format!("inkjs: {e}\n--- source ---\n{src}")))?;
    for ep in &mut inkjs_eps {
        inkjs::normalize_oracle_episode(ep);
    }

    prop_assert_eq!(
        inkjs_eps.len(),
        brink_eps.len(),
        "episode counts differ (inkjs vs brink) on:\n{}",
        src
    );
    let brink_by_path: HashMap<&[usize], _> = brink_eps
        .iter()
        .map(|ep| (ep.choice_path.as_slice(), ep))
        .collect();
    for oracle_ep in &inkjs_eps {
        let Some(brink_ep) = brink_by_path.get(oracle_ep.choice_path.as_slice()) else {
            return Err(TestCaseError::fail(format!(
                "brink has no episode for choice path {:?} on:\n{src}",
                oracle_ep.choice_path
            )));
        };
        let diff = brink_test_harness::diff_oracle(oracle_ep, brink_ep);
        prop_assert!(
            diff.matches,
            "brink diverges from inkjs on choice path {:?}:\n{}\n--- source ---\n{}",
            oracle_ep.choice_path,
            diff,
            src
        );
    }
    Ok(())
}

#[test]
fn brink_matches_inkjs_on_generated_plain_ink() {
    if !inkjs::enabled() {
        eprintln!(
            "inkjs_differential: skipped — set {}=1 (needs node + `npm ci` in tools/inkjs-oracle)",
            inkjs::ENABLE_ENV
        );
        return;
    }
    inkjs::ensure_installed().unwrap_or_else(|e| panic!("{e}"));

    let mut runner = proptest::test_runner::TestRunner::new(config());
    let outcome = runner.run(&arb_story_with(Profile::PLAIN_INK), |story| render(&story));
    eprintln!(
        "inkjs_differential: {} known-divergence hit(s) (see KNOWN_DIVERGENCES)",
        KNOWN_HITS.load(Ordering::Relaxed)
    );
    if let Err(e) = outcome {
        panic!("{e}");
    }
}
