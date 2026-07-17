//! Tests for batch-mode advancement (BH-2, #914) — most importantly THE
//! GATE: the order-invariance property (a batch of N flows produces identical
//! outcomes under any within-batch step permutation).

use super::*;

use bevy_app::{App, Update};
use bevy_asset::AssetPlugin;
use bevy_ecs::event::Event;
use bevy_ecs::observer::On;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::ResMut;
use brink_runtime::Line;

use crate::BrinkFlowRequest;
use crate::test_support::{add_story_assets, compile_test_story};

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
        let (lines, _awaiting) = step_flow(
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
    let (lines_a, _) = step_flow(
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
    let (lines_b, _) = step_flow(
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
