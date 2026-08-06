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
//!   attaches a [`BrinkFlowRequest<DoorsStory>`] + a dormant, reversible-latch
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
//! * **Detect verdict — component change-tick, cheap path (#996).**
//!   `should_open`'s dependency is a `Switch` component, not a `BrinkGlobals`
//!   variable. `main.rs` registers that component
//!   (`register_capability::<DoorsStory, Switch>("Switch")`), so
//!   `mark_wake_dirty` gets a per-frame change-tracker for it
//!   (`docs/effects-spec.md` §12.5, now wired — see [`switch_detect_summary`]).
//!   A parked locked door re-evaluates `should_open` (a `bind_brink_query`
//!   round trip) only when a `Switch` actually changes, not every frame —
//!   lifting the must-poll interim this port originally documented. This is
//!   the exact `is_player_nearby`-reading-`Transform` case the `sleep` module's
//!   own docs anticipate, now a real consumer of the change-tick wiring.
//! * **Fully reversible (`WakeArming::Latch`, issue #1081).** `doors.ink`
//!   never ends — it loops via `-> DONE` / `-> door_watch` (the same
//!   self-looping idiom `bevy-brink`'s own `sleep` module test fixtures
//!   use), and the flow's [`FlowSleep`] policy is a
//!   [`WakeArming::Latch`](bevy_brink::WakeArming::Latch): it wakes on
//!   switch-on, then re-arms watching for switch-off, then switch-on again,
//!   indefinitely. The `Latch` mode does the edge detection host-side, so
//!   `should_open` stays a plain level predicate (`is_switch_on()`) — no
//!   ink-side "was I previously open" state is needed. [`ink_door_sync_system`]
//!   reads which edge the policy currently watches for via
//!   [`FlowSleep::latch_waiting_for`] to derive [`Door::open`]. This closes
//!   the divergence from the Rust baseline (`doors::door_sync_system`, fully
//!   bidirectional) that an earlier `WakeArming::Once` version of this port
//!   had to accept — see the parity test in this module and `MIGRATION.md`'s
//!   G5 entry.

use std::collections::BTreeMap;
use std::time::Instant;

use bevy::prelude::*;
use bevy_brink::{
    BrinkFlowRequest, BrinkQueryInput, BrinkStoryAsset, DetectSummary, FlowSleep, Value,
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
/// variable. The `true` bit says the dependency is change-detection-capable;
/// because `main.rs` registers the `Switch` component under this same `"Switch"`
/// name (`register_capability::<DoorsStory, Switch>`), `mark_wake_dirty` now
/// has a per-frame change-tracker for it (`docs/effects-spec.md` §12.5, #996),
/// so a parked door re-evaluates `should_open` only when a `Switch` actually
/// changes — the cheap path, not the must-poll interim this port first shipped.
fn switch_detect_summary() -> DetectSummary {
    DetectSummary::from_bits(BTreeMap::from([("Switch".to_string(), true)]))
}

/// Ink mode only (`--doors-impl ink`): attach a dormant, reversible-latch ink
/// flow to every freshly spawned locked-door entity. `doors::spawn_doors_from_layout`
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
            FlowSleep::<DoorsStory>::latch("should_open")
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
/// door's [`FlowSleep`] latch state into its [`Door::open`] + sprite color, at
/// the same nanosecond-timed [`BehaviorTimings::doors`] slot the Rust
/// baseline fills, so the HUD's `doors` line reports whichever impl is live.
///
/// `Door::open` is derived from [`FlowSleep::latch_waiting_for`] (issue
/// #1081's `WakeArming::Latch`), not from the flow's `StoryStatus` — the
/// flow never ends (see `assets/doors.ink`'s `door_watch` loop), so `Ended`
/// is no longer a signal at all here. `latch_waiting_for() == true` means
/// the policy is watching for the switch to turn ON (the door is currently
/// locked); `false` means it is watching for the switch to turn OFF (the
/// door is currently open) — fully bidirectional, matching the Rust
/// baseline (`doors::door_sync_system`'s `door.open = switches.any(|s| s.on)`).
pub fn ink_door_sync_system(
    mut doors: Query<(&mut Door, &mut Sprite, &FlowSleep<DoorsStory>)>,
    mut timings: ResMut<BehaviorTimings>,
) {
    let start = Instant::now();

    for (mut door, mut sprite, sleep) in &mut doors {
        let open = !sleep.latch_waiting_for();
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
    use bevy_brink::{
        BrinkBindingsAppExt, BrinkCapabilityAppExt, BrinkPlugin, advance_batch, call_ink_function,
    };

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
        // #996: register the `Switch` component so the wake layer's per-frame
        // change-tracker (§12.5) covers `should_open`'s dependency — matching
        // `main.rs`, so these tests exercise the same cheap path production does.
        app.register_capability::<DoorsStory, Switch>("Switch");
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
    /// dormant, reversible-latch flow.
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
                FlowSleep::<DoorsStory>::latch("should_open")
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

    /// The G5 closure (issue #1081, `MIGRATION.md`): the Rust baseline
    /// (`door_sync_system`) is bidirectional — flipping the switch back off
    /// RE-LOCKS the door. The `WakeArming::Latch`-based ink port now matches
    /// it exactly, cycling through several full open/close rounds — where an
    /// earlier `WakeArming::Once` version of this port had to accept a
    /// documented divergence (never re-locking) instead.
    #[test]
    fn ink_door_matches_rust_baseline_on_relock() {
        let mut app = build_app();
        let story = build_doors_story(&mut app);
        let switch = spawn_switch(&mut app, 7, false);
        let door = spawn_door(&mut app, &story, 7);
        pump(&mut app, 4); // fulfill; starts locked (switch off)
        assert!(!app.world().get::<Door>(door).unwrap().open);

        // Several full cycles: on (opens) -> off (re-locks) -> on -> off ...
        // — `door_sync_system`'s predicate is `switches.any(|s| s.on)`, so the
        // Rust-baseline expectation at each step is simply the switch's own
        // live value.
        let script = [true, false, true, false, true, false];
        for (i, &on) in script.iter().enumerate() {
            app.world_mut().get_mut::<Switch>(switch).unwrap().on = on;
            pump(&mut app, 8); // let the wake settle (dirty -> evaluate -> Woken -> Collect -> Done -> re-park)
            let ink_open = app.world().get::<Door>(door).unwrap().open;
            assert_eq!(
                ink_open, on,
                "step {i}: the ink door must track the switch bidirectionally, exactly like \
                 the Rust door_sync_system baseline (switch on={on})"
            );
        }
    }

    /// Measure the `should_open` wake condition's per-evaluation cost — the
    /// `bind_brink_query` round trip `run_flow_sleep` used to pay **every**
    /// wake pass while a door stayed locked (the must-poll interim) — against a
    /// trivial Rust baseline. With #996's §12.5 change-tick wiring a parked
    /// door now pays this cost **zero** times per frame while its `Switch` is
    /// unchanged (replaced by one shared `Query<(), Changed<Switch>>::is_empty()`
    /// tick check, amortized across all doors), so this number is now the cost
    /// the cheap path *avoids*, not one paid per locked door per frame. Prints
    /// the numbers for the friction journal / #996's before-after data point;
    /// run with `cargo test measure_ink_door_wake_cost -- --nocapture`.
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
