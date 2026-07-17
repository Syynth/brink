//! Tests for batch-mode advancement (BH-2, #914) — most importantly THE
//! GATE: the order-invariance property (a batch of N flows produces identical
//! outcomes under any within-batch step permutation).

use super::*;

use std::collections::HashMap;

use bevy_app::{App, Update};
use bevy_asset::AssetPlugin;
use bevy_ecs::event::Event;
use bevy_ecs::observer::On;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{ResMut, RunSystemOnce as _};
use brink_runtime::Line;

use super::parallel::advance_batch_parallel;
use crate::test_support::{add_story_assets, compile_test_story};
use crate::{
    BrinkChoicesPresented, BrinkFlowRequest, BrinkLineDelivered, BrinkStoryEnded, BrinkTurnDone,
};

/// Compile a story and return `(program, tables, frame_start_world)`.
fn fixture(source: &str) -> (Program, Vec<Vec<LineEntry>>, World) {
    compile_test_story(source)
}

/// Read every declared global out of a world into a comparable vector — the
/// full World-scoped state, for asserting two batch runs converged
/// identically.
fn dump_globals(program: &Program, world: &World) -> Vec<Value> {
    (0..program.global_count())
        .map(|idx| world.global(idx).clone())
        .collect()
}

/// The batch core, exposed for step-order permutation: create `n` fresh flows
/// (flow-ids `0..n`), step them **in `step_order`** against `frame_start`
/// (buffering writes), then apply every flow's buffer to a fresh clone of
/// `frame_start` **in flow-id order**. Returns the converged world plus each
/// flow's rendered lines (indexed by flow-id).
///
/// This is exactly what [`advance_batch`] does, minus the ECS plumbing — the
/// Step loop is permutable so a test can prove permuting it changes nothing.
fn run_batch_permuted(
    program: &Program,
    tables: &[Vec<LineEntry>],
    frame_start: &World,
    n: usize,
    step_order: &[usize],
) -> (World, Vec<String>) {
    let mut flows: Vec<FlowInstance> = (0..n)
        .map(|_| FlowInstance::new_at_root(program).0)
        .collect();
    let mut bufs: Vec<WriteBuffer> = (0..n).map(|_| WriteBuffer::default()).collect();
    let mut rendered = vec![String::new(); n];

    // Step — in the given (possibly permuted) order.
    for &i in step_order {
        let (lines, _awaiting, _error) = step_flow(
            frame_start,
            &mut flows[i],
            program,
            tables,
            &FallbackHandler,
            &mut bufs[i],
        );
        rendered[i] = lines.iter().map(Line::text).collect();
    }

    // Apply — always in flow-id order.
    let mut world = frame_start.clone();
    for buf in &bufs {
        buf.apply_to(&mut world);
    }
    (world, rendered)
}

/// Frame-start read pinning: stepping flow A (which writes `g`) leaves
/// `frame_start` untouched, so flow B stepped against the same `frame_start`
/// still reads the frame-start value of `g` — never A's buffered write.
#[test]
fn reads_pin_to_frame_start_not_a_peers_buffered_write() {
    let (program, tables, frame_start) = fixture("VAR g = 0\nVal {g}.\n~ g = g + 1\n-> END\n");

    let mut flow_a = FlowInstance::new_at_root(&program).0;
    let mut buf_a = WriteBuffer::default();
    let (lines_a, _, _) = step_flow(
        &frame_start,
        &mut flow_a,
        &program,
        &tables,
        &FallbackHandler,
        &mut buf_a,
    );
    let text_a: String = lines_a.iter().map(Line::text).collect();
    assert!(
        text_a.contains("Val 0."),
        "A reads frame-start g=0: {text_a:?}"
    );
    // A buffered a write to g (its snapshot went 0 -> 1) but did NOT mutate
    // the shared frame-start world.
    assert!(buf_a.len() >= 1, "A should have buffered a write to g");
    assert_eq!(
        frame_start.global(program.global_index("g").expect("g exists")),
        &Value::Int(0),
        "frame-start world must be untouched by A's buffered write"
    );

    // B, stepped against the same untouched frame-start, still reads g=0.
    let mut flow_b = FlowInstance::new_at_root(&program).0;
    let mut buf_b = WriteBuffer::default();
    let (lines_b, _, _) = step_flow(
        &frame_start,
        &mut flow_b,
        &program,
        &tables,
        &FallbackHandler,
        &mut buf_b,
    );
    let text_b: String = lines_b.iter().map(Line::text).collect();
    assert!(
        text_b.contains("Val 0."),
        "B must read frame-start g=0, not A's write: {text_b:?}"
    );
}

/// THE GATE (§12.4): a batch of N flows produces identical outcomes under any
/// within-batch step permutation. Identical flows each read the frame-start
/// value of `g` and write `g + 1`; under frame-start semantics every flow
/// reads `0` (never a peer's same-frame write) and the converged world is
/// `g = 1` — regardless of the order flows are stepped. A naive immediate-
/// visibility driver would make later-stepped flows read `1`, `2`, … and
/// diverge across permutations; batch mode does not.
#[test]
fn order_invariance_identical_flows() {
    let (program, tables, frame_start) = fixture("VAR g = 0\nVal {g}.\n~ g = g + 1\n-> END\n");
    let n = 5;

    let permutations: &[&[usize]] = &[
        &[0, 1, 2, 3, 4],
        &[4, 3, 2, 1, 0],
        &[2, 0, 4, 1, 3],
        &[1, 3, 0, 4, 2],
        &[3, 2, 1, 4, 0],
    ];

    let mut baseline: Option<(Vec<Value>, Vec<String>)> = None;
    for perm in permutations {
        let (world, rendered) = run_batch_permuted(&program, &tables, &frame_start, n, perm);
        let globals = dump_globals(&program, &world);

        // Every flow read frame-start g=0 — no flow saw a peer's write.
        for (i, text) in rendered.iter().enumerate() {
            assert!(
                text.contains("Val 0."),
                "flow {i} under perm {perm:?} must read frame-start g=0: {text:?}"
            );
        }
        // Converged world: last-writer-wins over identical writes → g = 1.
        assert_eq!(
            world.global(program.global_index("g").expect("g exists")),
            &Value::Int(1),
            "converged g must be 1 under perm {perm:?}"
        );

        match &baseline {
            None => baseline = Some((globals, rendered)),
            Some((base_globals, base_rendered)) => {
                assert_eq!(
                    &globals, base_globals,
                    "converged world differs under step permutation {perm:?}"
                );
                assert_eq!(
                    &rendered, base_rendered,
                    "per-flow outcomes differ under step permutation {perm:?}"
                );
            }
        }
    }
}

/// Write-write conflicts resolve deterministically by **flow-id apply order**
/// (§12.4), independent of step order: two flows write the same cell distinct
/// values; the higher flow-id's write wins for every step permutation.
#[test]
fn write_write_resolves_by_flow_id_not_step_order() {
    // Two knots writing g to distinct constants; flow 0 starts in `low`
    // (writes 10), flow 1 in `high` (writes 20). We drive them from root but
    // parameterize the write via the flow index by compiling two variants.
    let low = fixture("VAR g = 0\n~ g = 10\n-> END\n");
    let high = fixture("VAR g = 0\n~ g = 20\n-> END\n");

    // frame_start shared layout: both declare only `g`, so global index 0 == g
    // in each program. Use `low`'s world as the shared frame-start.
    let g_idx = low.0.global_index("g").expect("g exists");
    assert_eq!(g_idx, high.0.global_index("g").expect("g exists"));

    let run = |step_first_high: bool| -> Value {
        let frame_start = low.2.clone();
        let mut flow_low = FlowInstance::new_at_root(&low.0).0;
        let mut flow_high = FlowInstance::new_at_root(&high.0).0;
        let mut buf_low = WriteBuffer::default();
        let mut buf_high = WriteBuffer::default();

        // Step in the requested order (order must not affect the result).
        if step_first_high {
            step_flow(
                &frame_start,
                &mut flow_high,
                &high.0,
                &high.1,
                &FallbackHandler,
                &mut buf_high,
            );
            step_flow(
                &frame_start,
                &mut flow_low,
                &low.0,
                &low.1,
                &FallbackHandler,
                &mut buf_low,
            );
        } else {
            step_flow(
                &frame_start,
                &mut flow_low,
                &low.0,
                &low.1,
                &FallbackHandler,
                &mut buf_low,
            );
            step_flow(
                &frame_start,
                &mut flow_high,
                &high.0,
                &high.1,
                &FallbackHandler,
                &mut buf_high,
            );
        }

        // Apply in flow-id order: flow 0 (low) then flow 1 (high).
        let mut world = frame_start.clone();
        buf_low.apply_to(&mut world);
        buf_high.apply_to(&mut world);
        world.global(g_idx).clone()
    };

    // Higher flow-id (high, applied last) wins regardless of step order.
    assert_eq!(run(false), Value::Int(20));
    assert_eq!(run(true), Value::Int(20));
}

/// The `WriteBuffer` observer records the flow's world mutations as an ordered
/// changeset, and replaying it converges the shared world.
#[test]
fn write_buffer_captures_and_replays_changeset() {
    let (program, tables, frame_start) = fixture("VAR g = 0\n~ g = 7\n-> END\n");
    let mut flow = FlowInstance::new_at_root(&program).0;
    let mut buf = WriteBuffer::default();
    step_flow(
        &frame_start,
        &mut flow,
        &program,
        &tables,
        &FallbackHandler,
        &mut buf,
    );
    assert!(buf.len() >= 1);

    let mut world = frame_start.clone();
    buf.apply_to(&mut world);
    assert_eq!(
        world.global(program.global_index("g").expect("g")),
        &Value::Int(7)
    );
}

// ── System-level reachability ───────────────────────────────────────────────

/// Marker for the reachability test's flows.
#[derive(Default)]
struct Batched;

/// `advance_batch::<M>` wired as a real Bevy `Update` system (the exact host
/// opt-in path from the docs) advances every pending flow and applies its
/// buffered writes to the shared world — proving the entry point is reachable,
/// not just unit-covered. Also asserts the `BrinkBatchReport` is populated.
#[test]
fn advance_batch_system_drives_and_applies() {
    let mut app = App::new();
    app.add_plugins(AssetPlugin::default());
    app.add_plugins(crate::BrinkPlugin::<Batched>::default());
    app.add_systems(Update, advance_batch::<Batched>);

    // A story that writes a global on its single turn, then ends.
    let (program, tables, ctx) = compile_test_story("VAR g = 0\nHi.\n~ g = 3\n-> END\n");
    let story = add_story_assets(&mut app, program, tables, ctx);

    // Spawn two flows sharing the story/world.
    for _ in 0..2 {
        app.world_mut().spawn(
            BrinkFlowRequest::<Batched>::builder()
                .story(story.clone())
                .build(),
        );
    }

    // Tick 1: fulfillment creates the flows + shared BrinkGlobals.
    app.update();
    // Tick 2: advance_batch runs the batch turn.
    app.update();

    let report = app.world().resource::<BrinkBatchReport<Batched>>();
    assert_eq!(report.stepped, 2, "both flows stepped to terminal");
    assert_eq!(report.awaiting, 0);
    assert!(
        report.writes_applied >= 2,
        "each flow buffered a write to g; got {}",
        report.writes_applied
    );

    // The shared world converged: g = 3 (both flows wrote 3, last-wins).
    let globals = app.world().resource::<BrinkGlobals<Batched>>();
    // g is the only global → index 0.
    assert_eq!(globals.inner.global(0), &Value::Int(3));
}

/// `step_flow` surfaces a `RuntimeError` (rather than silently discarding it)
/// when a flow's Step faults — e.g. a knot that reprints and re-diverts into
/// itself forever trips `FlowInstance::LINE_LIMIT` before any terminal line.
#[test]
fn step_flow_returns_the_runtime_error_on_fault() {
    let (program, tables, frame_start) = fixture("-> spam\n\n=== spam ===\nLine.\n-> spam\n");
    let mut flow = FlowInstance::new_at_root(&program).0;
    let mut buf = WriteBuffer::default();
    let (lines, awaiting, error) = step_flow(
        &frame_start,
        &mut flow,
        &program,
        &tables,
        &FallbackHandler,
        &mut buf,
    );
    assert!(lines.is_empty(), "a faulted step produces no lines");
    assert!(!awaiting, "a faulted step is not a deferred-external park");
    match error {
        Some(brink_runtime::RuntimeError::LineLimitExceeded(n)) => {
            assert_eq!(n, FlowInstance::LINE_LIMIT);
        }
        other => panic!("expected Some(LineLimitExceeded), got {other:?}"),
    }
}

/// A flow whose Step faults (line-limit exceeded) must be counted in
/// `BrinkBatchReport::errored`, not silently folded into `stepped` — the
/// regression this test guards: an ignored `RuntimeError` used to make an
/// errored flow indistinguishable from one that reached a real terminal line.
#[test]
fn errored_flow_is_reported_distinctly_not_counted_as_stepped() {
    let mut app = App::new();
    app.add_plugins(AssetPlugin::default());
    app.add_plugins(crate::BrinkPlugin::<Batched>::default());
    app.add_systems(Update, advance_batch::<Batched>);

    // A knot that reprints and re-diverts into itself forever: never reaches
    // a terminal line, so Step must fault with `LineLimitExceeded`.
    let (program, tables, ctx) = compile_test_story("-> spam\n\n=== spam ===\nLine.\n-> spam\n");
    let story = add_story_assets(&mut app, program, tables, ctx);
    app.world_mut().spawn(
        BrinkFlowRequest::<Batched>::builder()
            .story(story.clone())
            .build(),
    );

    app.update(); // fulfill
    app.update(); // batch turn: the flow's Step faults

    let report = app.world().resource::<BrinkBatchReport<Batched>>();
    assert_eq!(
        report.errored, 1,
        "the faulted flow must be counted as errored"
    );
    assert_eq!(
        report.stepped, 0,
        "a faulted flow must not be counted as stepped"
    );
    assert_eq!(
        report.awaiting, 0,
        "a faulted flow is not a deferred-external park"
    );
    assert_eq!(report.flows.len(), 1);
    assert!(
        report.flows[0].errored,
        "the flow's own FlowAccessRecord must also flag errored"
    );
}

/// Buffered command triggers flush in flow-id order at Apply. A command
/// binding records the flow entity into a shared log; the log order must match
/// the flow-id (entity) sort order, independent of collect/step order.
#[test]
fn command_triggers_flush_in_flow_id_order() {
    use crate::{BrinkBindingsAppExt, BrinkCommand, Value as BValue};

    #[derive(Event)]
    struct Ping {
        who: i32,
    }
    impl BrinkCommand for Ping {
        fn from_ink_args(args: &[BValue]) -> Result<Self, crate::BrinkArgError> {
            Ok(Ping {
                who: args.first().and_then(BValue::as_int).unwrap_or(-1),
            })
        }
    }

    #[derive(Resource, Default)]
    struct PingLog(Vec<i32>);

    let mut app = App::new();
    app.add_plugins(AssetPlugin::default());
    app.add_plugins(crate::BrinkPlugin::<Batched>::default());
    app.init_resource::<PingLog>();
    app.bind_brink_command::<Batched, Ping>("ping");
    app.add_observer(|on: On<Ping>, mut log: ResMut<PingLog>| {
        log.0.push(on.event().who);
    });
    app.add_systems(Update, advance_batch::<Batched>);

    // Each flow fires ping(<n>) with its own constant. Two variants so the
    // two flows emit distinct payloads (10 and 20).
    let (p0, t0, c0) = compile_test_story("EXTERNAL ping(n)\n~ temp _ = ping(10)\nHi.\n-> END\n");
    let (p1, t1, c1) = compile_test_story("EXTERNAL ping(n)\n~ temp _ = ping(20)\nHi.\n-> END\n");
    let story0 = add_story_assets(&mut app, p0, t0, c0);
    let story1 = add_story_assets(&mut app, p1, t1, c1);

    let e0 = app
        .world_mut()
        .spawn(BrinkFlowRequest::<Batched>::builder().story(story0).build())
        .id();
    let e1 = app
        .world_mut()
        .spawn(BrinkFlowRequest::<Batched>::builder().story(story1).build())
        .id();

    app.update(); // fulfill
    app.update(); // batch turn: buffer + flush commands in flow-id order
    app.update(); // let the queued command triggers/observers run

    let log = app.world().resource::<PingLog>().0.clone();
    // Both pings fired.
    assert_eq!(log.len(), 2, "both flows' commands fired; got {log:?}");
    // Order matches flow-id (entity) sort order, not spawn/collect order.
    let expect_first = if e0 <= e1 { 10 } else { 20 };
    let expect_second = if e0 <= e1 { 20 } else { 10 };
    assert_eq!(
        log,
        vec![expect_first, expect_second],
        "command triggers must flush in flow-id order (e0={e0:?}, e1={e1:?})"
    );
}

// ── Scenario-harness run of the batch-serial driver (BH-B, night-shift data) ─

/// A scenario-harness run of the **batch-serial** driver: one frame-start
/// batch turn over N flows, measured across flow counts. Attached to the PR
/// per the night-shift data rule — **numbers are provisional / in-wave**; the
/// canonical quiet-window baseline is captured separately (as BH-B-1's serial
/// baseline was, PR #907).
///
/// `#[ignore]` so `cargo test` never runs it (it spawns thousands of entities
/// and times wall clock — not a correctness assertion). Run explicitly:
///
/// ```sh
/// cargo test -p bevy-brink batch_serial_scenario_numbers -- --ignored --nocapture
/// ```
///
/// Measures the cost of one batch turn (frame-start snapshot + serial Step
/// with the per-flow snapshot clone + flow-id-ordered Apply) for a story that
/// writes one global and ends. The per-flow clone is BH-2's known serial cost
/// (§12.2's "borrow, don't copy" is the BH-3 optimization) — these numbers
/// exist to bound it, not to be a throughput target.
#[test]
#[ignore = "scenario harness: wall-clock timing, run explicitly with --ignored --nocapture"]
fn batch_serial_scenario_numbers() {
    use std::time::Instant;

    let flow_counts = [1usize, 64, 512, 4096];
    eprintln!("\n## batch-serial driver — one batch turn (provisional / in-wave)\n");
    eprintln!("| flows | batch turn (ms) | writes applied | us/flow |");
    eprintln!("|------:|----------------:|---------------:|--------:|");

    for &n in &flow_counts {
        let mut app = App::new();
        app.add_plugins(AssetPlugin::default());
        app.add_plugins(crate::BrinkPlugin::<Batched>::default());
        app.add_systems(Update, advance_batch::<Batched>);

        let (program, tables, ctx) =
            compile_test_story("VAR g = 0\nA line of narration.\n~ g = g + 1\n-> END\n");
        let story = add_story_assets(&mut app, program, tables, ctx);
        for _ in 0..n {
            app.world_mut().spawn(
                BrinkFlowRequest::<Batched>::builder()
                    .story(story.clone())
                    .build(),
            );
        }

        // Frame 1 fulfills the requests (creates flows + shared world); it is
        // NOT the measured work.
        app.update();

        // Frame 2 is the batch turn we measure.
        let start = Instant::now();
        app.update();
        let elapsed = start.elapsed();

        let report = app.world().resource::<BrinkBatchReport<Batched>>();
        #[expect(
            clippy::cast_precision_loss,
            reason = "provisional scenario timing; bit-exactness not required"
        )]
        let us_per_flow = (elapsed.as_secs_f64() * 1_000_000.0) / (n.max(1) as f64);
        eprintln!(
            "| {n} | {:.3} | {} | {:.2} |",
            elapsed.as_secs_f64() * 1000.0,
            report.writes_applied,
            us_per_flow,
        );
        assert_eq!(report.stepped, n, "all flows stepped for n={n}");
    }
    eprintln!();
}

/// The **parallelism-curve** companion to [`batch_serial_scenario_numbers`]
/// (BH-3, #927 night-shift data rule): the *same* one-batch-turn measurement
/// driven through the **parallel** [`advance_batch_parallel`] instead of the
/// serial loop, across the same flow counts — so the two runs form the
/// parallel-vs-serial curve attached to the PR. **Numbers are provisional /
/// in-wave** (this is `#[ignore]`, wall-clock timed, and shares the process
/// with the rest of the suite); the canonical quiet-window numbers are captured
/// separately.
///
/// ```sh
/// cargo test -p bevy-brink batch_parallel_scenario_numbers -- --ignored --nocapture
/// ```
#[test]
#[ignore = "scenario harness: wall-clock timing, run explicitly with --ignored --nocapture"]
fn batch_parallel_scenario_numbers() {
    use std::time::Instant;

    let flow_counts = [1usize, 64, 512, 4096];
    eprintln!("\n## batch-PARALLEL driver — one batch turn (provisional / in-wave)\n");
    eprintln!("| flows | batch turn (ms) | writes applied | us/flow |");
    eprintln!("|------:|----------------:|---------------:|--------:|");

    for &n in &flow_counts {
        let mut app = App::new();
        app.add_plugins(AssetPlugin::default());
        app.add_plugins(crate::BrinkPlugin::<Batched>::default());

        let (program, tables, ctx) =
            compile_test_story("VAR g = 0\nA line of narration.\n~ g = g + 1\n-> END\n");
        let story = add_story_assets(&mut app, program, tables, ctx);
        for _ in 0..n {
            app.world_mut().spawn(
                BrinkFlowRequest::<Batched>::builder()
                    .story(story.clone())
                    .build(),
            );
        }

        // Frame 1 fulfills the requests (creates flows + shared world); NOT the
        // measured work. No driver system is registered, so it is not stepped.
        app.update();

        // The measured work: one PARALLEL batch turn (exclusive-system driver,
        // invoked directly).
        let start = Instant::now();
        advance_batch_parallel::<Batched>(app.world_mut());
        let elapsed = start.elapsed();

        let report = app.world().resource::<BrinkBatchReport<Batched>>();
        #[expect(
            clippy::cast_precision_loss,
            reason = "provisional scenario timing; bit-exactness not required"
        )]
        let us_per_flow = (elapsed.as_secs_f64() * 1_000_000.0) / (n.max(1) as f64);
        eprintln!(
            "| {n} | {:.3} | {} | {:.2} |",
            elapsed.as_secs_f64() * 1000.0,
            report.writes_applied,
            us_per_flow,
        );
        assert_eq!(report.stepped, n, "all flows stepped for n={n}");
    }
    eprintln!();
}

// ── BH-3: the parallel Step phase (#927) ────────────────────────────────────

/// THE GATE (#927, closing #926): the **determinism law** — the parallel
/// driver ([`advance_batch_parallel`]) is **byte-identical** to the serial
/// driver ([`advance_batch`]) in flow-id order, over randomized workloads that
/// drive the REAL entry points (not a test reimplementation). A non-
/// deterministic parallel Step (a data race, a completion-order dependency in
/// Apply, a missed frame-start pin) would diverge on some seed; batch mode
/// does not.
///
/// The workload randomizes flow count, per-flow story variant (four shapes
/// that read/write shared globals, incl. one using `RANDOM` off the shared RNG
/// and one ending in a choice), initial world state, RNG seed, and turn count —
/// then runs both drivers over an identical setup and asserts the converged
/// world, the batch report, the per-flow flags, and the emitted line events all
/// match exactly.
#[test]
fn parallel_equals_serial_over_randomized_workloads() {
    for seed in 0u64..64 {
        let mut rng = TestRng::new(seed ^ 0x9E37_79B9_7F4A_7C15);
        let flow_count = rng.pick(1, 10);
        let assignments: Vec<usize> = (0..flow_count)
            .map(|_| rng.pick(0, VARIANTS.len() - 1))
            .collect();
        let init_g = i32::try_from(rng.pick(0, 40)).unwrap_or(0);
        let init_h = i32::try_from(rng.pick(0, 40)).unwrap_or(0);
        let rng_seed = i32::try_from(rng.pick(1, 100_000)).unwrap_or(1);
        let turns = rng.pick(1, 3);

        let serial = run_workload(
            &assignments,
            init_g,
            init_h,
            rng_seed,
            turns,
            Driver::Serial,
        );
        let parallel = run_workload(
            &assignments,
            init_g,
            init_h,
            rng_seed,
            turns,
            Driver::Parallel,
        );

        assert_eq!(
            serial, parallel,
            "determinism law violated: parallel != serial at seed={seed} \
             assignments={assignments:?} turns={turns} init=({init_g},{init_h}) rng={rng_seed}"
        );
    }
}

/// `advance_batch_parallel::<M>` wired as a real Bevy `Update` system (the
/// exact host opt-in path from the docs) advances every pending flow through
/// the parallel Step phase and converges the shared world — proving the entry
/// point is reachable, not just unit-covered.
#[test]
fn advance_batch_parallel_system_drives_and_applies() {
    let mut app = App::new();
    app.add_plugins(AssetPlugin::default());
    app.add_plugins(crate::BrinkPlugin::<Batched>::default());
    app.add_systems(Update, advance_batch_parallel::<Batched>);

    let (program, tables, ctx) = compile_test_story("VAR g = 0\nHi.\n~ g = 3\n-> END\n");
    let story = add_story_assets(&mut app, program, tables, ctx);
    for _ in 0..3 {
        app.world_mut().spawn(
            BrinkFlowRequest::<Batched>::builder()
                .story(story.clone())
                .build(),
        );
    }

    app.update(); // fulfillment creates the flows + shared BrinkGlobals
    app.update(); // parallel batch turn

    let report = app.world().resource::<BrinkBatchReport<Batched>>();
    assert_eq!(report.flows.len(), 3, "all three flows collected");
    assert_eq!(report.skipped_local, 0);
    assert_eq!(
        report.stepped + report.awaiting + report.errored,
        3,
        "every collected flow is accounted for"
    );
    // Both flows wrote g = 3 (absolute), last-wins → converged world g = 3.
    let globals = app.world().resource::<BrinkGlobals<Batched>>();
    assert_eq!(globals.inner.global(0), &Value::Int(3));
}

/// #925 (serial): a flow whose policy homes state to `Local` is **skipped**,
/// not stepped — counted distinctly in `skipped_local`, its world write never
/// applied. Guards the silent-corruption gap PR #920 documented.
#[test]
fn advance_batch_skips_local_policy_flow() {
    let (mut app, story) = local_policy_app();
    app.world_mut()
        .spawn(BrinkFlowRequest::<Batched>::builder().story(story).build());
    app.update(); // fulfill
    app.world_mut()
        .run_system_once(advance_batch::<Batched>)
        .expect("serial batch runs");
    assert_local_skipped(&app);
}

/// #925 (parallel): the same Local-policy guard fires on the parallel driver —
/// the guard lives in the shared Collect/Step path both drivers use.
#[test]
fn advance_batch_parallel_skips_local_policy_flow() {
    let (mut app, story) = local_policy_app();
    app.world_mut()
        .spawn(BrinkFlowRequest::<Batched>::builder().story(story).build());
    app.update(); // fulfill
    advance_batch_parallel::<Batched>(app.world_mut());
    assert_local_skipped(&app);
}

// ── BH-3 test scaffolding ───────────────────────────────────────────────────

/// Which driver a workload run drives — both are real production entry points.
#[derive(Clone, Copy)]
enum Driver {
    Serial,
    Parallel,
}

/// Four story shapes sharing an identical global layout (`VAR g`, then `VAR
/// h`, so slot 0 == g and slot 1 == h in every variant): a g-reader/g-writer,
/// a g-reader/h-writer, a `RANDOM`-off-the-shared-RNG writer, and a
/// choice-terminating flow. Enough cross-flow read/write interaction that a
/// naive immediate-visibility parallel driver would diverge across runs.
const VARIANTS: &[&str] = &[
    "VAR g = 0\nVAR h = 0\nGee {g}.\n~ g = g + 3\n-> END\n",
    "VAR g = 0\nVAR h = 0\n~ h = g * 2\nAich {h}.\n-> END\n",
    "VAR g = 0\nVAR h = 0\n~ g = RANDOM(1, 6)\nRoll {g}.\n-> END\n",
    "VAR g = 0\nVAR h = 0\nHello.\n+ [Wait]\n    -> END\n",
];

/// Deterministic PCG-style LCG (same idiom as the scenario harness) — no
/// wall-clock/OS entropy, so a workload is byte-identical on every run.
struct TestRng(u64);

impl TestRng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    /// Inclusive `[lo, hi]`.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "ranges here are always small (flow counts, variant indices, seeds); truncation of the high LCG bits is intended and harmless"
    )]
    fn pick(&mut self, lo: usize, hi: usize) -> usize {
        lo + (self.next() as usize) % (hi - lo + 1)
    }
}

/// The full observable outcome of a batch workload run — everything the
/// determinism law compares. Byte-identity is `PartialEq` over these fields.
#[derive(PartialEq, Debug)]
struct RunSnapshot {
    globals: Vec<Value>,
    visit_counts: HashMap<DefinitionId, u32>,
    turn_counts: HashMap<DefinitionId, u32>,
    turn_index: u32,
    rng_seed: i32,
    previous_random: i32,
    /// `(stepped, awaiting, errored, skipped_local, writes_applied, commands_applied)`.
    report: (usize, usize, usize, usize, usize, usize),
    /// `(awaiting, errored, skipped_local)` per flow, in flow-id order.
    flow_flags: Vec<(bool, bool, bool)>,
    /// `(kind, text)` per emitted line event, in emission order (kind: 0 text,
    /// 1 choices, 2 done, 3 end).
    events: Vec<(u8, String)>,
}

#[derive(Resource, Default)]
struct EventLog(Vec<(u8, String)>);

fn install_event_log(app: &mut App) {
    app.init_resource::<EventLog>();
    app.add_observer(
        |on: On<BrinkLineDelivered<Batched>>, mut log: ResMut<EventLog>| {
            log.0.push((0, on.event().text.clone()));
        },
    );
    app.add_observer(
        |on: On<BrinkChoicesPresented<Batched>>, mut log: ResMut<EventLog>| {
            log.0.push((1, on.event().text.clone()));
        },
    );
    app.add_observer(
        |on: On<BrinkTurnDone<Batched>>, mut log: ResMut<EventLog>| {
            log.0.push((2, on.event().text.clone()));
        },
    );
    app.add_observer(
        |on: On<BrinkStoryEnded<Batched>>, mut log: ResMut<EventLog>| {
            log.0.push((3, on.event().text.clone()));
        },
    );
}

/// Build an identical app + flow set, drive `turns` batch turns with the given
/// driver (invoked directly, so exclusive-vs-normal system *scheduling* can't
/// perturb the comparison — only the driver's internal parallelism differs),
/// and snapshot the full observable outcome.
fn run_workload(
    assignments: &[usize],
    init_g: i32,
    init_h: i32,
    rng_seed: i32,
    turns: usize,
    driver: Driver,
) -> RunSnapshot {
    let mut app = App::new();
    app.add_plugins(AssetPlugin::default());
    app.add_plugins(crate::BrinkPlugin::<Batched>::default());
    install_event_log(&mut app);

    let story_handles: Vec<_> = VARIANTS
        .iter()
        .map(|src| {
            let (p, t, c) = compile_test_story(src);
            add_story_assets(&mut app, p, t, c)
        })
        .collect();

    for &v in assignments {
        app.world_mut().spawn(
            BrinkFlowRequest::<Batched>::builder()
                .story(story_handles[v].clone())
                .build(),
        );
    }

    // Fulfillment only (no driver system added) — flows are created but not
    // stepped, so the initial state below is pristine for both drivers.
    app.update();
    app.update();

    // Randomize the shared world identically for both drivers.
    {
        let mut globals = app.world_mut().resource_mut::<BrinkGlobals<Batched>>();
        globals.inner.set_global(0, Value::Int(init_g));
        globals.inner.set_global(1, Value::Int(init_h));
        globals.inner.set_rng_seed(rng_seed);
    }

    for _ in 0..turns {
        match driver {
            Driver::Serial => {
                app.world_mut()
                    .run_system_once(advance_batch::<Batched>)
                    .expect("serial batch runs");
            }
            Driver::Parallel => advance_batch_parallel::<Batched>(app.world_mut()),
        }
    }

    let world = &app.world().resource::<BrinkGlobals<Batched>>().inner;
    let report = app.world().resource::<BrinkBatchReport<Batched>>();
    RunSnapshot {
        globals: world.globals.clone(),
        visit_counts: world.visit_counts.clone(),
        turn_counts: world.turn_counts.clone(),
        turn_index: world.turn_index,
        rng_seed: world.rng_seed,
        previous_random: world.previous_random,
        report: (
            report.stepped,
            report.awaiting,
            report.errored,
            report.skipped_local,
            report.writes_applied,
            report.commands_applied,
        ),
        flow_flags: report
            .flows
            .iter()
            .map(|f| (f.awaiting, f.errored, f.skipped_local))
            .collect(),
        events: app.world().resource::<EventLog>().0.clone(),
    }
}

/// Build an app whose installed `WorldPolicy` homes `g` to `Local`, plus a
/// story declaring `g` that writes it — so batching the flow (which routes
/// only the shared World) must skip rather than silently drop the write.
fn local_policy_app() -> (App, bevy_asset::Handle<crate::asset::BrinkStoryAsset>) {
    let policy = WorldPolicy {
        overrides: std::iter::once(("g".to_string(), Scope::Local)).collect(),
        ..Default::default()
    };
    let mut app = App::new();
    app.add_plugins(AssetPlugin::default());
    app.add_plugins(crate::BrinkPlugin::<Batched>::default().with_policy(policy));
    let (program, tables, ctx) = compile_test_story("VAR g = 0\nHi.\n~ g = 5\n-> END\n");
    let story = add_story_assets(&mut app, program, tables, ctx);
    (app, story)
}

fn assert_local_skipped(app: &App) {
    let report = app.world().resource::<BrinkBatchReport<Batched>>();
    assert_eq!(report.skipped_local, 1, "the Local-policy flow was skipped");
    assert_eq!(report.stepped, 0, "a skipped flow is not stepped");
    assert_eq!(report.awaiting, 0);
    assert_eq!(report.errored, 0);
    assert_eq!(report.writes_applied, 0, "a skipped flow applies no writes");
    assert!(
        report.flows.iter().all(|f| f.skipped_local),
        "every flow record flags skipped_local"
    );
    // The skipped flow's `~ g = 5` never landed: shared World `g` base stays 0.
    let globals = app.world().resource::<BrinkGlobals<Batched>>();
    assert_eq!(globals.inner.global(0), &Value::Int(0));
}
