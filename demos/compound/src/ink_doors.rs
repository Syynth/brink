//! Phase 1b: doors/switches, driven by **ink** instead of Rust —
//! `docs/drive-app-plan.md` §9's second entity migration (README "Suggested
//! Phase-1 migration order" #2). `src/doors.rs` stays untouched as the Rust
//! baseline (only a `pub(crate)` visibility bump on its accent-color helpers
//! so this module can reuse them instead of forking a second copy); this
//! module is the side-by-side ink port selected at launch with
//! `--doors-impl ink`.
//!
//! Unlike the alarm (Phase 1a, `call_ink_function` driven every frame), a
//! door is **reactive**: it does nothing until its switch flips. That is
//! exactly the BH-4 wake surface's job (`FlowSleep`/`wake_when`,
//! `docs/effects-spec.md` §13.1) — the host attaches a standing wake policy
//! and the plugin's own systems (`mark_wake_dirty`, `run_flow_sleep`) do the
//! rest. This is the **first real consumer of BH-4 outside its own test
//! suite**, and the `wake_when` authoring ergonomics it surfaces are this
//! port's whole point (see `MIGRATION.md`).
//!
//! ## The seams
//!
//! * **The flow entity IS the door entity.** [`attach_ink_door_flows`]
//!   attaches a [`BrinkFlowRequest<DoorsStory>`] + a dormant, one-shot
//!   [`FlowSleep<DoorsStory>`] straight onto the same entity
//!   `doors::spawn_doors_from_layout` already spawned (sprite, `Collider`,
//!   `Door`, `RoundScoped` — none of that changes). No id-mirroring plumbing
//!   is needed: [`is_switch_on`] reads the `Door` component directly off the
//!   calling flow entity.
//! * **Read seam — world-access binding, not a global mirror.** Where the
//!   alarm's write seam pushes engine events INTO ink state every frame,
//!   here the direction is reversed: `should_open` (`assets/doors.ink`)
//!   calls the `is_switch_on` `bind_brink_query` binding, which reads the
//!   live `Switch` component straight out of the ECS at evaluation time. No
//!   per-frame mirror system is needed at all — a genuine ergonomics win
//!   over Phase 1a's `call_ink_function`-every-frame shape.
//! * **Detect verdict — must-poll, documented.** `should_open`'s dependency
//!   is a `Switch` component, not a `BrinkGlobals` variable, so
//!   `mark_wake_dirty` has no change-tick hook on it
//!   (`docs/effects-spec.md` §12.5 is not wired — see [`switch_detect_summary`]
//!   and the friction entry in `MIGRATION.md`). This is the exact
//!   `is_player_nearby`-reading-`Transform` case the `sleep` module's own
//!   docs anticipate, now hit by a real consumer instead of a unit test.
//! * **Open is a PERMANENT signal (`WakeArming::Once`).** A door's flow runs
//!   its one turn and parks for good at `-> END`;
//!   [`ink_door_sync_system`] treats `StoryStatus::Ended` as "open". This is
//!   a deliberate simplification versus the Rust baseline
//!   (`doors::door_sync_system`), which is fully bidirectional — it
//!   RE-LOCKS a door if its switch is flipped back off. See the divergence
//!   test in this module and `MIGRATION.md` for why modeling the reversible
//!   case would need per-flow `Local`-scoped ink state
//!   (`docs/scoped-flow-state-spec.md`), out of scope for this minimal port.

use std::collections::BTreeMap;
use std::time::Instant;

use bevy::prelude::*;
use bevy_brink::runtime::StoryStatus;
use bevy_brink::{
    BrinkFlow, BrinkFlowRequest, BrinkQueryInput, BrinkStoryAsset, DetectSummary, FlowSleep, Value,
};

use crate::doors::{Door, Switch, accent_color};
use crate::timing::BehaviorTimings;

/// Story marker for the doors ink instance. Every locked door in a round is
/// its own flow under this one marker (§9's "many instances, one program"
/// shape) — see the module docs for why that's safe (no shared per-door
/// state rides `BrinkGlobals<DoorsStory>`; `doors.ink` declares no `VAR`s).
pub struct DoorsStory;

/// Which implementation drives doors/switches this run, chosen at launch.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DoorsImpl {
    /// The Rust baseline (`src/doors.rs`, `door_sync_system`).
    #[default]
    Rust,
    /// The ink port (`assets/doors.ink`, this module).
    Ink,
}

impl DoorsImpl {
    /// Parse `--doors-impl rust|ink` from the process args (default: Rust).
    #[must_use]
    pub fn from_args() -> Self {
        let mut args = std::env::args();
        while let Some(arg) = args.next() {
            let value = if arg == "--doors-impl" {
                args.next()
            } else {
                arg.strip_prefix("--doors-impl=").map(str::to_owned)
            };
            if let Some(value) = value {
                return match value.as_str() {
                    "ink" => Self::Ink,
                    _ => Self::Rust,
                };
            }
        }
        Self::Rust
    }

    /// Human-readable label for the HUD.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Ink => "ink",
        }
    }
}

/// Run condition: the Rust door writer (`door_sync_system`) is active.
pub fn doors_is_rust(mode: Res<DoorsImpl>) -> bool {
    *mode == DoorsImpl::Rust
}

/// Run condition: the ink door writer ([`ink_door_sync_system`]) is active.
pub fn doors_is_ink(mode: Res<DoorsImpl>) -> bool {
    *mode == DoorsImpl::Ink
}

/// The `should_open` condition's dependency verdict (`#913`/§13.1): it reads
/// a `Switch` component through [`is_switch_on`], not a `BrinkGlobals`
/// variable, so `mark_wake_dirty` has no change-tick hook on it. A
/// non-empty `bits` map forces the must-poll path regardless of the bool
/// value — `true` here only documents that the dependency *would* be
/// change-detection-capable if `docs/effects-spec.md` §12.5's component-tick
/// wiring existed.
fn switch_detect_summary() -> DetectSummary {
    DetectSummary::from_bits(BTreeMap::from([("Switch".to_string(), true)]))
}

/// Ink mode only (`--doors-impl ink`): attach a dormant, one-shot ink flow to
/// every freshly spawned locked-door entity. `doors::spawn_doors_from_layout`
/// is unmodified and still runs for BOTH implementations (it also spawns the
/// `Switch` entities, untouched by this port); this system only decorates
/// the `Door` entities it produces.
///
/// Runs every `Update` frame, not just `Startup`: a fresh layout (and fresh
/// `Door` entities) is spawned on every `StartRound`
/// (`rounds::start_round_system`), not only the game's first round.
pub fn attach_ink_door_flows(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    doors: Query<Entity, Added<Door>>,
) {
    for entity in &doors {
        let story: Handle<BrinkStoryAsset> = asset_server.load("doors.ink");
        commands.entity(entity).insert((
            BrinkFlowRequest::<DoorsStory>::builder()
                .story(story)
                .build(),
            FlowSleep::<DoorsStory>::once("should_open")
                .dormant()
                .with_detect(switch_detect_summary()),
        ));
    }
}

/// World-access binding (`bind_brink_query`): is the switch matching this
/// door's `switch_id` currently on? Reads the `Door` component straight off
/// the CALLING flow entity — the flow entity IS the door entity in this
/// port (see [`attach_ink_door_flows`]) — so no id argument needs to cross
/// the ink/engine boundary at all.
pub fn is_switch_on(
    In((flow, _args)): In<BrinkQueryInput>,
    doors: Query<&Door>,
    switches: Query<&Switch>,
) -> Value {
    let Ok(door) = doors.get(flow) else {
        return Value::Bool(false);
    };
    Value::Bool(switches.iter().any(|sw| sw.id == door.switch_id && sw.on))
}

/// The ink counterpart of `doors::door_sync_system`: mirrors each ink-mode
/// door's [`BrinkFlow`] status into its [`Door::open`] + sprite color, at the
/// same nanosecond-timed [`BehaviorTimings::doors`] slot the Rust baseline
/// fills, so the HUD's `doors` line reports whichever impl is live.
///
/// Reaching `StoryStatus::Ended` is a PERMANENT "open" signal
/// (`WakeArming::Once`) — see the module docs for why this diverges from the
/// Rust baseline's bidirectional (re-lockable) behavior.
pub fn ink_door_sync_system(
    mut doors: Query<(&mut Door, &mut Sprite, &BrinkFlow<DoorsStory>)>,
    mut timings: ResMut<BehaviorTimings>,
) {
    let start = Instant::now();

    for (mut door, mut sprite, flow) in &mut doors {
        let open = flow.inner.status() == StoryStatus::Ended;
        door.open = open;
        sprite.color = if open {
            let mut c = accent_color(door.switch_id);
            c.set_alpha(0.18);
            c
        } else {
            Color::srgb(0.55, 0.18, 0.18)
        };
    }

    timings.doors = start.elapsed();
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::AssetPlugin;
    use bevy_brink::{BrinkBindingsAppExt, BrinkPlugin, advance_batch, call_ink_function};

    /// Compile `assets/doors.ink` inline (deterministic, no async
    /// `AssetServer`) and insert the story assets, via
    /// [`bevy_brink::compile_story_inline`] (#1060, landed after this port
    /// started — see `MIGRATION.md`'s G3 entry) instead of hand-rolling the
    /// compile→link→context→insert dance; matches
    /// `ink_alarm::tests::build_alarm_story`'s post-#1060 shape and drops
    /// this demo's now-unnecessary direct `brink-compiler` dev-dependency.
    fn build_doors_story(app: &mut App) -> Handle<BrinkStoryAsset> {
        let src = include_str!("../assets/doors.ink");
        bevy_brink::compile_story_inline(app, "doors.ink", src).expect("doors.ink compiles")
    }

    fn build_app() -> App {
        let mut app = App::new();
        app.add_plugins((AssetPlugin::default(), BrinkPlugin::<DoorsStory>::default()));
        app.bind_brink_query::<DoorsStory, _, _>("is_switch_on", is_switch_on);
        app.init_resource::<BehaviorTimings>();
        app.add_systems(Update, (advance_batch::<DoorsStory>, ink_door_sync_system));
        app
    }

    fn spawn_switch(app: &mut App, id: u8, on: bool) -> Entity {
        app.world_mut().spawn(Switch { id, on }).id()
    }

    /// Spawn a door entity exactly the way [`attach_ink_door_flows`] decorates
    /// a real `spawn_doors_from_layout` door: `Door` + `Sprite` (the read
    /// seam requires one, matching `door_sync_system`'s own shape) + the
    /// dormant, one-shot flow.
    fn spawn_door(app: &mut App, story: &Handle<BrinkStoryAsset>, switch_id: u8) -> Entity {
        app.world_mut()
            .spawn((
                Door {
                    switch_id,
                    open: false,
                },
                Sprite::from_color(Color::srgb(0.55, 0.18, 0.18), Vec2::new(10.0, 10.0)),
                BrinkFlowRequest::<DoorsStory>::builder()
                    .story(story.clone())
                    .build(),
                FlowSleep::<DoorsStory>::once("should_open")
                    .dormant()
                    .with_detect(switch_detect_summary()),
            ))
            .id()
    }

    fn pump(app: &mut App, frames: usize) {
        for _ in 0..frames {
            app.update();
        }
    }

    /// Reachability + the core contract: a door stays locked while its
    /// switch is off (dormant — never steps), then opens once the switch
    /// flips on, and stays open.
    #[test]
    fn ink_door_opens_when_switch_flips_and_stays_open() {
        let mut app = build_app();
        let story = build_doors_story(&mut app);
        let switch = spawn_switch(&mut app, 0, false);
        let door = spawn_door(&mut app, &story, 0);

        pump(&mut app, 6);
        assert!(
            !app.world().get::<Door>(door).unwrap().open,
            "must stay locked while the switch is off"
        );

        app.world_mut().get_mut::<Switch>(switch).unwrap().on = true;
        pump(&mut app, 8);
        assert!(
            app.world().get::<Door>(door).unwrap().open,
            "must open once its switch flips on"
        );

        pump(&mut app, 4);
        assert!(
            app.world().get::<Door>(door).unwrap().open,
            "stays open on later frames"
        );
    }

    /// Two doors sharing one switch id both open together; an unrelated
    /// switch never opens a door watching a different id
    /// (`doors::Switch` docs: "opens every door sharing its `id`").
    #[test]
    fn ink_doors_sharing_a_switch_open_together_others_stay_locked() {
        let mut app = build_app();
        let story = build_doors_story(&mut app);
        let switch = spawn_switch(&mut app, 3, false);
        let door_a = spawn_door(&mut app, &story, 3);
        let door_b = spawn_door(&mut app, &story, 3);
        let _other_switch = spawn_switch(&mut app, 4, false);
        let other_door = spawn_door(&mut app, &story, 4);

        pump(&mut app, 6);
        app.world_mut().get_mut::<Switch>(switch).unwrap().on = true;
        pump(&mut app, 8);

        assert!(app.world().get::<Door>(door_a).unwrap().open);
        assert!(app.world().get::<Door>(door_b).unwrap().open);
        assert!(
            !app.world().get::<Door>(other_door).unwrap().open,
            "a door watching a different switch id must not open"
        );
    }

    /// Frame-by-frame parity vs the Rust baseline for the common (monotonic)
    /// case: while the switch has never been flipped true, both
    /// implementations agree the door is closed every single frame.
    #[test]
    fn ink_door_matches_rust_baseline_before_the_first_flip() {
        let mut app = build_app();
        let story = build_doors_story(&mut app);
        let switch = spawn_switch(&mut app, 2, false);
        let door = spawn_door(&mut app, &story, 2);

        for i in 0..10 {
            app.update();
            let rust_open = false; // door_sync_system's predicate: switch never on
            let ink_open = app.world().get::<Door>(door).unwrap().open;
            assert_eq!(ink_open, rust_open, "frame {i}: both closed pre-flip");
        }
        let _ = switch;
    }

    /// The documented divergence (MIGRATION.md): the Rust baseline
    /// (`door_sync_system`) is bidirectional — flipping the switch back off
    /// RE-LOCKS the door. The ink port's `WakeArming::Once` semantics never
    /// re-lock once opened. This test proves that's a deliberate, tested
    /// simplification, not a silent bug.
    #[test]
    fn ink_door_diverges_from_rust_baseline_on_relock() {
        let mut app = build_app();
        let story = build_doors_story(&mut app);
        let switch = spawn_switch(&mut app, 7, false);
        let door = spawn_door(&mut app, &story, 7);

        // Flip sequence: closed, closed, ON (opens), stays on, then back OFF.
        let script = [false, false, true, true, true, false, false, false];
        let mut rust_open = false;
        for &on in &script {
            app.world_mut().get_mut::<Switch>(switch).unwrap().on = on;
            app.update();
            rust_open = on; // door_sync_system: door.open = switches.any(|s| s.on)
        }
        // Let the wake settle over a few more frames (dormant → Woken →
        // Collect → Ended can straddle a frame boundary beyond the raw flip
        // sequence above).
        pump(&mut app, 6);

        let ink_open = app.world().get::<Door>(door).unwrap().open;
        assert!(ink_open, "the ink door opened once the switch went true");
        assert!(
            !rust_open,
            "fixture sanity: the flip sequence ends with the switch off"
        );
        assert_ne!(
            ink_open, rust_open,
            "documented divergence: WakeArming::Once never re-locks an ink \
             door once opened; the Rust door_sync_system baseline does"
        );
    }

    /// Measure the `should_open` wake condition's per-evaluation cost (the
    /// number `run_flow_sleep` pays every must-polled wake pass while a door
    /// stays locked) against a trivial Rust baseline. Prints the numbers for
    /// the friction journal; run with
    /// `cargo test measure_ink_door_wake_cost -- --nocapture`.
    #[test]
    fn measure_ink_door_wake_cost() {
        use std::hint::black_box;

        const N: u32 = 10_000;

        let mut app = build_app();
        let story = build_doors_story(&mut app);
        let _switch = spawn_switch(&mut app, 0, false);
        let door = spawn_door(&mut app, &story, 0);
        pump(&mut app, 2); // fulfill the request; stays Parked (switch is off)

        let start = std::time::Instant::now();
        for _ in 0..N {
            call_ink_function::<DoorsStory>(app.world_mut(), door, "should_open", &[])
                .expect("should_open");
        }
        let ink_cost = start.elapsed() / N;

        // Rust baseline: the trivial predicate `door_sync_system` evaluates
        // per door per frame.
        let switches = [Switch { id: 0, on: false }];
        let door_switch_id: u8 = 0;
        let start = std::time::Instant::now();
        for _ in 0..N {
            let _ = black_box(
                switches
                    .iter()
                    .any(|sw| sw.id == black_box(door_switch_id) && sw.on),
            );
        }
        let rust_cost = start.elapsed() / N;

        println!(
            "ink should_open (bind_brink_query round trip) per-call: {ink_cost:?}   | \
             rust baseline (trivial bool scan): {rust_cost:?}"
        );
    }
}
