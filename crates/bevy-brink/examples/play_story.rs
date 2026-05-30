//! Visual bevy-brink example: load a `.ink` story, render it in a Bevy
//! UI window, advance with SPACE, choose with number keys, and **edit
//! the source file mid-execution to see the program reload live**.
//!
//! Run with `cargo run --example play_story` from the repo root. While
//! the window is open, edit `crates/bevy-brink/examples/assets/story.ink`
//! (or `characters.ink`, which it INCLUDEs), save, and the displayed
//! story restarts from the beginning with the new content.
//!
//! # Patterns demonstrated
//!
//! ## 1. One entity per conversation
//!
//! Each active flow lives on its own entity. Per-flow input state
//! (pending choices, status banner) is stored as components on that
//! entity. A game with multiple concurrent NPC dialogues spawns
//! multiple such entities; each one self-contains.
//!
//! ## 2. Request-component fulfillment
//!
//! Spawning a flow is declarative: spawn an entity carrying a
//! `BrinkFlowRequest<M>` (plus any per-flow UI state you want), and
//! the plugin's fulfillment system swaps the request for the live
//! flow components once assets are ready. No polling, no readiness
//! latches.
//!
//! ## 3. The flow's transcript is the rendering source of truth
//!
//! `BrinkFlow.inner.transcript()` is an append-only structural log of
//! every output part the runtime has produced. The renderer walks
//! it through `brink_runtime::transcript::render_transcript` against
//! the current line tables to produce displayable lines — locale
//! swaps re-render the same transcript with different tables.
//!
//! UIs do **not** maintain their own running text by accumulating from
//! `BrinkLineDelivered` events. The runtime owns the transcript;
//! consumers read it. Observers exist for *input state* (which choices
//! are pending) and *status transitions* (story just ended, hot-reload
//! happened) — not for assembling text.
//!
//! ## 4. One SPACE press = one `step_one`
//!
//! The consumer's input pace is the flow's pace. External function
//! calls and per-line side effects fire at predictable moments. There
//! is no "advance until terminal" verb in production code.
//!
//! ## 5. Hot-reload as transcript reset
//!
//! When the source file changes, `replay_on_reload` rebuilds the flow
//! against the new bytecode (clearing its transcript) and walks to
//! the player's current page, firing the same observer events as a
//! fresh advance. The transcript repopulates naturally; UIs reading
//! it stay in sync.

#![expect(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "bevy systems take Res/Query by value and naturally have many arguments and complex query filters"
)]

use std::fmt::Write as _;

use bevy::input::ButtonInput;
use bevy::prelude::*;
use bevy_brink::{
    BrinkBindings, BrinkBindingsAppExt, BrinkChoicesPresented, BrinkCommand, BrinkContext,
    BrinkFlow, BrinkFlowRequest, BrinkFlowReset, BrinkLocale, BrinkPlugin, BrinkProgram,
    BrinkReplayLog, BrinkStoryAsset, BrinkStoryEnded, BrinkTranscript, LineTablesAsset,
    ProgramAsset, Value, digit_key_to_choice_index,
};
use brink_runtime::StoryStatus;

// ── Per-flow UI components ────────────────────────────────────────────────
//
// These live on the flow entity alongside the brink-provided components
// (BrinkFlow, BrinkContext, BrinkProgram, BrinkLocale, BrinkReplayLog,
// and the opt-in BrinkTranscript). PendingChoices is *input state* —
// what digit keys mean right now. Banner is *status* — ephemeral
// feedback the rendering system surfaces.
//
// We do NOT keep an accumulated "page text" component. Story text is
// owned by the runtime: BrinkTranscript<M> is auto-rendered by the
// plugin from `flow.inner.transcript()`, and we read it in the render
// system. UIs that want to display something other than the runtime's
// transcript (e.g. typewriter animation per BrinkLineDelivered event)
// can opt out of BrinkTranscript and observe events directly.

/// Choices currently presented to the player, indexed 1..=N for keyboard
/// digits. Empty when the story isn't waiting for a choice.
#[derive(Component, Default)]
struct PendingChoices(Vec<String>);

/// Status banner — used for ephemeral feedback like "Reloaded from
/// disk" or "Story ended". Per-flow because each flow may have its
/// own status to surface.
#[derive(Component, Default)]
struct Banner(String);

/// Fire-and-forget command event for the story's `~ play_sound("name")`
/// calls. `#[derive(BrinkCommand)]` generates the parse from the ink
/// args (one `String`); `#[derive(Event)]` makes it observable. A real
/// game would play audio in the observer — here we just log it.
#[derive(Event, BrinkCommand)]
struct PlaySound {
    name: String,
}

// ── UI node markers ───────────────────────────────────────────────────────
//
// These tag the visible Text nodes in the layout. The render system
// looks them up to write into.

#[derive(Component)]
struct StoryText;

#[derive(Component)]
struct InstructionsText;

#[derive(Component)]
struct BannerText;

// ── App entry ─────────────────────────────────────────────────────────────

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/examples/assets").to_string(),
            watch_for_changes_override: Some(true),
            ..Default::default()
        }))
        .add_plugins(BrinkPlugin::<()>::default())
        // ── External-function bindings (ink → engine) ──────────────────
        // Two synchronous kinds, registered once at startup:
        //   • bind_brink_fn  — a pure function of the ink args. Resolved
        //     inline while the VM steps; no World access. Here: uppercase
        //     a string, so `{shout("come in")}` renders "COME IN".
        //   • bind_brink_command — parse the ink args into a Bevy Event
        //     and fire it (fire-and-forget). The story's `~ play_sound(...)`
        //     calls land as PlaySound events; `on_play_sound` reacts.
        .bind_brink_fn::<(), _, _>("shout", |args| {
            args.first()
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_uppercase()
        })
        .bind_brink_command::<(), PlaySound>("play_sound")
        // Observers track *input state* and *status transitions* — not
        // story text. on_choices captures pending choices for input
        // handling; on_story_ended/on_flow_reset surface user-visible
        // status. Story text is auto-accumulated into BrinkTranscript
        // by the plugin and read in update_displays.
        .add_observer(on_choices)
        .add_observer(on_story_ended)
        .add_observer(on_flow_reset)
        .add_observer(on_play_sound)
        .add_systems(Startup, (setup_ui, load_story))
        .add_systems(Update, (handle_input, update_displays.after(handle_input)))
        .run();
}

// ── (1) Loading a story ───────────────────────────────────────────────────
//
// One `commands.spawn` carrying the request + the per-flow components
// we want on this flow. `BrinkTranscript::default()` opts this flow
// into auto-rendered transcript output — the plugin keeps it in sync
// with `flow.inner.transcript()`. The fulfillment system swaps the
// request for the live flow components without disturbing the rest.

fn load_story(asset_server: Res<AssetServer>, mut commands: Commands) {
    info!("loading story.ink");
    let story: Handle<BrinkStoryAsset> = asset_server.load("story.ink");
    commands.spawn((
        BrinkFlowRequest::<()>::builder().story(story).build(),
        // Auto-rendered transcript — opt-in.
        BrinkTranscript::<()>::default(),
        // Per-flow input state and status.
        PendingChoices::default(),
        Banner::default(),
    ));
}

// ── (2) UI scaffolding ────────────────────────────────────────────────────
//
// Pure Bevy UI — not brink-specific. Three Text nodes (banner, story,
// instructions) inside a column.

fn setup_ui(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands
        .spawn((
            Name::new("UI Root"),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(24.0)),
                row_gap: Val::Px(16.0),
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn((
                Name::new("Banner"),
                BannerText,
                Text::new(""),
                TextColor(Color::srgb(0.6, 0.85, 0.6)),
                TextFont {
                    font_size: 18.0,
                    ..default()
                },
            ));

            root.spawn((
                Name::new("Story"),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
                StoryText,
                Text::new("Loading story..."),
                TextColor(Color::WHITE),
                TextFont {
                    font_size: 22.0,
                    ..default()
                },
            ));

            root.spawn((
                Name::new("Instructions"),
                InstructionsText,
                Text::new(""),
                TextColor(Color::srgb(0.6, 0.6, 0.6)),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
            ));
        });
}

// ── (3) Reacting to flow events ───────────────────────────────────────────
//
// Each observer reads `trigger.event().entity` to find the flow
// entity that produced the event and writes into that entity's input
// state / status components. Story text is NOT touched here — it
// lives on the runtime side, available via BrinkTranscript.
//
// For a single-flow app `q.get_mut(event.entity)` is just a one-entity
// lookup. For a multi-flow app the same pattern routes events to the
// right entity's state automatically.

fn on_choices(
    trigger: On<BrinkChoicesPresented<()>>,
    mut q: Query<&mut PendingChoices>,
) {
    if let Ok(mut choices) = q.get_mut(trigger.event().entity) {
        choices.0 = trigger
            .event()
            .choices
            .iter()
            .map(|c| c.text.clone())
            .collect();
    }
}

fn on_story_ended(trigger: On<BrinkStoryEnded<()>>, mut q: Query<&mut Banner>) {
    if let Ok(mut banner) = q.get_mut(trigger.event().entity) {
        banner.0 = "Story ended. Edit the .ink file or press ESC to quit.".to_string();
    }
}

/// Reacts to the story's `~ play_sound(...)` calls. The flow-driver
/// (`handle_input`) builds these into events while stepping and flushes
/// them via `BrinkHandler::flush`; this observer is where a real game
/// would trigger audio. We just log so the binding is visible in the
/// console as you play.
fn on_play_sound(trigger: On<PlaySound>) {
    info!("[play_sound] {}", trigger.event().name);
}

/// Fired by the plugin's reload-replay system *before* it walks the
/// new bytecode. Clears input state and updates the banner. The
/// transcript is reset by the runtime when the flow is rebuilt; we
/// don't touch it here.
fn on_flow_reset(
    trigger: On<BrinkFlowReset<()>>,
    mut q: Query<(&mut PendingChoices, &mut Banner)>,
) {
    if let Ok((mut choices, mut banner)) = q.get_mut(trigger.event().entity) {
        choices.0.clear();
        banner.0 = "Reloaded — replayed choices in the new program.".to_string();
    }
}

// ── (4) Driving the flow ──────────────────────────────────────────────────
//
// Each SPACE press maps to ONE `flow.step_one(...)` call — one line at
// a time. The consumer's input pace is the flow's pace, so external
// function calls and other per-line side effects fire at predictable
// moments. (Walking past multiple lines in one shot via a "step until
// terminal" verb is *not* the recommended pattern for production game
// code: external callbacks would fire at unpredictable times relative
// to player input.)
//
// Each step fires observer events; the observers above append text
// into the per-flow `PageText` component. So this handler is just
// "consume input → call step_one or choose; let the events do the
// rendering." The transcript accumulates for the entire run — it's
// only ever cleared on hot-reload (`BrinkFlowReset`).

fn handle_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut flows: Query<(
        Entity,
        &mut BrinkFlow<()>,
        &mut BrinkContext<()>,
        &BrinkProgram<()>,
        &BrinkLocale<()>,
        &mut BrinkReplayLog<()>,
        &mut PendingChoices,
        &mut Banner,
    )>,
    programs: Res<Assets<ProgramAsset>>,
    line_tables_assets: Res<Assets<LineTablesAsset>>,
    bindings: Res<BrinkBindings<()>>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
        return;
    }

    let Ok((
        entity,
        mut flow,
        mut ctx,
        brink_program,
        locale,
        mut replay_log,
        mut choices,
        mut banner,
    )) = flows.single_mut()
    else {
        return;
    };
    let Some(program_asset) = programs.get(&brink_program.handle) else {
        return;
    };
    let Some(lt_asset) = line_tables_assets.get(&locale.handle) else {
        return;
    };

    // While a choice is active: digit keys pick. choose() consumes
    // the pending choice but does not produce output; the next SPACE
    // will step into the chosen path. Whether the picked choice
    // appears in the transcript depends on the ink choice syntax —
    // content-emitting choices (e.g. `* Hello\n  ...`) are part of
    // the runtime's output and land in BrinkTranscript naturally;
    // suppressed choices (e.g. `* [Hello] -> ...`) don't.
    if !choices.0.is_empty() {
        if let Some(idx) = digit_key_to_choice_index(&keys, choices.0.len()) {
            if let Err(err) = flow.choose_recording(&mut ctx.inner, &mut replay_log, idx) {
                banner.0 = format!("choose error: {err}");
                return;
            }
            choices.0.clear();
        }
        return;
    }

    if !keys.just_pressed(KeyCode::Space) {
        return;
    }

    // Status-driven dispatch:
    // - Active / Done: step one more line. Done means a `-> DONE` was
    //   hit last step; the next step begins a new turn naturally.
    // - Ended: nothing more to do.
    // - WaitingForChoice: handled above; reaching here would be a bug
    //   (choices should be non-empty), but guard anyway.
    match flow.inner.status() {
        StoryStatus::Active | StoryStatus::Done => {}
        StoryStatus::Ended => {
            banner.0 = "Story ended. Edit the .ink file or press ESC to quit.".to_string();
            return;
        }
        StoryStatus::WaitingForChoice => return,
    }

    // Build a handler from the binding registry, step one line through
    // it (pure-fn bindings resolve inline; command bindings are buffered),
    // then flush the buffered command events into the world.
    let handler = bindings.handler();
    let result = flow.step_one(
        &program_asset.program,
        &lt_asset.tables,
        &mut ctx.inner,
        &handler,
        entity,
        &mut commands,
    );
    handler.flush(&mut commands);
    if let Err(err) = result {
        banner.0 = format!("step error: {err}");
    }
}

// ── (5) Rendering the UI ──────────────────────────────────────────────────
//
// Read the runtime-owned BrinkTranscript<()> and the per-flow input
// state, write into the visible Text nodes. For a multi-flow game
// with separate UI panels, query each flow entity and route to a
// per-flow Text node — this version just shows the single flow's
// state.

fn update_displays(
    flow_q: Query<(&BrinkTranscript<()>, &PendingChoices, &Banner)>,
    mut q_story: Query<&mut Text, (With<StoryText>, Without<InstructionsText>, Without<BannerText>)>,
    mut q_instr: Query<&mut Text, (With<InstructionsText>, Without<StoryText>, Without<BannerText>)>,
    mut q_banner: Query<&mut Text, (With<BannerText>, Without<StoryText>, Without<InstructionsText>)>,
) {
    let Ok((transcript, choices, banner)) = flow_q.single() else {
        return;
    };

    if let Ok(mut text) = q_story.single_mut() {
        let body = transcript.text();
        if choices.0.is_empty() {
            if body.is_empty() {
                text.0 = "(press SPACE to begin)".to_string();
            } else {
                text.0 = body;
            }
        } else {
            let mut s = body;
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            for (i, c) in choices.0.iter().enumerate() {
                let _ = write!(s, "\n  [{}] {}", i + 1, c);
            }
            text.0 = s;
        }
    }

    if let Ok(mut text) = q_instr.single_mut() {
        text.0 = if choices.0.is_empty() {
            "SPACE: advance     ESC: quit".to_string()
        } else {
            "1-9: select choice     ESC: quit".to_string()
        };
    }

    if let Ok(mut text) = q_banner.single_mut() {
        text.0.clone_from(&banner.0);
    }
}
