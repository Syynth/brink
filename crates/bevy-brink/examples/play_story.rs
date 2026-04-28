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
//! Each active flow lives on its own entity. Per-flow state — including
//! UI state for that flow — is stored as components on that entity. A
//! game with multiple concurrent NPC dialogues spawns multiple such
//! entities; each one self-contains.
//!
//! ## 2. Request-component fulfillment
//!
//! Spawning a flow is declarative: spawn an entity carrying a
//! `BrinkFlowRequest<M>` (plus any per-flow UI state you want), and
//! the plugin's fulfillment system swaps the request for the live
//! flow components once assets are ready. No polling, no readiness
//! latches.
//!
//! ## 3. Observer-driven UI updates
//!
//! As the flow advances, observer events fire on the flow's entity:
//! `BrinkLineDelivered`, `BrinkChoicesPresented`, `BrinkTurnDone`,
//! `BrinkStoryEnded` (plus `BrinkFlowReset` in dev mode). Observers
//! receive `trigger.event().entity` to identify *which* flow produced
//! the event, and write into per-flow UI components on that entity.
//!
//! ## 4. Hot-reload as event redelivery
//!
//! When the source file changes, `replay_on_reload` rebuilds the flow
//! against the new bytecode and walks to the player's current page,
//! firing the same observer events as a fresh advance. UIs that
//! react to those events stay in sync automatically.

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
    BrinkChoicesPresented, BrinkContext, BrinkFlow, BrinkFlowRequest, BrinkFlowReset,
    BrinkLineDelivered, BrinkLocale, BrinkPlugin, BrinkProgram, BrinkReplayLog, BrinkStoryAsset,
    BrinkStoryEnded, BrinkTurnDone, LineTablesAsset, ProgramAsset, digit_key_to_choice_index,
};
use brink_runtime::{FallbackHandler, StoryStatus};

// ── Per-flow UI components ────────────────────────────────────────────────
//
// These live on the flow entity, alongside the brink-provided components
// (BrinkFlow, BrinkContext, BrinkProgram, BrinkLocale, BrinkReplayLog).
// Observers populate them; the rendering system reads them.
//
// In a multi-flow game (several concurrent NPC dialogues), each flow
// entity has its own copy of these — no global cross-talk.

/// Accumulated text for the current "page" of the story. Cleared when
/// the user advances or selects a choice.
#[derive(Component, Default)]
struct PageText(String);

/// Choices currently presented to the player, indexed 1..=N for keyboard
/// digits. Empty when the story isn't waiting for a choice.
#[derive(Component, Default)]
struct PendingChoices(Vec<String>);

/// Status banner — used for ephemeral feedback like "Reloaded from
/// disk" or "Story ended". Per-flow because each flow may have its
/// own status to surface.
#[derive(Component, Default)]
struct Banner(String);

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
        // Observers run synchronously in response to triggered events.
        // No MessageReader/Writer needed. Each observer reacts to one
        // event variant. This is also the path replay uses: when the
        // story file changes, the plugin's replay system fires the
        // same events as it walks the new bytecode, so the UI stays
        // in sync automatically.
        .add_observer(on_line)
        .add_observer(on_choices)
        .add_observer(on_turn_done)
        .add_observer(on_story_ended)
        .add_observer(on_flow_reset)
        .add_systems(Startup, (setup_ui, load_story))
        .add_systems(Update, (handle_input, update_displays.after(handle_input)))
        .run();
}

// ── (1) Loading a story ───────────────────────────────────────────────────
//
// One `commands.spawn` carrying the request component + the per-flow
// UI components. The fulfillment system swaps in the live flow
// components without disturbing the UI state.

fn load_story(asset_server: Res<AssetServer>, mut commands: Commands) {
    info!("loading story.ink");
    let story: Handle<BrinkStoryAsset> = asset_server.load("story.ink");
    commands.spawn((
        BrinkFlowRequest::<()>::builder().story(story).build(),
        // Per-flow UI state, attached to the same entity.
        PageText::default(),
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
// entity that produced the event and writes into that entity's
// per-flow UI components.
//
// For a single-flow app this is just `q.get_mut(event.entity)`. For a
// multi-flow app the same pattern routes events to the right entity's
// state automatically.

fn on_line(
    trigger: On<BrinkLineDelivered<()>>,
    mut q: Query<&mut PageText>,
) {
    if let Ok(mut page) = q.get_mut(trigger.event().entity) {
        page.0.push_str(&trigger.event().text);
    }
}

fn on_choices(
    trigger: On<BrinkChoicesPresented<()>>,
    mut q: Query<(&mut PageText, &mut PendingChoices)>,
) {
    if let Ok((mut page, mut choices)) = q.get_mut(trigger.event().entity) {
        let event = trigger.event();
        page.0.push_str(&event.text);
        choices.0 = event.choices.iter().map(|c| c.text.clone()).collect();
    }
}

fn on_turn_done(
    trigger: On<BrinkTurnDone<()>>,
    mut q: Query<&mut PageText>,
) {
    if let Ok(mut page) = q.get_mut(trigger.event().entity) {
        page.0.push_str(&trigger.event().text);
    }
}

fn on_story_ended(
    trigger: On<BrinkStoryEnded<()>>,
    mut q: Query<(&mut PageText, &mut Banner)>,
) {
    if let Ok((mut page, mut banner)) = q.get_mut(trigger.event().entity) {
        page.0.push_str(&trigger.event().text);
        banner.0 = "Story ended. Edit the .ink file or press ESC to quit.".to_string();
    }
}

/// Fired by the plugin's reload-replay system *before* it walks the
/// new bytecode. Clearing per-flow UI state here (rather than on
/// `AssetEvent::Modified`) lets trigger ordering guarantee that the
/// subsequent line-delivery events from replay populate fresh state —
/// no risk of clearing *after* the page was already populated.
fn on_flow_reset(
    trigger: On<BrinkFlowReset<()>>,
    mut q: Query<(&mut PageText, &mut PendingChoices, &mut Banner)>,
) {
    if let Ok((mut page, mut choices, mut banner)) = q.get_mut(trigger.event().entity) {
        page.0.clear();
        choices.0.clear();
        banner.0 = "Reloaded — replayed choices in the new program.".to_string();
    }
}

// ── (4) Driving the flow ──────────────────────────────────────────────────
//
// Read input, look up the assets the flow needs (program + line
// tables — the per-flow handles point us at the Asset slots), call
// `choose` / `advance_until_terminal` against the per-flow Context.
//
// The query holds every per-flow component this system needs to
// touch: BrinkFlow (mutable, to step), BrinkContext (mutable, the
// Context the flow advances against), BrinkProgram + BrinkLocale
// (read-only handles), BrinkReplayLog (mutable, to record choices
// for hot-reload), Banner (mutable, to surface errors), PageText +
// PendingChoices (mutable, cleared when we advance).

fn handle_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut flows: Query<(
        Entity,
        &mut BrinkFlow<()>,
        &mut BrinkContext<()>,
        &BrinkProgram<()>,
        &BrinkLocale<()>,
        &mut BrinkReplayLog<()>,
        &mut PageText,
        &mut PendingChoices,
        &mut Banner,
    )>,
    programs: Res<Assets<ProgramAsset>>,
    line_tables_assets: Res<Assets<LineTablesAsset>>,
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
        mut page,
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

    // While a choice is active: digit keys pick. After picking, advance
    // until the next terminal line.
    if !choices.0.is_empty() {
        if let Some(idx) = digit_key_to_choice_index(&keys, choices.0.len()) {
            if let Err(err) = flow.choose_recording(&mut ctx.inner, &mut replay_log, idx) {
                banner.0 = format!("choose error: {err}");
                return;
            }
            page.0.clear();
            choices.0.clear();
            if let Err(err) = flow.advance_until_terminal(
                &program_asset.program,
                &lt_asset.tables,
                &mut ctx.inner,
                &FallbackHandler,
                entity,
                &mut commands,
            ) {
                banner.0 = format!("advance error: {err}");
            }
        }
        return;
    }

    // Otherwise, SPACE advances when the flow is active or just done.
    if keys.just_pressed(KeyCode::Space) {
        match flow.inner.status() {
            StoryStatus::Active | StoryStatus::Done => {
                page.0.clear();
                if let Err(err) = flow.advance_until_terminal(
                    &program_asset.program,
                    &lt_asset.tables,
                    &mut ctx.inner,
                    &FallbackHandler,
                    entity,
                    &mut commands,
                ) {
                    banner.0 = format!("advance error: {err}");
                }
            }
            StoryStatus::Ended => {
                banner.0 = "Story ended. Edit the .ink file or press ESC to quit.".to_string();
            }
            StoryStatus::WaitingForChoice => {}
        }
    }
}

// ── (5) Rendering the UI ──────────────────────────────────────────────────
//
// Read the per-flow UI components and write into the visible Text
// nodes. For a multi-flow game with separate UI panels, you'd query
// each flow entity and route to a per-flow Text node — this version
// just shows the single flow's state.

fn update_displays(
    flow_ui: Query<(&PageText, &PendingChoices, &Banner)>,
    mut q_story: Query<&mut Text, (With<StoryText>, Without<InstructionsText>, Without<BannerText>)>,
    mut q_instr: Query<&mut Text, (With<InstructionsText>, Without<StoryText>, Without<BannerText>)>,
    mut q_banner: Query<&mut Text, (With<BannerText>, Without<StoryText>, Without<InstructionsText>)>,
) {
    let Ok((page, choices, banner)) = flow_ui.single() else {
        return;
    };

    if let Ok(mut text) = q_story.single_mut() {
        if choices.0.is_empty() {
            if page.0.is_empty() {
                text.0 = "(press SPACE to begin)".to_string();
            } else {
                text.0.clone_from(&page.0);
            }
        } else {
            let mut s = page.0.clone();
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
