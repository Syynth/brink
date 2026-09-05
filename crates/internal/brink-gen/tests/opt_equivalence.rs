//! The optimizer's generator property (`docs/optimizer-spec.md` §5.2).
//!
//! `trace(opt(compile(P))) == trace(compile(P))` over generated stories
//! explores shapes nobody wrote. The corpus fence
//! (`brink-test-harness/tests/opt_corpus_fence.rs`) covers the shapes we
//! happen to have; this covers the ones we do not.
//!
//! It mirrors `equivalence.rs` — same hand-read `PROPTEST_CASES`, same
//! `max_shrink_time` cap, same `trace_config()` bounds — and judges through the
//! same `opt_fence::judge()` seam everything else does.
//!
//! # No known-divergence list, deliberately
//!
//! `equivalence.rs` carries `RESPELL_KNOWN_DIVERGENCES` because the respeller
//! may legitimately refuse a story. The optimizer has no such escape: it either
//! preserves observable behaviour on every story or it is wrong. **No
//! allowance list should ever be added here.** If a generated story fails, the
//! optimizer is broken or the fence is — never the story.
//!
//! # Non-vacuity
//!
//! With an empty pass list the property is nearly free, so the second test
//! matters more than the first: it drives a *negative control* through the same
//! generator and requires it to be caught. That is what proves the generator is
//! emitting stories the oracle can actually tell apart, rather than 48 empty
//! ones — the failure `equivalence.rs` guards with its own
//! `equal >= cases / 10` floor.

use std::sync::atomic::{AtomicUsize, Ordering};

use brink_gen::{Profile, arb_story_with, print_ink};
use brink_opt::{OptConfig, control};
use brink_test_harness::corpus::compile_source_to_inkb;
use brink_test_harness::opt_fence::{is_line_text_grounded, judge};
use brink_test_harness::trace::TraceConfig;
use proptest::prelude::*;

const CASES: u32 = 48;

/// `ProptestConfig::with_cases` ignores `PROPTEST_CASES` (only `default()`
/// reads it), so the override is applied by hand — the `equivalence.rs` idiom.
fn config() -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(CASES);
    ProptestConfig {
        cases,
        max_shrink_time: 60_000,
        ..ProptestConfig::default()
    }
}

/// Deep enough to walk a generated story's whole choice tree, capped so a
/// pathological shape cannot run away.
fn trace_config() -> TraceConfig {
    TraceConfig {
        max_depth: 24,
        max_runs: 256,
        ..TraceConfig::default()
    }
}

fn fail(msg: String) -> TestCaseError {
    TestCaseError::fail(msg)
}

/// Stories with real content, counted so the property cannot pass vacuously on
/// a generator that produced nothing worth optimizing.
static NONTRIVIAL: AtomicUsize = AtomicUsize::new(0);

/// Generated stories the negative control was actually caught on.
static CONTROL_KILLS: AtomicUsize = AtomicUsize::new(0);
/// Cases where the resident pass list reported a change — the "metric
/// moved" half of the spec's standard pair (`docs/optimizer-spec.md` §5.2):
/// a pass that never fires on generated stories is not being tested.
static FUSED: AtomicUsize = AtomicUsize::new(0);

/// Every obligation must hold on a generated story.
fn opt_preserves_everything(story: &brink_gen::Story) -> Result<(), TestCaseError> {
    let src = print_ink(story);
    let (pre_data, pre) = compile_source_to_inkb("opt-gen", "story.ink", &src)
        .map_err(|e| fail(format!("compile failed: {e}\n--- source ---\n{src}")))?;

    let v = judge(&pre_data, &pre, &OptConfig::defaults(), &trace_config())
        .map_err(|e| fail(format!("fence error: {e}\n--- source ---\n{src}")))?;

    prop_assert!(
        v.trace_clean,
        "opt(P) diverges from P:\n{}\n--- source ---\n{src}",
        v.detail
    );
    prop_assert!(
        v.identity_clean,
        "opt(P) changes line identity:\n{}\n--- source ---\n{src}",
        v.detail
    );
    prop_assert!(v.idempotent, "opt(opt(P)) != opt(P)\n--- source ---\n{src}");
    prop_assert!(
        v.stable,
        "two optimizer runs over P produced different bytes\n--- source ---\n{src}"
    );
    prop_assert!(
        v.changed != v.bytes_identical,
        "a pass that reports a change must move bytes, and one that reports \
         none must be byte-identical (the latter is a brink-format round-trip \
         failure, not an optimizer one)\n--- source ---\n{src}"
    );

    if v.before.line_entries > 0 {
        NONTRIVIAL.fetch_add(1, Ordering::Relaxed);
    }
    if v.changed {
        FUSED.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

/// The same generator, but running a control that must be caught.
///
/// Grounding is decided **before** the verdict, never from it: deciding
/// "ungrounded" because the trace came back clean would make the assertion
/// below unreachable, which is precisely the vacuity this test exists to rule
/// out.
fn control_is_caught(story: &brink_gen::Story) -> Result<(), TestCaseError> {
    let src = print_ink(story);
    let Ok((pre_data, pre)) = compile_source_to_inkb("opt-gen-control", "story.ink", &src) else {
        return Ok(());
    };
    if !is_line_text_grounded(&pre_data, &pre, &trace_config()).unwrap_or(false) {
        return Ok(());
    }

    let v = judge(
        &pre_data,
        &pre,
        &control::config("control:retext"),
        &trace_config(),
    )
    .map_err(|e| fail(format!("fence error: {e}\n--- source ---\n{src}")))?;

    prop_assert!(
        !v.trace_clean,
        "the retext control survived a story whose runs render line-table \
         text:\n--- source ---\n{src}"
    );
    CONTROL_KILLS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// The property, plus its non-vacuity floor.
///
/// A hand-rolled runner rather than `proptest!` (the `inkjs_differential.rs`
/// idiom) so the floor can be asserted after the run without executing the
/// property twice.
///
/// The floor is stricter than `equivalence.rs`'s `equal >= cases / 10`, because
/// nothing here is allowed to be refused: the optimizer either preserves every
/// story or it is wrong.
#[test]
fn opt_preserves_trace_and_identity() {
    let cases = usize::try_from(config().cases).unwrap_or(usize::MAX);
    let mut runner = proptest::test_runner::TestRunner::new(config());
    NONTRIVIAL.store(0, Ordering::Relaxed);
    FUSED.store(0, Ordering::Relaxed);
    runner
        .run(&arb_story_with(Profile::PLAIN_INK), |story| {
            opt_preserves_everything(&story)
        })
        .expect("the optimizer property must hold");
    let nontrivial = NONTRIVIAL.load(Ordering::Relaxed);
    assert!(
        nontrivial >= cases / 2,
        "only {nontrivial} of {cases} generated stories carried any line entries — \
         the property is passing on empty artifacts"
    );
    // The standard pair's second half: the passes must actually fire on the
    // shapes the generator emits, or the trace-equality above is vacuous.
    let fused = FUSED.load(Ordering::Relaxed);
    assert!(
        fused >= cases / 2,
        "the resident passes changed only {fused} of {cases} generated stories — \
         a pass that never fires is not being tested"
    );
}

/// The negative control, over the generator.
///
/// Proves the generator emits stories the oracle can tell apart. Without this,
/// a generator that degenerated to trivial stories would keep
/// `opt_preserves_trace_and_identity` green forever.
#[test]
fn the_generator_produces_stories_the_oracle_can_distinguish() {
    let cases = usize::try_from(config().cases).unwrap_or(usize::MAX);
    let mut runner = proptest::test_runner::TestRunner::new(config());
    CONTROL_KILLS.store(0, Ordering::Relaxed);
    runner
        .run(&arb_story_with(Profile::PLAIN_INK), |story| {
            control_is_caught(&story)
        })
        .expect("the control must never survive a story it perturbed");
    let kills = CONTROL_KILLS.load(Ordering::Relaxed);
    assert!(
        kills >= cases / 4,
        "the retext control was only caught on {kills} of {cases} generated \
         stories — the generator is not producing distinguishable output"
    );
}
