//! BH-4 tests (`docs/effects-spec.md` §13.1; #973): the `FlowSleep` wake
//! contract driven end-to-end through the plugin's own wake systems +
//! `advance_batch` (reachability, not just unit coverage), plus the `#913`
//! detect-verdict consumption and the BH-B wake-fan-out scenario ratios.

use super::*;

use bevy_app::{App, Update};
use bevy_ecs::observer::On;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::ResMut;
use brink_runtime::ContextAccess as _;

use crate::advance_batch;
use crate::capability::ContainerAccess;
use crate::event::{BrinkLineDelivered, BrinkStoryEnded, BrinkTurnDone};
use crate::globals::BrinkGlobals;
use crate::test_support::{add_story_assets, compile_test_story, make_test_app};
use crate::{BrinkBatchReport, BrinkFlowRequest};

/// Accumulates every line/turn/end text an entity produces so a test can
/// assert on player-visible output across frames.
#[derive(Resource, Default)]
struct TextLog(String);

/// A story that runs one turn ("Woke up!" `-> DONE`), then a second turn on
/// resume ("Second turn." `-> END`). The wake condition `should_wake` returns
/// the `gate` global — truthy (nonzero) means "wake".
const GATED_STORY: &str = "VAR gate = 0\n\
     Woke up!\n-> DONE\n\
     Second turn.\n-> END\n\
     === function should_wake() ===\n~ return gate\n";

/// A story whose knot re-parks at `-> DONE` every turn forever — a wake under
/// a persistent policy runs exactly one "Beat." turn and re-arms.
const LOOPING_STORY: &str = "VAR gate = 1\n\
     -> beat\n\
     === beat ===\n\
     Beat.\n-> DONE\n-> beat\n\
     === function should_wake() ===\n~ return gate\n";

fn build_app() -> App {
    let mut app = make_test_app();
    app.add_systems(Update, advance_batch::<()>);
    app.insert_resource(TextLog::default());
    app.add_observer(|t: On<BrinkLineDelivered<()>>, mut log: ResMut<TextLog>| {
        log.0.push_str(&t.event().text);
    });
    app.add_observer(|t: On<BrinkTurnDone<()>>, mut log: ResMut<TextLog>| {
        log.0.push_str(&t.event().text);
    });
    app.add_observer(|t: On<BrinkStoryEnded<()>>, mut log: ResMut<TextLog>| {
        log.0.push_str(&t.event().text);
    });
    app
}

/// Mutate the shared `gate` global (marks `BrinkGlobals` changed, which is the
/// dependency-change signal `mark_wake_dirty` keys off for detect-capable
/// policies).
fn set_gate(app: &mut App, program_global_idx: u32, value: i32) {
    app.world_mut()
        .resource_mut::<BrinkGlobals<()>>()
        .inner
        .set_global(program_global_idx, Value::Int(value));
}

fn pump(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update();
    }
}

// ── The core contract ───────────────────────────────────────────────────────

/// Reachability + points 1, 2, 6: a **dormant** flow spawned with a
/// `FlowSleep` policy never runs while its condition is false (Collect skips
/// it — zero steps), and runs its first turn only once the condition goes true.
/// Driven entirely through the plugin's registered wake systems + a
/// host-registered `advance_batch` — no direct call into `run_flow_sleep`.
#[test]
fn dormant_flow_stays_parked_until_condition_true() {
    let mut app = build_app();
    let (program, tables, ctx) = compile_test_story(GATED_STORY);
    let gate_idx = program.global_index("gate").expect("gate global exists");
    let story = add_story_assets(&mut app, program, tables, ctx);

    // Spawn dormant: the request + a dormant persistent policy on one entity.
    app.world_mut().spawn((
        BrinkFlowRequest::<()>::builder().story(story).build(),
        FlowSleep::<()>::persistent("should_wake").dormant(),
    ));

    // Fulfill + several wake passes with gate == 0: the flow must never step.
    pump(&mut app, 6);
    assert!(
        app.world().resource::<TextLog>().0.is_empty(),
        "a dormant flow whose condition is false must never run: got {:?}",
        app.world().resource::<TextLog>().0
    );
    // Its policy is still parked, not woken/faulted.
    let sleep = single_sleep(&mut app);
    assert_eq!(sleep, SleepState::Parked, "still parked while gate == 0");

    // Flip the dependency true → the flow wakes and runs.
    set_gate(&mut app, gate_idx, 1);
    pump(&mut app, 6);
    assert!(
        app.world().resource::<TextLog>().0.contains("Woke up!"),
        "flow should wake and run its first turn once gate != 0: got {:?}",
        app.world().resource::<TextLog>().0
    );
}

/// Point 2, "re-evaluate, don't wake": while a dependency changes but the
/// condition stays false, the flow is re-evaluated (not woken). We flip gate
/// to a still-false value (0 → 0 is not a change; use a distinct false-ish
/// path) — here we prove the inverse cleanly: a woken persistent flow that
/// then has its gate cleared re-parks and does NOT keep running.
#[test]
fn persistent_policy_reparks_when_condition_goes_false() {
    let mut app = build_app();
    let (program, tables, ctx) = compile_test_story(LOOPING_STORY);
    let gate_idx = program.global_index("gate").expect("gate global exists");
    let story = add_story_assets(&mut app, program, tables, ctx);

    // Persistent, must-poll (a single non-detect-capable dependency) so it
    // re-evaluates every pass regardless of change-detection timing.
    app.world_mut().spawn((
        BrinkFlowRequest::<()>::builder().story(story).build(),
        FlowSleep::<()>::persistent("should_wake").with_detect(DetectSummary::from_bits(
            [("Poll".to_string(), false)].into_iter().collect(),
        )),
    ));

    // gate == 1 (from the fixture): the flow keeps waking + running "Beat."
    // turns. Pump a while and confirm it ran more than once (a wake storm).
    pump(&mut app, 8);
    let beats_running = app.world().resource::<TextLog>().0.matches("Beat.").count();
    assert!(
        beats_running >= 2,
        "a persistent always-true policy should re-arm and run repeatedly; got {beats_running} beats"
    );

    // Clear the gate → the next re-evaluation is false → the flow re-parks and
    // stops producing beats.
    set_gate(&mut app, gate_idx, 0);
    pump(&mut app, 4); // let it settle (finish any in-flight woken turn)
    let after_clear = app.world().resource::<TextLog>().0.matches("Beat.").count();
    pump(&mut app, 6);
    let final_count = app.world().resource::<TextLog>().0.matches("Beat.").count();
    assert_eq!(
        final_count, after_clear,
        "once the condition is false the flow must stop running (parked, zero cost)"
    );
    assert_eq!(single_sleep(&mut app), SleepState::Parked);
}

/// Point 4: `wake_once` fires exactly once, then the policy is removed and the
/// flow reverts to ordinary advancement.
#[test]
fn wake_once_fires_once_then_retires() {
    let mut app = build_app();
    let (program, tables, ctx) = compile_test_story(GATED_STORY);
    let gate_idx = program.global_index("gate").expect("gate global exists");
    let story = add_story_assets(&mut app, program, tables, ctx);
    let entity = app
        .world_mut()
        .spawn((
            BrinkFlowRequest::<()>::builder().story(story).build(),
            FlowSleep::<()>::once("should_wake").dormant(),
        ))
        .id();

    pump(&mut app, 4); // parked, gate == 0
    set_gate(&mut app, gate_idx, 1);
    pump(&mut app, 8);

    // The one-shot fired and was removed once it woke and ran its turn.
    assert!(
        app.world().entity(entity).get::<FlowSleep<()>>().is_none(),
        "a wake_once policy must be removed after firing"
    );
    assert!(
        app.world().resource::<TextLog>().0.contains("Woke up!"),
        "the one-shot turn should have run"
    );
}

/// Point 4 / "cancellation → false": a cancelled policy resolves its condition
/// to a permanent false — the flow never wakes even when the dependency later
/// goes true.
#[test]
fn cancel_resolves_condition_to_false_forever() {
    let mut app = build_app();
    let (program, tables, ctx) = compile_test_story(GATED_STORY);
    let gate_idx = program.global_index("gate").expect("gate global exists");
    let story = add_story_assets(&mut app, program, tables, ctx);
    let entity = app
        .world_mut()
        .spawn((
            BrinkFlowRequest::<()>::builder().story(story).build(),
            FlowSleep::<()>::persistent("should_wake").dormant(),
        ))
        .id();

    pump(&mut app, 3); // fulfilled, parked

    // Cancel the policy, THEN make the dependency true. The flow must stay put.
    app.world_mut()
        .get_mut::<FlowSleep<()>>(entity)
        .expect("policy present")
        .cancel();
    set_gate(&mut app, gate_idx, 1);
    pump(&mut app, 8);

    assert!(
        app.world().resource::<TextLog>().0.is_empty(),
        "a cancelled policy must never wake the flow, even once gate != 0: got {:?}",
        app.world().resource::<TextLog>().0
    );
    assert_eq!(
        app.world()
            .entity(entity)
            .get::<FlowSleep<()>>()
            .expect("still attached")
            .state(),
        SleepState::Cancelled
    );
}

/// Point 5: an `-> END` flow is dead and the policy is inert — the component is
/// dropped once the flow ends.
#[test]
fn ended_flow_drops_its_policy() {
    let mut app = build_app();
    // A story that ends on its very first (and only) turn.
    let (program, tables, ctx) = compile_test_story(
        "VAR gate = 1\nDone here.\n-> END\n=== function should_wake() ===\n~ return gate\n",
    );
    let story = add_story_assets(&mut app, program, tables, ctx);
    let entity = app
        .world_mut()
        .spawn((
            BrinkFlowRequest::<()>::builder().story(story).build(),
            FlowSleep::<()>::persistent("should_wake").dormant(),
        ))
        .id();

    // gate == 1 from the start: it wakes, runs its only turn to END, and the
    // now-dead policy is dropped.
    pump(&mut app, 8);
    assert!(
        app.world().entity(entity).get::<FlowSleep<()>>().is_none(),
        "an -> END flow's policy is inert and must be removed"
    );
    assert!(app.world().resource::<TextLog>().0.contains("Done here."));
}

// ── The Detect phase (#913 consumption) ─────────────────────────────────────

/// The `#913` AND-merge, consumed at the wake layer: a `DetectSummary` built
/// from a container whose `detect` map folded a conflicting bit to `false`
/// classifies the policy as **must-poll** (`dependencies_all_detect_capable ==
/// false`). Without the AND-merge (last-write-wins leaving `true`) this would
/// wrongly report detect-capable and risk a missed wake.
#[test]
fn detect_summary_from_and_merged_container_polls_on_conflict() {
    // A container access carrying the AND-merged result of a conflict
    // (Transform read by one detect-capable + one opaque external → false).
    let access = ContainerAccess {
        detect: [("Transform".to_string(), false)].into_iter().collect(),
        ..ContainerAccess::default()
    };
    let summary = DetectSummary::from_container_access(&access);
    assert!(
        !summary.all_detect_capable,
        "a must-poll (false) merged bit must classify the policy as polling"
    );

    let sleep = FlowSleep::<()>::persistent("cond").with_detect(summary);
    assert!(
        !sleep.dependencies_all_detect_capable(),
        "the policy inherits the must-poll verdict — it will re-evaluate every pass"
    );
}

/// The complementary case: all-`true` (or empty) detect maps stay
/// detect-capable, so the policy re-evaluates only on a World change.
#[test]
fn detect_summary_all_true_or_empty_is_detect_capable() {
    let all_true = DetectSummary::from_bits(
        [
            ("Transform".to_string(), true),
            ("Health".to_string(), true),
        ]
        .into_iter()
        .collect(),
    );
    assert!(all_true.all_detect_capable);

    let empty = DetectSummary::from_bits(std::collections::BTreeMap::new());
    assert!(
        empty.all_detect_capable,
        "no external-capability dependency → change-detectable via the ink World"
    );
}

/// Review finding: `DetectSummary::default()` (what [`FlowSleep::new`] builds
/// every policy with before an optional [`FlowSleep::with_detect`]) must match
/// [`DetectSummary::from_bits`]'s vacuous-true semantics for an empty map — a
/// derived `Default` would instead leave `all_detect_capable: false`, silently
/// forcing every policy built without `.with_detect` onto the must-poll path
/// (the opposite of the documented "no dependency → cheap path" default).
#[test]
fn detect_summary_default_matches_vacuous_true_from_bits() {
    assert_eq!(
        DetectSummary::default(),
        DetectSummary::from_bits(std::collections::BTreeMap::new()),
        "DetectSummary::default() must agree with from_bits(empty): both vacuously \
         all-detect-capable"
    );
    assert!(
        DetectSummary::default().all_detect_capable,
        "a policy built without .with_detect must default to the cheap \
         (all-detect-capable) path, not must-poll"
    );

    // The consumer that actually matters: FlowSleep::new goes through
    // DetectSummary::default() (via persistent/once), never from_bits directly.
    let sleep = FlowSleep::<()>::persistent("cond");
    assert!(
        sleep.dependencies_all_detect_capable(),
        "a freshly built FlowSleep with no .with_detect must be all-detect-capable"
    );
}

#[test]
fn condition_truthiness_matches_ink_coercion() {
    assert!(is_condition_true(&Value::Bool(true)));
    assert!(!is_condition_true(&Value::Bool(false)));
    assert!(is_condition_true(&Value::Int(1)));
    assert!(is_condition_true(&Value::Int(-1)));
    assert!(!is_condition_true(&Value::Int(0)));
    assert!(is_condition_true(&Value::Float(0.5)));
    assert!(!is_condition_true(&Value::Float(0.0)));
    // Non-numeric / null → conservative false (park, never spuriously wake).
    assert!(!is_condition_true(&Value::Null));
}

// ── BH-B: the wake-fan-out scenario (provisional night-data) ─────────────────

/// The BH-B wake-fan-out axis (this issue's night-data deliverable,
/// **provisional**): parked flows cost zero (skipped by Collect) while active
/// flows step, and a wake-storm (all-persistent, always-true) steps every flow
/// every turn. Asserts the structural step ratios that back
/// `benches/baselines/wake-fan-out-provisional.md`.
#[test]
fn wake_fan_out_scenario_ratios() {
    // 8 flows total; 6 parked (dormant, gate == 0), 2 active (no policy).
    let mut app = build_app();
    let (program, tables, ctx) = compile_test_story(GATED_STORY);
    let story = add_story_assets(&mut app, program, tables, ctx);

    let parked = 6usize;
    let active = 2usize;
    for _ in 0..parked {
        app.world_mut().spawn((
            BrinkFlowRequest::<()>::builder()
                .story(story.clone())
                .build(),
            FlowSleep::<()>::persistent("should_wake").dormant(),
        ));
    }
    for _ in 0..active {
        app.world_mut().spawn(
            BrinkFlowRequest::<()>::builder()
                .story(story.clone())
                .build(),
        );
    }

    app.update(); // fulfill
    app.update(); // one batch turn

    let report = app.world().resource::<BrinkBatchReport<()>>();
    // Only the 2 active flows stepped; the 6 parked flows were skipped by
    // Collect entirely (zero cost) — not stepped, not awaiting, not errored.
    assert_eq!(
        report.stepped, active,
        "exactly the active flows step; parked flows are skipped by Collect"
    );
    assert_eq!(report.awaiting, 0);
    assert_eq!(report.errored, 0);
    assert!(
        report.flows.len() <= active,
        "parked flows contribute no per-flow batch record: {} records for {active} active",
        report.flows.len()
    );

    // Wake-storm: a fresh app where every flow is persistent + always-true →
    // every flow wakes and steps each turn.
    let mut storm_app = build_app();
    let (sp, st, sc) = compile_test_story(LOOPING_STORY);
    let storm_story = add_story_assets(&mut storm_app, sp, st, sc);
    let storm_n = 5usize;
    for _ in 0..storm_n {
        storm_app.world_mut().spawn((
            BrinkFlowRequest::<()>::builder()
                .story(storm_story.clone())
                .build(),
            FlowSleep::<()>::persistent("should_wake").with_detect(DetectSummary::from_bits(
                [("Poll".to_string(), false)].into_iter().collect(),
            )),
        ));
    }
    // Pump enough frames that every flow has woken at least once and a batch
    // turn stepped all of them together.
    let mut max_stepped = 0usize;
    for _ in 0..12 {
        storm_app.update();
        max_stepped =
            max_stepped.max(storm_app.world().resource::<BrinkBatchReport<()>>().stepped);
    }
    assert_eq!(
        max_stepped, storm_n,
        "wake storm: all {storm_n} flows wake and step together in a batch turn"
    );
}

// ── helpers ─────────────────────────────────────────────────────────────────

/// The lone `FlowSleep<()>` policy's state, for single-flow tests.
fn single_sleep(app: &mut App) -> SleepState {
    let mut q = app.world_mut().query::<&FlowSleep<()>>();
    let sleeps: Vec<SleepState> = q.iter(app.world()).map(FlowSleep::state).collect();
    assert_eq!(sleeps.len(), 1, "expected exactly one FlowSleep policy");
    sleeps[0]
}
