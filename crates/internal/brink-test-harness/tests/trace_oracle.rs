//! Sensitivity tests for the observable-equivalence oracle (issue #3376,
//! `docs/observable-semantics-spec.md` §2/§3).
//!
//! Each test isolates **one** observable from `docs/observable-semantics-spec.md`
//! §2 and proves the oracle sees it: two programs that agree on everything
//! else and differ only in that observable must produce a non-empty
//! [`trace_diff`]. Delete the corresponding capture from
//! `brink_test_harness::trace` and exactly the matching test goes red — that
//! is what makes this file a regression test for the definition rather than
//! for the implementation's shape.
//!
//! Every fixture is real `.ink` (and, for the surface-parity tests, real
//! `.brink`) source compiled through the actual pipeline, never a hand-built
//! `StoryData`.
#![expect(
    clippy::expect_used,
    reason = "test helpers outside a #[test] fn: fail loudly on a bad fixture, always \
              preceded by an assert! naming what went wrong"
)]

use brink_test_harness::corpus::{compile_entry_to_inkb, compile_source_to_inkb};
use brink_test_harness::trace::{
    DivergenceKind, ExternalStubs, FunctionProbe, LinkedProgram, RunSpec, Terminal, TraceConfig,
    TraceEvent, capture, explore_runs, line_identity_diff, trace_diff_with,
};

/// Compile one source file through the real pipeline, returning its
/// `StoryData` and `.inkb` bytes.
fn build(label: &str, file: &str, source: &str) -> (brink_format::StoryData, Vec<u8>) {
    let result = compile_source_to_inkb(label, file, source);
    assert!(result.is_ok(), "compile {label}/{file}: {result:?}");
    result.expect("just asserted the compile succeeded")
}

fn ink(label: &str, source: &str) -> (brink_format::StoryData, Vec<u8>) {
    build(label, "story.ink", source)
}

fn brink(label: &str, source: &str) -> (brink_format::StoryData, Vec<u8>) {
    build(label, "story.brink", source)
}

/// Compile `.brink` source that declares its own `@[convention]` handler
/// (issue #2289: an unconfigured handler is `E169`, not a silent pass), so
/// — unlike [`brink`] — this writes a co-located `brink.toml` naming
/// `story.brink` as its own conventions module, alongside `story.brink`
/// itself, in a scratch directory unique to `label`.
fn brink_with_own_conventions(label: &str, source: &str) -> (brink_format::StoryData, Vec<u8>) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "brink-trace-{label}-{}-{nonce}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&dir).ok();
    let created = std::fs::create_dir_all(&dir);
    assert!(
        created.is_ok(),
        "create scratch dir {}: {created:?}",
        dir.display()
    );
    let wrote_source = std::fs::write(dir.join("story.brink"), source);
    assert!(
        wrote_source.is_ok(),
        "write scratch source: {wrote_source:?}"
    );
    let wrote_toml = std::fs::write(
        dir.join("brink.toml"),
        "[project]\nconventions = \"story.brink\"\n",
    );
    assert!(
        wrote_toml.is_ok(),
        "write scratch brink.toml: {wrote_toml:?}"
    );
    let result = compile_entry_to_inkb(&dir.join("story.brink"));
    std::fs::remove_dir_all(&dir).ok();
    assert!(result.is_ok(), "compile {label}/story.brink: {result:?}");
    result.expect("just asserted the compile succeeded")
}

/// Explore `p`'s runs, then diff `p` against `q` over exactly those runs.
fn diff_over_p_runs(p: &[u8], q: &[u8], config: &TraceConfig) -> brink_test_harness::TraceDiff {
    let linked = LinkedProgram::from_inkb(p);
    assert!(linked.is_ok(), "link P: {linked:?}");
    let linked = linked.expect("just asserted P links");
    let runs = explore_runs(&linked, config);
    assert!(runs.is_ok(), "explore P: {runs:?}");
    let runs = runs.expect("just asserted exploration succeeded");
    assert!(!runs.is_empty(), "exploration produced no runs");
    let diff = trace_diff_with(p, q, &runs, config);
    assert!(diff.is_ok(), "trace_diff: {diff:?}");
    diff.expect("just asserted the diff succeeded")
}

fn assert_equivalent(label: &str, p: &[u8], q: &[u8], config: &TraceConfig) {
    let diff = diff_over_p_runs(p, q, config);
    assert!(
        diff.is_empty(),
        "{label}: expected no divergence, got {diff}"
    );
}

fn assert_divergent(label: &str, p: &[u8], q: &[u8], config: &TraceConfig) {
    let diff = diff_over_p_runs(p, q, config);
    assert!(
        !diff.is_empty(),
        "{label}: expected a divergence, oracle reported {diff}"
    );
}

// ── The self-check: compiling the same source twice is equivalent ───────────

#[test]
fn the_same_source_compiled_twice_is_observably_equivalent() {
    let source = "\
VAR gold = 3
Hello.
* [Take the gold] You take {gold} coins.
    -> END
* [Leave] You leave.
    -> END
";
    let (_, p) = ink("self-check-a", source);
    let (_, q) = ink("self-check-b", source);
    assert_equivalent("identical source", &p, &q, &TraceConfig::default());
}

// ── §2 item 1: output steps, in order ──────────────────────────────────────

#[test]
fn a_changed_line_of_text_is_detected() {
    let (_, p) = ink("text-a", "Hello.\n-> END\n");
    let (_, q) = ink("text-b", "Goodbye.\n-> END\n");
    assert_divergent("line text", &p, &q, &TraceConfig::default());
}

#[test]
fn a_changed_line_tag_is_detected() {
    let (_, p) = ink("tag-a", "Hello. #mood: calm\n-> END\n");
    let (_, q) = ink("tag-b", "Hello. #mood: tense\n-> END\n");
    assert_divergent("line tags", &p, &q, &TraceConfig::default());
}

#[test]
fn swapping_two_choices_is_detected_because_choices_compare_by_order() {
    // Both programs present the same *set* of choices with the same bodies;
    // only the presentation order differs. Spec §2.1 RULED: choices compare
    // by order, because hosts pick by index.
    let p_src = "\
Pick.
* [North] You go north.
    -> END
* [South] You go south.
    -> END
";
    let q_src = "\
Pick.
* [South] You go south.
    -> END
* [North] You go north.
    -> END
";
    let (_, p) = ink("choice-order-a", p_src);
    let (_, q) = ink("choice-order-b", q_src);
    assert_divergent("choice order", &p, &q, &TraceConfig::default());
}

#[test]
fn the_terminal_kind_is_part_of_the_trace() {
    // Identical output; one story ends, the other only yields.
    let (_, p) = ink("terminal-end", "Hello.\n-> END\n");
    let (_, q) = ink("terminal-done", "Hello.\n-> DONE\n");
    assert_divergent("terminal kind", &p, &q, &TraceConfig::default());
}

// ── §2 item 2: external calls, in order, with arguments ────────────────────

/// The external's fallback returns the same value for every argument, so the
/// two programs print byte-identical output and read identical globals. The
/// **only** difference is the argument the host saw — if external calls were
/// not in the trace, this would pass.
const EXTERNAL_ARG_TEMPLATE: &str = "\
EXTERNAL logEvent(code)

The value is {logEvent(ARG)}.
-> END

=== function logEvent(code) ===
~ return 0
";

#[test]
fn an_external_calls_arguments_are_captured() {
    let (_, p) = ink("external-arg-a", &EXTERNAL_ARG_TEMPLATE.replace("ARG", "3"));
    let (_, q) = ink("external-arg-b", &EXTERNAL_ARG_TEMPLATE.replace("ARG", "4"));
    assert_divergent("external args", &p, &q, &TraceConfig::default());
}

#[test]
fn the_external_call_sequence_is_recorded_with_its_name_and_arguments() {
    let (_, p) = ink(
        "external-record",
        &EXTERNAL_ARG_TEMPLATE.replace("ARG", "3"),
    );
    let linked = LinkedProgram::from_inkb(&p);
    assert!(linked.is_ok(), "link: {linked:?}");
    let linked = linked.expect("just asserted the program links");
    let config = TraceConfig::default();
    let trace = capture(&linked, &RunSpec::root(), &config);
    assert!(trace.is_ok(), "capture: {trace:?}");
    let trace = trace.expect("just asserted the capture succeeded");

    let externals: Vec<(String, Vec<brink_format::Value>)> = trace
        .events
        .iter()
        .filter_map(|e| match e {
            TraceEvent::External { name, args, .. } => Some((name.clone(), args.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        externals.len(),
        1,
        "expected exactly one external call in {:?}",
        trace.events
    );
    let (name, args) = externals
        .first()
        .expect("just asserted there is exactly one external call");
    assert_eq!(name, "logEvent");
    assert_eq!(args.len(), 1, "expected one argument, got {args:?}");
    assert_eq!(format!("{:?}", args[0]), "Int(3)", "argument value");
}

#[test]
fn a_stubbed_external_result_is_part_of_the_runs_definition() {
    // Same program, two different stub answers: the trace must differ,
    // because the stubbed results are part of what defines a run.
    let source = "\
EXTERNAL roll()

You rolled {roll()}.
-> END

=== function roll() ===
~ return 0
";
    let (_, p) = ink("external-stub", source);
    let linked = LinkedProgram::from_inkb(&p);
    assert!(linked.is_ok(), "link: {linked:?}");
    let linked = linked.expect("just asserted the program links");

    let fallback = TraceConfig::default();
    let mut fixed = TraceConfig::default();
    let mut stubs = std::collections::BTreeMap::new();
    stubs.insert("roll".to_string(), brink_format::Value::Int(6));
    fixed.externals = ExternalStubs::Fixed(stubs);

    let a = capture(&linked, &RunSpec::root(), &fallback);
    let b = capture(&linked, &RunSpec::root(), &fixed);
    assert!(a.is_ok() && b.is_ok(), "captures: {a:?} {b:?}");
    let a = a.expect("just asserted capture a succeeded");
    let b = b.expect("just asserted capture b succeeded");
    assert_ne!(
        a.events, b.events,
        "a different stubbed external result must change the trace"
    );
}

// ── §2 item 3: host-readable state at every turn boundary ──────────────────

#[test]
fn a_global_the_story_never_reads_is_still_observable() {
    // THE §2 item 3 test. Both programs print exactly the same text and
    // present no choices; they differ only in the value of a global nothing
    // in the story ever reads. RULED **in**: hosts read globals, so this is
    // observable.
    let (_, p) = ink("global-a", "VAR gold = 10\nHello.\n-> END\n");
    let (_, q) = ink("global-b", "VAR gold = 11\nHello.\n-> END\n");
    assert_divergent("unread global", &p, &q, &TraceConfig::default());
}

#[test]
fn host_readable_globals_are_captured_at_every_turn_boundary() {
    let source = "\
VAR gold = 1
Start.
* [Earn]
    ~ gold = 5
    You now have {gold}.
    -> END
";
    let (_, p) = ink("globals-boundary", source);
    let linked = LinkedProgram::from_inkb(&p);
    assert!(linked.is_ok(), "link: {linked:?}");
    let linked = linked.expect("just asserted the program links");
    let run = RunSpec::root().then(0);
    let trace = capture(&linked, &run, &TraceConfig::default());
    assert!(trace.is_ok(), "capture: {trace:?}");
    let trace = trace.expect("just asserted the capture succeeded");

    let boundaries: Vec<&Vec<(String, brink_format::Value)>> = trace
        .events
        .iter()
        .filter_map(|e| match e {
            TraceEvent::Globals(g) => Some(g),
            _ => None,
        })
        .collect();
    assert!(
        boundaries.len() >= 3,
        "expected a globals capture at the start, at the choice, and at the \
         terminal — got {} in {:?}",
        boundaries.len(),
        trace.events
    );
    let first = boundaries.first().map(|g| format!("{g:?}"));
    let last = boundaries.last().map(|g| format!("{g:?}"));
    assert_ne!(
        first, last,
        "the run writes `gold`, so the first and last boundary must differ"
    );
}

// ── §2.1: RNG draw order is protected by construction ──────────────────────

#[test]
fn removing_an_unused_random_draw_is_detected() {
    // Spec §2.1: "A dead `RANDOM(1, 6)` whose result is unused is *not*
    // removable." The draw lands in a temp — an internal the trace never
    // captures — so the only way to see it is through the *later* draw it
    // shifts. Explored under several seeds, because equivalence must hold
    // under every seed.
    let p_src = "\
~ temp junk = RANDOM(1, 6)
You rolled {RANDOM(1, 100)}.
-> END
";
    let q_src = "\
You rolled {RANDOM(1, 100)}.
-> END
";
    let (_, p) = ink("random-draw-a", p_src);
    let (_, q) = ink("random-draw-b", q_src);
    let config = TraceConfig {
        seeds: vec![1, 2, 3, 4, 5, 6, 7, 8],
        ..TraceConfig::default()
    };
    assert_divergent("dead RANDOM draw", &p, &q, &config);
}

// ── §2 item 4: host-invoked function results ───────────────────────────────

#[test]
fn a_host_invoked_functions_result_is_observable() {
    // Identical output, identical globals; only what a host `callFunction`
    // gets back differs.
    let template = "\
Hello.
-> END

=== function score() ===
~ return RESULT
";
    let (_, p) = ink("probe-a", &template.replace("RESULT", "7"));
    let (_, q) = ink("probe-b", &template.replace("RESULT", "8"));
    let config = TraceConfig {
        probes: vec![FunctionProbe {
            name: "score".to_string(),
            args: Vec::new(),
        }],
        ..TraceConfig::default()
    };
    assert_divergent("host-invoked function result", &p, &q, &config);
}

// ── §6: a fault is observable; its message is not ──────────────────────────

#[test]
fn a_fault_on_one_side_only_is_detected() {
    // Neither program prints anything before terminating, so the *only*
    // difference in the trace is that one faulted and the other did not.
    // Spec §6: "a run that faults faults in both or neither".
    let (_, p) = ink("fault-a", "~ temp z = 0\n~ temp y = 5 / z\n-> END\n");
    let (_, q) = ink("fault-b", "~ temp z = 0\n~ temp y = 5\n-> END\n");
    assert_divergent("fault vs no fault", &p, &q, &TraceConfig::default());
}

#[test]
fn a_one_sided_fault_is_named_fault_asymmetry_not_a_bare_terminal_diff() {
    // Regression: `DivergenceKind::FaultAsymmetry` was declared and
    // rendered by `Display` but never constructed — `first_divergence`
    // reported a one-sided fault as a bare `Differs` on two `Terminal`
    // events instead of naming the asymmetry.
    let (_, p) = ink("fault-kind-a", "~ temp z = 0\n~ temp y = 5 / z\n-> END\n");
    let (_, q) = ink("fault-kind-b", "~ temp z = 0\n~ temp y = 5\n-> END\n");
    let diff = diff_over_p_runs(&p, &q, &TraceConfig::default());
    assert!(
        !diff.is_empty(),
        "expected a divergence, oracle reported {diff}"
    );
    let first = diff.first().expect("just asserted the diff is non-empty");
    assert!(
        matches!(first.kind, DivergenceKind::FaultAsymmetry { .. }),
        "expected FaultAsymmetry, got {:?}",
        first.kind
    );
}

#[test]
fn an_external_call_made_before_a_fault_on_the_same_step_is_still_captured() {
    // Regression: `capture`'s fault branch used to return before draining
    // the handler, so an external call made earlier in the *same step* as
    // a fault was silently discarded. Because fault text is not compared
    // (spec §6), two programs that call the external with different
    // arguments before an otherwise-identical fault were reported
    // observably equivalent — a blind spot in the one harness whose
    // purpose is sensitivity. Both programs here fault at the same
    // division by zero; the only difference is the argument the external
    // saw beforehand.
    let template = "\
EXTERNAL logEvent(code)

~ logEvent(ARG)
~ temp z = 0
~ temp y = 5 / z
-> END

=== function logEvent(code) ===
~ return 0
";
    let (_, p) = ink("fault-external-a", &template.replace("ARG", "3"));
    let (_, q) = ink("fault-external-b", &template.replace("ARG", "4"));
    assert_divergent(
        "external call before fault",
        &p,
        &q,
        &TraceConfig::default(),
    );
}

#[test]
fn a_safe_exit_is_distinguished_from_running_out_of_content() {
    // Same printed text, both ending in `Step::Done`. One reached an
    // explicit `-> DONE`; the other ran dry, so the host's *next*
    // `continue_single` would error. `Story::did_safe_exit` is how a host
    // tells them apart, so the terminal kind carries it.
    let (_, p) = ink(
        "safe-exit-a",
        "-> start\n\n=== start ===\nHello.\n-> DONE\n",
    );
    let (_, q) = ink("safe-exit-b", "-> start\n\n=== start ===\nHello.\n");
    assert_divergent(
        "safe exit vs ran-out-of-content",
        &p,
        &q,
        &TraceConfig::default(),
    );
}

// ── §2.2: translation identity ─────────────────────────────────────────────

#[test]
fn translation_identity_is_unchanged_for_an_identical_recompile() {
    let source = "Hello.\nThere.\n-> END\n";
    let (p, _) = ink("identity-a", source);
    let (q, _) = ink("identity-b", source);
    let diff = line_identity_diff(&p, &q);
    assert!(diff.is_empty(), "identical source: {diff}");
}

#[test]
fn translation_identity_reports_a_changed_line_hash() {
    let (p, _) = ink("identity-changed-a", "Hello.\nThere.\n-> END\n");
    let (q, _) = ink("identity-changed-b", "Hello.\nThere, friend.\n-> END\n");
    let diff = line_identity_diff(&p, &q);
    assert!(
        !diff.is_empty(),
        "editing a line must move its identity, got {diff}"
    );
}

#[test]
fn translation_identity_is_a_separate_result_from_the_trace_diff() {
    // A tag-only edit changes the runtime trace. Whether it also moves line
    // identity is the *second* obligation (§2.2) — the point of this test is
    // that the two checks are asked separately, so a caller can act on one
    // without the other having veto power.
    let (p_data, p) = ink("identity-separate-a", "Hello.\n-> END\n");
    let (q_data, q) = ink("identity-separate-b", "Hello. #beat\n-> END\n");
    assert_divergent("tagged line", &p, &q, &TraceConfig::default());
    let identity = line_identity_diff(&p_data, &q_data);
    // Reported either way; the assertion is that asking is possible and the
    // result is well formed, not which way it lands.
    assert_eq!(
        identity.is_empty(),
        identity.changes.is_empty(),
        "LineIdentityDiff::is_empty must agree with its own change list"
    );
}

// ── The native (`.brink`) surface ──────────────────────────────────────────

#[test]
fn the_native_surface_compiles_and_traces_identically() {
    let source = "\
flow main() {
  Hello from native.
  -> END
}
";
    let (_, p) = brink("native-self-a", source);
    let (_, q) = brink("native-self-b", source);
    assert_equivalent("identical .brink source", &p, &q, &TraceConfig::default());
}

#[test]
fn a_native_surface_public_global_the_story_never_reads_is_still_observable() {
    // `pub`, because the native surface is always a *declared* module and
    // therefore defaults private (`docs/modules-spec.md` §"Defaults"). A
    // public global is host-readable, so §2 item 3 covers it.
    let template = "\
pub var gold = VALUE

flow main() {
  Hello from native.
  -> END
}
";
    let (_, p) = brink("native-global-a", &template.replace("VALUE", "10"));
    let (_, q) = brink("native-global-b", &template.replace("VALUE", "11"));
    assert_divergent("native unread global", &p, &q, &TraceConfig::default());
}

#[test]
fn a_module_private_native_global_is_not_host_readable_and_so_is_not_in_the_trace() {
    // The other half of the rule above, pinned rather than left to chance:
    // a native `var` with no `pub` is module-private, the host is outside
    // every module, so it is *not* host-readable state and §2 item 3 does
    // not cover it. Two programs differing only there are reported
    // equivalent — correct under the definition, and exactly the escape
    // hatch §2.3 contemplates, already present in fact on this surface.
    let template = "\
var secret = VALUE

flow main() {
  Hello from native.
  -> END
}
";
    let (_, p) = brink("native-private-a", &template.replace("VALUE", "10"));
    let (_, q) = brink("native-private-b", &template.replace("VALUE", "11"));
    assert_equivalent(
        "native module-private global",
        &p,
        &q,
        &TraceConfig::default(),
    );
}

#[test]
fn a_native_surface_text_change_is_detected() {
    let template = "\
flow main() {
  MESSAGE
  -> END
}
";
    let (_, p) = brink("native-text-a", &template.replace("MESSAGE", "Hello."));
    let (_, q) = brink("native-text-b", &template.replace("MESSAGE", "Goodbye."));
    assert_divergent("native line text", &p, &q, &TraceConfig::default());
}

#[test]
fn element_data_from_an_attaching_convention_is_detected() {
    // §2 item 1 names `element.data` explicitly, but until this test
    // nothing constructed a program whose `element.data` differs — the
    // corpus self-check is a self-equivalence sweep, so it stayed green
    // even with the capture dropped. Modeled on
    // `tests/tier1-native/conventions-attach-schema/story.brink`: the two
    // programs are identical except for the `attach = Cue` handler's own
    // returned `voiceover` field. The handler's claimed `VENDOR` line is
    // consumed by the attaching convention (ruling item 2/6,
    // `docs/decision-log.md` "The element output model"), so the visible
    // transcript is byte-identical and only `element.data` differs.
    let template = "\
struct Cue {
  speaker: string,
  voiceover: bool,
}

@[convention(claims = \"^(?<name>[A-Z][A-Z '-]*)$\", attach = Cue, order = 10)]
fn cue(name: string): Cue {
  return Cue { speaker: name, voiceover: VALUE };
}

flow main() {
  VENDOR
  You shouldn't be here after dark.
  -> END
}
";
    let (_, p) = brink_with_own_conventions("element-data-a", &template.replace("VALUE", "false"));
    let (_, q) = brink_with_own_conventions("element-data-b", &template.replace("VALUE", "true"));
    assert_divergent(
        "element.data from attach handler",
        &p,
        &q,
        &TraceConfig::default(),
    );
}

// ── Run-set exploration ────────────────────────────────────────────────────

#[test]
fn exploration_enumerates_every_branch_up_to_the_depth_bound() {
    let source = "\
Start.
* [A] -> mid
* [B] -> mid

=== mid ===
Middle.
* [C] -> END
* [D] -> END
";
    let (_, p) = ink("explore", source);
    let linked = LinkedProgram::from_inkb(&p);
    assert!(linked.is_ok(), "link: {linked:?}");
    let linked = linked.expect("just asserted the program links");
    let runs = explore_runs(&linked, &TraceConfig::default());
    assert!(runs.is_ok(), "explore: {runs:?}");
    let runs = runs.expect("just asserted exploration succeeded");
    let mut paths: Vec<Vec<usize>> = runs.iter().map(|r| r.choices.clone()).collect();
    paths.sort();
    assert_eq!(
        paths,
        vec![vec![0, 0], vec![0, 1], vec![1, 0], vec![1, 1]],
        "expected all four root-to-leaf choice paths"
    );
}

#[test]
fn replaying_a_run_a_program_cannot_offer_is_itself_a_divergence() {
    // A run recorded against a two-choice program, replayed on a one-choice
    // program: the shape mismatch has to surface, not silently truncate.
    let (_, one) = ink("one-choice", "Pick.\n* [A] -> END\n");
    let linked = LinkedProgram::from_inkb(&one);
    assert!(linked.is_ok(), "link: {linked:?}");
    let linked = linked.expect("just asserted the program links");
    let run = RunSpec::root().then(1);
    let trace = capture(&linked, &run, &TraceConfig::default());
    assert!(trace.is_ok(), "capture: {trace:?}");
    let trace = trace.expect("just asserted the capture succeeded");
    assert!(
        trace.events.iter().any(|e| matches!(
            e,
            TraceEvent::Terminal(Terminal::ChoiceOutOfRange {
                wanted: 1,
                offered: 1
            })
        )),
        "expected a ChoiceOutOfRange terminal, got {:?}",
        trace.events
    );
}
