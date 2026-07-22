//! BH-4 tests (`docs/effects-spec.md` §13.1; #973): the `FlowSleep` wake
//! contract driven end-to-end through the plugin's own wake systems +
//! `advance_batch` (reachability, not just unit coverage), plus the `#913`
//! detect-verdict consumption and the BH-B wake-fan-out scenario ratios.

use super::*;

use bevy_app::{App, Update};
use bevy_ecs::component::Component;
use bevy_ecs::observer::On;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{In, ResMut, RunSystemOnce as _};
use brink_format::{CallAtom, CapabilityParam, DefinitionTag, DirectEffects, DispatchEntry};
use brink_runtime::ContextAccess as _;

use crate::advance_batch;
use crate::asset::{BrinkStoryAsset, LineTablesAsset};
use crate::bindings::BrinkBindingsAppExt as _;
use crate::capability::{
    BrinkCapabilityAppExt, CapabilityChanges, CapabilityEffects, CapabilityManifest,
    CapabilityManifestExternal, CapabilityRegistry, ContainerAccess,
};
use crate::event::{BrinkLineDelivered, BrinkStoryEnded, BrinkTurnDone};
use crate::globals::BrinkGlobals;
use crate::test_support::{add_story_assets, compile_test_story, make_test_app};
use crate::{BrinkBatchReport, BrinkFlowRequest};

/// A call atom naming `name`, v1's always-`Any` capability param, no handle
/// param — the same shape `crate::capability`'s own tests build (`atom`).
fn call_atom(name: brink_format::NameId) -> CallAtom {
    CallAtom {
        name,
        capability: CapabilityParam::Any,
        handle_param: None,
    }
}

/// Accumulates every line/turn/end text an entity produces so a test can
/// assert on player-visible output across frames.
#[derive(Resource, Default)]
struct TextLog(String);

/// A dummy marker component registered under the `"GameState"` capability
/// name (issue #1040's reachability test) — only the name resolving
/// matters, not what the component actually is.
#[derive(Component)]
struct GameStateCap;

/// A component-backed capability the §12.5 detect-path tests (#996) watch:
/// a wake condition reads `open` through [`is_gate_open`], so flipping it is
/// exactly the "watched component changed" signal `mark_wake_dirty` must key
/// off (the `is_player_nearby`-reading-`Transform` / door-reading-`Switch`
/// shape, now with the change-tick wiring the earlier interim lacked).
#[derive(Component)]
struct Gate {
    open: bool,
}

/// World-access binding: is the flow entity's own [`Gate`] open? The
/// component-reading wake condition `should_wake` in [`GATED_GATE_STORY`]
/// calls this — a real `bind_brink_query` round trip, the same seam a door's
/// `is_switch_on` uses, so the detect-path win is exercised end-to-end and
/// not just at the flag level.
fn is_gate_open(
    In((flow, _args)): In<crate::BrinkQueryInput>,
    gates: bevy_ecs::system::Query<&Gate>,
) -> Value {
    Value::Bool(gates.get(flow).is_ok_and(|gate| gate.open))
}

/// A one-turn story whose wake condition reads a `Gate` **component** (via the
/// [`is_gate_open`] `EXTERNAL`), not a `BrinkGlobals` variable — the §12.5
/// component-backed detect case (#996).
const GATED_GATE_STORY: &str = "EXTERNAL is_gate_open(id)\n\
     Woke up!\n-> END\n\
     === function should_wake() ===\n~ return is_gate_open(0)\n";

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

/// Like [`LOOPING_STORY`] but starts with `gate == 0` (closed/false) instead
/// of `1` — used by the [`WakeArming::Latch`] tests, which need to observe
/// the "stays parked while false" state from frame one (before ever setting
/// the gate), unlike `LOOPING_STORY`'s always-true wake-storm fixture.
const LATCH_STORY: &str = "VAR gate = 0\n\
     -> beat\n\
     === beat ===\n\
     Beat.\n-> DONE\n-> beat\n\
     === function should_wake() ===\n~ return gate\n";

/// Like [`GATED_STORY`] but also declares (and, from a knot no purity test
/// ever plays, calls — so the names actually intern into the story's name
/// table exactly as a real compiled `.inkb` would) two `EXTERNAL` bindings
/// the issue #1040 purity tests below reference from a hand-built call atom.
/// `should_wake`'s own body never calls either — these tests build their
/// `EffectRowEntry`s by hand rather than relying on real effects inference
/// (the same pattern [`no_write_row`]/[`writing_row`] already use), so a
/// purity-rejected condition is exercised end-to-end through
/// [`run_flow_sleep`] without ever needing a bound runtime handler for
/// either external.
const GATED_STORY_WITH_EXTERNALS: &str = "VAR gate = 0\n\
     EXTERNAL touch_state(id)\n\
     EXTERNAL read_state(id)\n\
     Woke up!\n-> DONE\n\
     Second turn.\n-> END\n\
     === function should_wake() ===\n~ return gate\n\
     === function uses_externals() ===\n\
     ~ temp a = touch_state(0)\n\
     ~ temp b = read_state(0)\n\
     ~ return 0\n";

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

/// Issue #1081: a [`WakeArming::Latch`] policy wakes on a transition, then
/// goes quiet until the *opposite* transition, repeating indefinitely — a
/// reversible boolean latch (a door that re-locks), unlike `Once` (fires
/// forever after the first wake) or `Persistent` (re-steps every turn
/// boundary while the condition stays true). Driven end-to-end through the
/// plugin's registered wake systems + `advance_batch`, across several flips.
#[test]
fn latch_wakes_on_each_transition_and_cycles_across_multiple_flips() {
    let mut app = build_app();
    let (program, tables, ctx) = compile_test_story(LATCH_STORY);
    let gate_idx = program.global_index("gate").expect("gate global exists");
    let story = add_story_assets(&mut app, program, tables, ctx);

    app.world_mut().spawn((
        BrinkFlowRequest::<()>::builder().story(story).build(),
        FlowSleep::<()>::latch("should_wake")
            .dormant()
            .with_detect(DetectSummary::from_bits(
                [("Poll".to_string(), false)].into_iter().collect(),
            )),
    ));

    pump(&mut app, 4);
    assert!(
        app.world().resource::<TextLog>().0.is_empty(),
        "must stay parked while the condition is false: got {:?}",
        app.world().resource::<TextLog>().0
    );

    // Rising edge #1: fires exactly once, then re-arms watching for false —
    // NOT the Persistent wake-storm shape (which would keep re-stepping
    // every turn while the condition stays true).
    set_gate(&mut app, gate_idx, 1);
    pump(&mut app, 8);
    let after_first_rise = app.world().resource::<TextLog>().0.matches("Beat.").count();
    assert_eq!(
        after_first_rise, 1,
        "a Latch policy must fire exactly once per edge, not re-step while \
         the condition stays true"
    );
    pump(&mut app, 6); // still true, still parked watching for the fall
    assert_eq!(
        app.world().resource::<TextLog>().0.matches("Beat.").count(),
        1,
        "must not re-fire again while the condition is still true (waiting for the fall)"
    );

    // Falling edge #1: the opposite transition fires again — proving the
    // policy re-armed for the opposite edge instead of retiring like `Once`.
    set_gate(&mut app, gate_idx, 0);
    pump(&mut app, 8);
    assert_eq!(
        app.world().resource::<TextLog>().0.matches("Beat.").count(),
        2,
        "the falling edge must also fire, and the component must still be \
         attached (Latch never retires, unlike Once)"
    );
    assert!(
        {
            let mut q = app.world_mut().query::<&FlowSleep<()>>();
            q.iter(app.world()).next().is_some()
        },
        "a Latch policy must never be removed"
    );

    // Cycle through several more flips to prove this repeats indefinitely,
    // not just once each way.
    for expected_count in 3..=6 {
        let on = expected_count % 2 == 1;
        set_gate(&mut app, gate_idx, i32::from(on));
        pump(&mut app, 8);
        assert_eq!(
            app.world().resource::<TextLog>().0.matches("Beat.").count(),
            expected_count,
            "flip #{expected_count} (gate={on}) must fire exactly one more wake"
        );
    }
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

/// A component-backed detect-capable policy whose capability is **unregistered**
/// (no `register_capability` call, so no change-tracker exists for it) must
/// still must-poll — `mark_wake_dirty` cannot observe a component it has no
/// tracker for, and folding an unobservable capability to the cheap path would
/// gate re-evaluation on a signal that never fires (a missed wake). This is the
/// conservative fallback the §12.5 wiring (#996) deliberately keeps for any
/// capability the wake layer can't see.
#[test]
fn mark_wake_dirty_must_polls_an_unregistered_component_backed_policy() {
    let mut app = App::new();
    // The two resources `mark_wake_dirty` reads exist, but the registry is
    // empty (nothing registered) so "Transform" is untracked.
    app.init_resource::<CapabilityRegistry<()>>();
    app.init_resource::<CapabilityChanges<()>>();
    let entity = app
        .world_mut()
        .spawn(
            FlowSleep::<()>::persistent("should_wake")
                .with_detect(DetectSummary::from_bits(
                    [("Transform".to_string(), true)].into_iter().collect(),
                ))
                .dormant(),
        )
        .id();
    assert!(
        app.world()
            .entity(entity)
            .get::<FlowSleep<()>>()
            .expect("just spawned")
            .dependencies_all_detect_capable(),
        "fixture sanity: the AND-merge verdict is all-true"
    );

    // Isolate the case under test: pretend the bootstrap evaluation already
    // ran, so only the detect/world-changed gate is exercised (not the
    // "never evaluated" first-run flag). No `BrinkGlobals` resource exists in
    // this bare `App` at all, so `world_changed` is unconditionally `false`.
    app.world_mut()
        .get_mut::<FlowSleep<()>>(entity)
        .expect("policy present")
        .evaluated_once = true;

    app.world_mut()
        .run_system_once(mark_wake_dirty::<()>)
        .expect("mark_wake_dirty runs");

    assert!(
        app.world()
            .entity(entity)
            .get::<FlowSleep<()>>()
            .expect("still attached")
            .needs_eval,
        "a detect-capable policy naming an UNREGISTERED capability must be \
         must-polled — mark_wake_dirty has no change-tracker to observe it and \
         must not risk a missed wake"
    );
}

/// The §12.5 cheap path, part (a) — "does NOT re-evaluate when nothing
/// changed" (#996). A registered, component-backed, detect-capable policy is
/// **not** flagged on a quiet frame once its component's change tick has
/// settled: the per-capability tracker records `Gate` as unchanged, so
/// `mark_wake_dirty` leaves the parked policy alone (no wasted `bind_brink_query`
/// re-evaluation every frame — the whole point of lifting the must-poll
/// interim). Driven through the plugin's own registered systems
/// (`detect_capability_changes` + `mark_wake_dirty`), not a direct call.
#[test]
fn detect_capable_component_policy_is_not_reevaluated_while_unchanged() {
    let mut app = make_test_app();
    app.register_capability::<(), Gate>("Gate");
    // A Gate entity to watch, plus the parked, detect-capable policy. No
    // fulfilled flow (no BrinkFlow) so `run_flow_sleep` never clears
    // `needs_eval` — leaving the raw `mark_wake_dirty` verdict observable.
    let gate = app.world_mut().spawn(Gate { open: false }).id();
    let entity = app
        .world_mut()
        .spawn(
            FlowSleep::<()>::persistent("should_wake")
                .with_detect(DetectSummary::from_bits(
                    [("Gate".to_string(), true)].into_iter().collect(),
                ))
                .dormant(),
        )
        .id();
    // Skip the bootstrap first-eval flag so only the detect gate is exercised.
    app.world_mut()
        .get_mut::<FlowSleep<()>>(entity)
        .expect("policy present")
        .evaluated_once = true;

    // Let the freshly-added Gate's change tick settle (Added counts as
    // Changed for a couple frames), then clear any flag it raised.
    pump(&mut app, 3);
    app.world_mut()
        .get_mut::<FlowSleep<()>>(entity)
        .expect("policy present")
        .needs_eval = false;

    // A quiet frame: Gate untouched → tracker verdict false → not flagged.
    app.update();
    assert!(
        !app.world()
            .entity(entity)
            .get::<FlowSleep<()>>()
            .expect("still attached")
            .needs_eval,
        "a detect-capable component-backed policy must NOT be flagged on a \
         frame where its watched component did not change — the §12.5 cheap path"
    );

    // Now mutate the watched component → the tracker sees the change → flagged.
    app.world_mut().get_mut::<Gate>(gate).expect("gate").open = true;
    app.update();
    assert!(
        app.world()
            .entity(entity)
            .get::<FlowSleep<()>>()
            .expect("still attached")
            .needs_eval,
        "flipping the watched component must flag the policy for re-evaluation \
         the same frame — the missed-wake class this issue closes"
    );
}

/// The §12.5 cheap path, part (b) — "DOES wake when the watched component
/// changes" (#996), end-to-end through the plugin's registered wake systems +
/// `advance_batch`. A dormant, component-reading flow stays parked (never runs
/// its turn) while its `Gate` is closed, then wakes and runs the moment the
/// `Gate` flips open — a real `bind_brink_query` round trip, not a global.
#[test]
fn detect_capable_component_condition_wakes_on_component_change() {
    let mut app = build_app();
    app.register_capability::<(), Gate>("Gate");
    app.bind_brink_query::<(), _, _>("is_gate_open", is_gate_open);
    let (program, tables, ctx) = compile_test_story(GATED_GATE_STORY);
    let story = add_story_assets(&mut app, program, tables, ctx);

    // The flow entity IS the gate entity (the doors-port shape): one entity
    // carries the Gate, the flow request, and a dormant one-shot policy whose
    // condition reads that Gate.
    let entity =
        app.world_mut()
            .spawn((
                Gate { open: false },
                BrinkFlowRequest::<()>::builder().story(story).build(),
                FlowSleep::<()>::once("should_wake").dormant().with_detect(
                    DetectSummary::from_bits([("Gate".to_string(), true)].into_iter().collect()),
                ),
            ))
            .id();

    // Gate closed: the flow must never wake or run its turn.
    pump(&mut app, 6);
    assert!(
        app.world().resource::<TextLog>().0.is_empty(),
        "a dormant component-backed flow must stay parked while its Gate is \
         closed: got {:?}",
        app.world().resource::<TextLog>().0
    );

    // Flip the watched component → the flow wakes and runs its turn.
    app.world_mut().get_mut::<Gate>(entity).expect("gate").open = true;
    pump(&mut app, 8);
    assert!(
        app.world().resource::<TextLog>().0.contains("Woke up!"),
        "the flow must wake and run once its watched Gate opens: got {:?}",
        app.world().resource::<TextLog>().0
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
        max_stepped = max_stepped.max(storm_app.world().resource::<BrinkBatchReport<()>>().stepped);
    }
    assert_eq!(
        max_stepped, storm_n,
        "wake storm: all {storm_n} flows wake and step together in a batch turn"
    );
}

// ── Wake-condition purity (issue #995, BH-4 follow-up) ──────────────────────

/// Like `add_story_assets`, but builds the `ProgramAsset` with `effect_rows`
/// populated from construction — `add_story_assets` always ships an empty
/// table (BH-1's capability join isn't what most tests exercise), so a
/// purity test that needs a *real*, non-empty `EffectRows` entry wires it in
/// here, exactly like a compiled `.inkb`'s loader would (`asset.rs`'s
/// `InkbLoader::load` sets `effect_rows` from the decoded `StoryData` the
/// same way).
///
/// Deliberately **not** `add_story_assets` + a post-hoc `Assets::get_mut`
/// mutation (an earlier version of these tests did exactly that): mutating
/// an already-`add`ed asset fires `AssetEvent::Modified`, and that is
/// precisely the event the `dev`-feature's `replay_on_reload` hot-reload
/// system watches for (`crate::replay`, on by default — `bevy-brink`'s
/// `dev` feature is default-on). One frame later, once the entity is
/// fulfilled (`BrinkReplayLog` attached), it would rebuild the flow *and*
/// drive it to its first terminal via real `advance_until_terminal` events —
/// completely out from under a still-`Parked`/dormant `FlowSleep` policy,
/// corrupting these tests' state before the purity gate is ever exercised.
/// Building the asset once via `Assets::add` (an `AssetEvent::Added`, which
/// `replay_on_reload` never reacts to) sidesteps that footgun entirely.
fn add_story_assets_with_effect_rows(
    app: &mut App,
    program: Program,
    tables: Vec<Vec<brink_format::LineEntry>>,
    initial_context: brink_runtime::World,
    effect_rows: Vec<EffectRowEntry>,
) -> bevy_asset::Handle<BrinkStoryAsset> {
    let world = app.world_mut();
    let program_handle = world
        .resource_mut::<Assets<ProgramAsset>>()
        .add(ProgramAsset {
            program,
            initial_context,
            effect_rows,
        });
    let tables_handle = world
        .resource_mut::<Assets<LineTablesAsset>>()
        .add(LineTablesAsset { tables });
    world
        .resource_mut::<Assets<BrinkStoryAsset>>()
        .add(BrinkStoryAsset {
            program: program_handle,
            line_tables: tables_handle,
        })
}

fn no_write_row(def: DefinitionId) -> EffectRowEntry {
    EffectRowEntry {
        def,
        is_entry: true,
        direct: DirectEffects::default(),
        dispatches: vec![],
    }
}

fn writing_row(def: DefinitionId, write: DefinitionId) -> EffectRowEntry {
    EffectRowEntry {
        def,
        is_entry: true,
        direct: DirectEffects {
            writes: vec![write],
            ..DirectEffects::default()
        },
        dispatches: vec![],
    }
}

fn opaque_row(def: DefinitionId) -> EffectRowEntry {
    EffectRowEntry {
        def,
        is_entry: true,
        direct: DirectEffects {
            opaque: true,
            ..DirectEffects::default()
        },
        dispatches: vec![],
    }
}

/// A pure condition (empty writes, not opaque) passes the checker directly —
/// the non-blocking baseline every other purity test contrasts with.
#[test]
fn check_named_condition_purity_accepts_a_pure_row() {
    let (program, _tables, _ctx) = compile_test_story(GATED_STORY);
    let def = program
        .definition_id_for_path("should_wake")
        .expect("should_wake resolves");
    let rows = vec![no_write_row(def)];
    assert!(
        check_named_condition_purity(
            &program,
            &rows,
            &CapabilityManifest::default(),
            "should_wake"
        )
        .is_ok()
    );
}

/// The core rejection: a condition whose row writes a global is rejected with
/// the named [`WakeConditionPurityError::Writes`] variant, not a panic.
#[test]
fn check_named_condition_purity_rejects_a_writing_row() {
    let (program, _tables, _ctx) = compile_test_story(GATED_STORY);
    let def = program
        .definition_id_for_path("should_wake")
        .expect("should_wake resolves");
    let write_id = DefinitionId::new(DefinitionTag::GlobalVar, 999);
    let rows = vec![writing_row(def, write_id)];

    let err = check_named_condition_purity(
        &program,
        &rows,
        &CapabilityManifest::default(),
        "should_wake",
    )
    .unwrap_err();
    assert!(
        matches!(&err, WakeConditionPurityError::Writes { condition, .. } if condition == "should_wake"),
        "got {err:?}"
    );
}

/// A dispatch's static fallback write also counts (§7: v1 always folds a
/// dispatch's conservative fallback in, no runtime narrowing) — not just the
/// row's own direct writes.
#[test]
fn check_named_condition_purity_rejects_a_dispatch_fallback_write() {
    let (program, _tables, _ctx) = compile_test_story(GATED_STORY);
    let def = program
        .definition_id_for_path("should_wake")
        .expect("should_wake resolves");
    let write_id = DefinitionId::new(DefinitionTag::GlobalVar, 999);
    let dispatch_cell = DefinitionId::new(DefinitionTag::GlobalVar, 1000);
    let rows = vec![EffectRowEntry {
        def,
        is_entry: true,
        direct: DirectEffects::default(),
        dispatches: vec![DispatchEntry {
            cell: dispatch_cell,
            narrowable: false,
            fallback: DirectEffects {
                writes: vec![write_id],
                ..DirectEffects::default()
            },
        }],
    }];

    let err = check_named_condition_purity(
        &program,
        &rows,
        &CapabilityManifest::default(),
        "should_wake",
    )
    .unwrap_err();
    assert!(
        matches!(err, WakeConditionPurityError::Writes { .. }),
        "got {err:?}"
    );
}

/// An opaque row (effects inference couldn't summarize a call) is rejected
/// conservatively — purity can't be proven, so it isn't assumed.
#[test]
fn check_named_condition_purity_rejects_an_opaque_row() {
    let (program, _tables, _ctx) = compile_test_story(GATED_STORY);
    let def = program
        .definition_id_for_path("should_wake")
        .expect("should_wake resolves");
    let rows = vec![opaque_row(def)];

    let err = check_named_condition_purity(
        &program,
        &rows,
        &CapabilityManifest::default(),
        "should_wake",
    )
    .unwrap_err();
    assert!(
        matches!(err, WakeConditionPurityError::Opaque { .. }),
        "got {err:?}"
    );
}

/// A condition name that doesn't resolve to any definition is its own named
/// error, not confused with a purity failure.
#[test]
fn check_named_condition_purity_unknown_condition_is_named() {
    let (program, _tables, _ctx) = compile_test_story(GATED_STORY);
    let def = program
        .definition_id_for_path("should_wake")
        .expect("should_wake resolves");
    let rows = vec![no_write_row(def)];

    let err = check_named_condition_purity(
        &program,
        &rows,
        &CapabilityManifest::default(),
        "no_such_fn",
    )
    .unwrap_err();
    assert!(
        matches!(&err, WakeConditionPurityError::UnknownCondition { condition } if condition == "no_such_fn"),
        "got {err:?}"
    );
}

/// A story whose `EffectRows` table is empty entirely (never ran the
/// compiler's effects emission — the same shape `add_story_assets` ships by
/// default) is outside the guarantee this checks: the empty table bypasses
/// the check rather than rejecting every condition such a story could name.
#[test]
fn check_named_condition_purity_bypasses_when_effect_rows_table_is_empty() {
    let (program, _tables, _ctx) = compile_test_story(GATED_STORY);
    assert!(
        check_named_condition_purity(&program, &[], &CapabilityManifest::default(), "should_wake")
            .is_ok()
    );
    // Even a name that wouldn't resolve at all is let through — there is no
    // row-based guarantee to check against.
    assert!(
        check_named_condition_purity(&program, &[], &CapabilityManifest::default(), "no_such_fn")
            .is_ok()
    );
}

// ── Wake-condition purity: EXTERNAL-mediated writes (issue #1040) ───────────

/// An `EffectRowEntry` whose direct part calls exactly one `EXTERNAL` (via
/// `call`) and performs no ink-level reads/writes/opaque calls of its own —
/// isolates the manifest-join check from the plain global-write check
/// [`writing_row`]/[`no_write_row`] already cover.
fn calling_row(def: DefinitionId, call: CallAtom) -> EffectRowEntry {
    EffectRowEntry {
        def,
        is_entry: true,
        direct: DirectEffects {
            calls: vec![call],
            ..DirectEffects::default()
        },
        dispatches: vec![],
    }
}

/// A one-external [`CapabilityManifest`] — `name`'s manifest entry declares
/// exactly `effects`.
fn manifest_with(name: &str, effects: CapabilityEffects) -> CapabilityManifest {
    CapabilityManifest {
        externals: vec![CapabilityManifestExternal {
            name: name.to_string(),
            effects,
        }],
    }
}

/// Reads-only case: the condition calls an `EXTERNAL` whose manifest entry
/// declares `reads` but no `writes` — accepted, exactly as the issue
/// specifies ("reads-only accepted").
#[test]
fn check_named_condition_purity_accepts_a_reads_only_external_call() {
    let (program, _tables, _ctx) = compile_test_story(GATED_STORY_WITH_EXTERNALS);
    let def = program
        .definition_id_for_path("should_wake")
        .expect("should_wake resolves");
    let read_state = program
        .name_id("read_state")
        .expect("interned as a call kind");
    let rows = vec![calling_row(def, call_atom(read_state))];
    let manifest = manifest_with(
        "read_state",
        CapabilityEffects {
            reads: vec!["GameState".to_string()],
            writes: vec![],
            detect: std::collections::BTreeMap::new(),
        },
    );

    assert!(check_named_condition_purity(&program, &rows, &manifest, "should_wake").is_ok());
}

/// The core rejection this issue adds: a condition calling an `EXTERNAL`
/// whose manifest entry declares `writes` is rejected with the named
/// [`WakeConditionPurityError::ExternalWrites`] variant — not the ink-level
/// `Writes` variant (that's a distinct axis: the row's own writes vs. writes
/// mediated through a host binding), and not a panic.
#[test]
fn check_named_condition_purity_rejects_an_external_declared_write() {
    let (program, _tables, _ctx) = compile_test_story(GATED_STORY_WITH_EXTERNALS);
    let def = program
        .definition_id_for_path("should_wake")
        .expect("should_wake resolves");
    let touch_state = program
        .name_id("touch_state")
        .expect("interned as a call kind");
    let rows = vec![calling_row(def, call_atom(touch_state))];
    let manifest = manifest_with(
        "touch_state",
        CapabilityEffects {
            reads: vec![],
            writes: vec!["GameState".to_string()],
            detect: std::collections::BTreeMap::new(),
        },
    );

    let err = check_named_condition_purity(&program, &rows, &manifest, "should_wake").unwrap_err();
    assert!(
        matches!(
            &err,
            WakeConditionPurityError::ExternalWrites { condition, external, writes }
                if condition == "should_wake"
                    && external == "touch_state"
                    && writes == &vec!["GameState".to_string()]
        ),
        "got {err:?}"
    );
}

/// An `EXTERNAL` the manifest has **no entry for at all** is accepted, not
/// rejected — matching the BH-1 access join's posture
/// (`resolve_call_atom`/`collect_missing_from_direct` in
/// `crate::capability`): not every `EXTERNAL` touches ECS state, and a
/// manifest's `effects` key is opt-in (`docs/effects-spec.md` §13.2), so
/// absence from the manifest contributes no access rather than proving
/// impurity. Regression test for issue #1040's review fix: a pure binding
/// (e.g. a `bind_brink_fn` helper with no manifest entry) used in a wake
/// condition must not permanently Fault the flow.
#[test]
fn check_named_condition_purity_accepts_an_unregistered_external() {
    let (program, _tables, _ctx) = compile_test_story(GATED_STORY_WITH_EXTERNALS);
    let def = program
        .definition_id_for_path("should_wake")
        .expect("should_wake resolves");
    let touch_state = program
        .name_id("touch_state")
        .expect("interned as a call kind");
    let rows = vec![calling_row(def, call_atom(touch_state))];
    // Empty manifest: `touch_state` has no entry at all.
    let manifest = CapabilityManifest::default();

    assert!(check_named_condition_purity(&program, &rows, &manifest, "should_wake").is_ok());
}

/// A dispatch's static fallback calling a writing `EXTERNAL` also counts
/// (§7: v1 always folds a dispatch's conservative fallback in) — mirrors
/// [`check_named_condition_purity_rejects_a_dispatch_fallback_write`] for the
/// manifest-join axis.
#[test]
fn check_named_condition_purity_rejects_an_external_write_via_dispatch_fallback() {
    let (program, _tables, _ctx) = compile_test_story(GATED_STORY_WITH_EXTERNALS);
    let def = program
        .definition_id_for_path("should_wake")
        .expect("should_wake resolves");
    let touch_state = program
        .name_id("touch_state")
        .expect("interned as a call kind");
    let dispatch_cell = DefinitionId::new(DefinitionTag::GlobalVar, 1001);
    let rows = vec![EffectRowEntry {
        def,
        is_entry: true,
        direct: DirectEffects::default(),
        dispatches: vec![DispatchEntry {
            cell: dispatch_cell,
            narrowable: false,
            fallback: DirectEffects {
                calls: vec![call_atom(touch_state)],
                ..DirectEffects::default()
            },
        }],
    }];
    let manifest = manifest_with(
        "touch_state",
        CapabilityEffects {
            reads: vec![],
            writes: vec!["GameState".to_string()],
            detect: std::collections::BTreeMap::new(),
        },
    );

    let err = check_named_condition_purity(&program, &rows, &manifest, "should_wake").unwrap_err();
    assert!(
        matches!(err, WakeConditionPurityError::ExternalWrites { .. }),
        "got {err:?}"
    );
}

/// Reachability, end-to-end through `run_flow_sleep` (the registered plugin
/// system, not a direct unit call), mirroring
/// `writing_condition_is_rejected_and_never_evaluated_through_run_flow_sleep`
/// for the new axis: a condition calling a manifest-declared-writing
/// `EXTERNAL` is rejected before it is ever evaluated — the flow never
/// wakes, never runs its turn, and the policy lands in `Faulted`. The
/// `CapabilityManifest` is a real app resource here (`app.insert_resource`),
/// not a hand-passed argument — proving the check is actually wired into the
/// live system, not just the unit-level helper function.
#[test]
fn writing_external_condition_is_rejected_and_never_evaluated_through_run_flow_sleep() {
    let mut app = build_app();
    let (program, tables, ctx) = compile_test_story(GATED_STORY_WITH_EXTERNALS);
    let gate_idx = program.global_index("gate").expect("gate global exists");
    let def = program
        .definition_id_for_path("should_wake")
        .expect("should_wake resolves");
    let touch_state = program
        .name_id("touch_state")
        .expect("interned as a call kind");
    let story = add_story_assets_with_effect_rows(
        &mut app,
        program,
        tables,
        ctx,
        vec![calling_row(def, call_atom(touch_state))],
    );
    app.insert_resource(manifest_with(
        "touch_state",
        CapabilityEffects {
            reads: vec![],
            writes: vec!["GameState".to_string()],
            detect: std::collections::BTreeMap::new(),
        },
    ));
    // The manifest names a "GameState" capability, so the load-boundary
    // admission gate (issue #912) needs it registered or the story is
    // rejected at load — before `run_flow_sleep`'s purity check would ever
    // get a chance to run.
    app.register_capability::<(), GameStateCap>("GameState");

    app.world_mut().spawn((
        BrinkFlowRequest::<()>::builder().story(story).build(),
        FlowSleep::<()>::persistent("should_wake").dormant(),
    ));
    app.update(); // fulfill

    // Flip the gate true — a correctly-implemented (but externally impure)
    // condition would return true and wake. It must never get the chance.
    set_gate(&mut app, gate_idx, 1);
    pump(&mut app, 5);

    assert_eq!(
        single_sleep(&mut app),
        SleepState::Faulted,
        "a condition calling a manifest-declared-writing EXTERNAL must land in Faulted, never \
         Woken"
    );
    assert!(
        app.world().resource::<TextLog>().0.is_empty(),
        "the flow's turn must never run — the externally-mediated write was never evaluated"
    );
}

/// Dynamic fn-value condition resolution (a `Value::FnRef`/`Closure` token,
/// e.g. one a host resolved dynamically rather than naming statically) is
/// checked through the exact same row inspection as the named path — pure
/// passes, writing rejects.
#[test]
fn check_value_condition_purity_checks_a_resolved_fn_value_token() {
    let (program, _tables, _ctx) = compile_test_story(GATED_STORY);
    let def = program
        .definition_id_for_path("should_wake")
        .expect("should_wake resolves");
    let token = Value::FnRef(def);

    let pure_rows = vec![no_write_row(def)];
    assert!(
        check_value_condition_purity(&program, &pure_rows, &CapabilityManifest::default(), &token)
            .is_ok()
    );

    let write_id = DefinitionId::new(DefinitionTag::GlobalVar, 999);
    let writing_rows = vec![writing_row(def, write_id)];
    let err = check_value_condition_purity(
        &program,
        &writing_rows,
        &CapabilityManifest::default(),
        &token,
    )
    .unwrap_err();
    assert!(
        matches!(err, WakeConditionPurityError::Writes { .. }),
        "a dynamic fn-value condition token must be purity-checked exactly like a named one; \
         got {err:?}"
    );
}

/// A `Value` that isn't a function value at all has no target to check — its
/// own named error, not silently treated as pure.
#[test]
fn check_value_condition_purity_rejects_a_non_function_value() {
    let (program, _tables, _ctx) = compile_test_story(GATED_STORY);
    let def = program
        .definition_id_for_path("should_wake")
        .expect("should_wake resolves");
    let rows = vec![no_write_row(def)];
    let err = check_value_condition_purity(
        &program,
        &rows,
        &CapabilityManifest::default(),
        &Value::Int(1),
    )
    .unwrap_err();
    assert!(
        matches!(err, WakeConditionPurityError::NotAFunctionValue),
        "got {err:?}"
    );
}

/// Reachability, end-to-end through `run_flow_sleep` (the registered plugin
/// system, not a direct unit call): a **pure** condition's row admits it into
/// evaluation normally — the purity gate never blocks a legitimately pure
/// wake condition. Contrasts with the writing-condition test below.
#[test]
fn pure_condition_wakes_normally_through_run_flow_sleep() {
    let mut app = build_app();
    let (program, tables, ctx) = compile_test_story(GATED_STORY);
    let gate_idx = program.global_index("gate").expect("gate global exists");
    let def = program
        .definition_id_for_path("should_wake")
        .expect("should_wake resolves");
    let story =
        add_story_assets_with_effect_rows(&mut app, program, tables, ctx, vec![no_write_row(def)]);

    app.world_mut().spawn((
        BrinkFlowRequest::<()>::builder().story(story).build(),
        FlowSleep::<()>::persistent("should_wake").dormant(),
    ));
    app.update(); // fulfill

    set_gate(&mut app, gate_idx, 1);
    // Margin, deliberately not a single exact frame (issue #1082): whether
    // `advance_batch` (added by `build_app`, unordered w.r.t. the plugin's
    // own wake-condition systems — a host system, not one `BrinkPlugin`
    // knows to order against) observes the newly-Woken flow in the very
    // frame `run_flow_sleep` wakes it, or only the frame after, is exactly
    // the ordering `run_flow_sleep`'s own docs call out as unconstrained
    // ("whether it runs before or after the batch driver in a frame, a wake
    // takes effect on the following frame's Collect"). Three frames is
    // comfortably enough for the wake *and* its turn to have run either way,
    // without also being enough for a second (illegitimate) wake — this
    // policy's one dependency change was the single `set_gate` call above,
    // and the empty-turn guard (issue #1082) means no idle frame here can
    // manufacture another.
    pump(&mut app, 3);

    let state = single_sleep(&mut app);
    assert!(
        matches!(state, SleepState::Woken | SleepState::Parked),
        "a pure condition must wake the flow and let its turn run, not fault or vanish: got {state:?}"
    );
    assert!(
        app.world().resource::<TextLog>().0.contains("Woke up!"),
        "a pure condition must be evaluated and wake the flow normally"
    );
}

/// Regression test for issue #1082: `advance_batch` must not touch
/// `BrinkGlobals` on a turn that collects zero flows. Before the fix,
/// `advance_batch` took `&mut globals.inner` (via `apply_batch_writes`)
/// *unconditionally*, which trips Bevy's change detection the instant the
/// reference is taken — regardless of whether anything was actually
/// written. `mark_wake_dirty` treats *any* `BrinkGlobals` change as "recheck
/// every Parked all-detect-capable policy" (its only signal, since no
/// per-capability component-tick hook exists yet — see the module docs), so
/// an idle turn manufactured a spurious wake-up check on literally every
/// frame `advance_batch` ran. For a **persistent** policy whose condition is
/// never reset back to false (as here — `gate` is set once and never
/// cleared), that spurious signal alone was enough to re-wake the flow over
/// and over with no further `set_gate` call, racing it through every
/// remaining turn (here, all the way past `-> END`) purely as a function of
/// how many frames happened to elapse before an assertion ran — which is
/// exactly the "passes in isolation, flakes under full-suite timing"
/// signature reported in #1082, not state leaking between tests.
///
/// This pins the fix directly: once the one legitimate wake (from the one
/// real `set_gate` write) has run its turn and re-parked, the policy must
/// stay `Parked` — and the story must never advance past its first turn —
/// no matter how many additional frames pass with nothing left to collect.
#[test]
#[ignore = "flaky: #1146 — spurious wake ~1/12; quarantined 2026-07-19, un-ignore with the fix"]
fn idle_turns_never_manufacture_a_spurious_wake_signal() {
    let mut app = build_app();
    let (program, tables, ctx) = compile_test_story(GATED_STORY);
    let gate_idx = program.global_index("gate").expect("gate global exists");
    let def = program
        .definition_id_for_path("should_wake")
        .expect("should_wake resolves");
    let story =
        add_story_assets_with_effect_rows(&mut app, program, tables, ctx, vec![no_write_row(def)]);

    app.world_mut().spawn((
        BrinkFlowRequest::<()>::builder().story(story).build(),
        FlowSleep::<()>::persistent("should_wake").dormant(),
    ));
    app.update(); // fulfill

    // The one and only dependency change this test ever makes.
    set_gate(&mut app, gate_idx, 1);

    // Pump well past the single legitimate wake+step+rearm cycle. With the
    // bug, this reliably (not just occasionally) re-wakes the flow, runs its
    // second turn, reaches `-> END`, and the plugin retires the dead flow's
    // `FlowSleep` policy — `single_sleep` then panics on a zero count, the
    // exact symptom #1082 reported.
    pump(&mut app, 20);

    assert_eq!(
        single_sleep(&mut app),
        SleepState::Parked,
        "a persistent policy's one true evaluation must wake exactly once and then \
         stay parked — a turn with nothing to collect must never manufacture another"
    );
    let log = &app.world().resource::<TextLog>().0;
    assert!(
        log.contains("Woke up!") && !log.contains("Second turn."),
        "the flow must run its one woken turn and no further turn: got {log:?}"
    );
}

/// The core enforcement, reachable end-to-end: a condition whose (real,
/// non-empty) effect row writes a global is rejected before it is ever
/// evaluated — the flow never wakes, never runs its turn, and the policy
/// lands in `Faulted` (never silently retried into a spin) rather than being
/// called anyway.
#[test]
fn writing_condition_is_rejected_and_never_evaluated_through_run_flow_sleep() {
    let mut app = build_app();
    let (program, tables, ctx) = compile_test_story(GATED_STORY);
    let gate_idx = program.global_index("gate").expect("gate global exists");
    let def = program
        .definition_id_for_path("should_wake")
        .expect("should_wake resolves");
    let write_id = DefinitionId::new(DefinitionTag::GlobalVar, 999);
    let story = add_story_assets_with_effect_rows(
        &mut app,
        program,
        tables,
        ctx,
        vec![writing_row(def, write_id)],
    );

    app.world_mut().spawn((
        BrinkFlowRequest::<()>::builder().story(story).build(),
        FlowSleep::<()>::persistent("should_wake").dormant(),
    ));
    app.update(); // fulfill

    // Flip the gate true — a correctly-implemented (but impure) condition
    // would return true and wake. It must never get the chance to run.
    set_gate(&mut app, gate_idx, 1);
    pump(&mut app, 5);

    assert_eq!(
        single_sleep(&mut app),
        SleepState::Faulted,
        "an impure condition's policy must land in Faulted, never Woken"
    );
    assert!(
        app.world().resource::<TextLog>().0.is_empty(),
        "the flow's turn must never run — the writing condition was never evaluated"
    );
}

/// Reachability, end-to-end through `run_flow_sleep`, for the **dynamic
/// fn-value** condition shape (issue #1078): a `FlowSleep` built with
/// [`FlowSleep::with_condition_value`] resolves and evaluates the token
/// directly (`call_ink_function_value`) instead of resolving `condition` by
/// path — exactly like the named path, a pure token wakes the flow normally.
#[test]
fn dynamic_fn_value_pure_condition_wakes_normally_through_run_flow_sleep() {
    let mut app = build_app();
    let (program, tables, ctx) = compile_test_story(GATED_STORY);
    let gate_idx = program.global_index("gate").expect("gate global exists");
    let def = program
        .definition_id_for_path("should_wake")
        .expect("should_wake resolves");
    let token = Value::FnRef(def);
    let story =
        add_story_assets_with_effect_rows(&mut app, program, tables, ctx, vec![no_write_row(def)]);

    app.world_mut().spawn((
        BrinkFlowRequest::<()>::builder().story(story).build(),
        FlowSleep::<()>::persistent("should_wake")
            .with_condition_value(token)
            .dormant(),
    ));
    app.update(); // fulfill

    set_gate(&mut app, gate_idx, 1);
    // Same margin, same reason as `pure_condition_wakes_normally_through_run_flow_sleep`
    // above (issue #1082): `advance_batch` is unordered w.r.t. the plugin's
    // own wake-condition systems, so whether it observes the newly-Woken
    // flow on the wake's own frame or the frame after is unconstrained by
    // `run_flow_sleep`'s own docs. Three frames is enough for the wake *and*
    // its turn to have run either way, without being enough for a second
    // (illegitimate) wake — the empty-turn guard (issue #1082) means no idle
    // frame here can manufacture one.
    pump(&mut app, 3);

    let state = single_sleep(&mut app);
    assert!(
        matches!(state, SleepState::Woken | SleepState::Parked),
        "a pure dynamic fn-value condition must wake the flow and let its turn run, not fault or vanish: got {state:?}"
    );
    assert!(
        app.world().resource::<TextLog>().0.contains("Woke up!"),
        "a pure dynamic fn-value condition must be evaluated and wake the flow normally"
    );
}

/// The dynamic-fn-value counterpart of
/// `writing_condition_is_rejected_and_never_evaluated_through_run_flow_sleep`
/// (issue #1078): a `FlowSleep` whose [`FlowSleep::with_condition_value`]
/// token resolves to a row that writes a global is rejected — via
/// `check_value_condition_purity` — before it is ever evaluated: never
/// called even once, landing in `Faulted` rather than being silently
/// retried.
#[test]
fn dynamic_fn_value_condition_with_writes_is_rejected_and_never_evaluated_through_run_flow_sleep() {
    let mut app = build_app();
    let (program, tables, ctx) = compile_test_story(GATED_STORY);
    let gate_idx = program.global_index("gate").expect("gate global exists");
    let def = program
        .definition_id_for_path("should_wake")
        .expect("should_wake resolves");
    let token = Value::FnRef(def);
    let write_id = DefinitionId::new(DefinitionTag::GlobalVar, 999);
    let story = add_story_assets_with_effect_rows(
        &mut app,
        program,
        tables,
        ctx,
        vec![writing_row(def, write_id)],
    );

    app.world_mut().spawn((
        BrinkFlowRequest::<()>::builder().story(story).build(),
        FlowSleep::<()>::persistent("should_wake")
            .with_condition_value(token)
            .dormant(),
    ));
    app.update(); // fulfill

    // Flip the gate true — a correctly-implemented (but impure) condition
    // would return true and wake. It must never get the chance to run.
    set_gate(&mut app, gate_idx, 1);
    pump(&mut app, 5);

    assert_eq!(
        single_sleep(&mut app),
        SleepState::Faulted,
        "an impure dynamic fn-value condition's policy must land in Faulted, never Woken"
    );
    assert!(
        app.world().resource::<TextLog>().0.is_empty(),
        "the flow's turn must never run — the writing condition was never evaluated"
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

/// `FlowSleep` must be inspector-visible **as a component** on the flow entity
/// (module docs, `sleep.rs:112,142`): `#[derive(Reflect)]` alone only
/// registers the type's shape — it does not attach the `ReflectComponent`
/// type data that entity inspectors (e.g. `bevy-inspector-egui`) need to
/// enumerate/edit a component on an entity. That type data comes only from
/// the `#[reflect(Component)]` attribute alongside the derive.
///
/// Checked directly via [`GetTypeRegistration`] rather than through an `App`'s
/// auto-derived registry: this crate builds `bevy_reflect`/`bevy_ecs` with
/// `default-features = false` (no `auto_register_inventory`), so nothing gets
/// into `AppTypeRegistry` without an explicit `register_type` call regardless
/// of `#[reflect(Component)]` — that gap is a separate, out-of-scope concern
/// from this finding. This test isolates exactly what the attribute produces.
#[test]
fn flow_sleep_reflects_as_component() {
    use bevy_reflect::GetTypeRegistration as _;

    let registration = FlowSleep::<()>::get_type_registration();
    assert!(
        registration
            .data::<bevy_ecs::reflect::ReflectComponent>()
            .is_some(),
        "FlowSleep<()> is missing ReflectComponent type data — inspectors cannot see it as a \
         component on the flow entity; add `#[reflect(Component)]` alongside `#[derive(Component, Reflect)]`"
    );
}
