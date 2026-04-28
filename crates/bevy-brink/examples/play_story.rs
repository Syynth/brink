//! Visual bevy-brink example: load a `.ink` story, render it in a Bevy
//! UI window, advance with SPACE, choose with number keys, and **edit
//! the source file mid-execution to see the program reload live**.
//!
//! Run with `cargo run --example play_story` from the repo root. While
//! the window is open, edit `crates/bevy-brink/examples/assets/story.ink`
//! (or `characters.ink`, which it INCLUDEs), save, and the displayed
//! story restarts from the beginning with the new content.
//!
//! Demonstrates:
//! - Request-component spawn pattern: `BrinkFlowRequest` + the plugin
//!   handles asset readiness, init, globals, line tables.
//! - Observer-based event hooks: per-line UI updates without buffered
//!   message readers.
//! - Hot-reload replay: when the source changes, the plugin rebuilds
//!   the flow against the new bytecode and replays recorded choices,
//!   firing the same observer events along the way.

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
    BrinkStoryEnded, BrinkTurnDone, FlowStart, LineTablesAsset, ProgramAsset,
    digit_key_to_choice_index,
};
use brink_runtime::{FallbackHandler, StoryStatus};

// ── Resources ──────────────────────────────────────────────────────────────

/// Accumulated text for the current "page" of the story. Cleared when
/// the user advances or selects a choice.
#[derive(Resource, Default)]
struct PageText(String);

/// Choices currently presented to the player, indexed 1..=N for keyboard
/// digits. Empty when the story isn't waiting for a choice.
#[derive(Resource, Default)]
struct PendingChoices(Vec<String>);

/// Status banner across the top of the screen — used for ephemeral
/// feedback like "Reloaded from disk" or "Story ended".
#[derive(Resource, Default)]
struct Banner(String);

// ── Marker components for UI nodes ─────────────────────────────────────────

#[derive(Component)]
struct StoryText;

#[derive(Component)]
struct InstructionsText;

#[derive(Component)]
struct BannerText;

// ── App entry ──────────────────────────────────────────────────────────────

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/examples/assets").to_string(),
            watch_for_changes_override: Some(true),
            ..Default::default()
        }))
        .add_plugins(BrinkPlugin::<()>::default())
        .insert_resource(PageText::default())
        .insert_resource(PendingChoices::default())
        .insert_resource(Banner::default())
        // Observers run synchronously in response to triggered events.
        // No MessageReader/Writer needed — each observer reacts to its
        // event variant. This is also the path replay uses: when the
        // story file changes, the plugin's replay system fires the same
        // events as it walks the new bytecode, so the UI stays in sync
        // automatically.
        .add_observer(on_line)
        .add_observer(on_choices)
        .add_observer(on_turn_done)
        .add_observer(on_story_ended)
        .add_observer(on_flow_reset)
        .add_systems(Startup, (setup_ui, load_story))
        .add_systems(
            Update,
            (handle_input, update_displays.after(handle_input)),
        )
        .run();
}

// ── Startup ────────────────────────────────────────────────────────────────

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

fn load_story(asset_server: Res<AssetServer>, mut commands: Commands) {
    info!("loading story.ink");
    let handle: Handle<BrinkStoryAsset> = asset_server.load("story.ink");
    commands.spawn(
        BrinkFlowRequest::<()>::builder()
            .story(handle)
            .start(FlowStart::Root)
            .build(),
    );
}

// ── Observers (one per Line variant) ───────────────────────────────────────

fn on_line(trigger: On<BrinkLineDelivered<()>>, mut page: ResMut<PageText>) {
    page.0.push_str(&trigger.event().text);
}

fn on_choices(
    trigger: On<BrinkChoicesPresented<()>>,
    mut page: ResMut<PageText>,
    mut choices: ResMut<PendingChoices>,
) {
    let event = trigger.event();
    page.0.push_str(&event.text);
    choices.0 = event.choices.iter().map(|c| c.text.clone()).collect();
}

fn on_turn_done(trigger: On<BrinkTurnDone<()>>, mut page: ResMut<PageText>) {
    page.0.push_str(&trigger.event().text);
}

fn on_story_ended(
    trigger: On<BrinkStoryEnded<()>>,
    mut page: ResMut<PageText>,
    mut banner: ResMut<Banner>,
) {
    page.0.push_str(&trigger.event().text);
    banner.0 = "Story ended. Edit the .ink file or press ESC to quit.".to_string();
}

/// Fired by the plugin's reload-replay system before it walks the new
/// bytecode. Clearing here (instead of on `AssetEvent::Modified`) lets
/// trigger ordering guarantee that the subsequent line-delivery events
/// from replay populate fresh state — no risk of clearing *after* the
/// page was already populated.
fn on_flow_reset(
    _trigger: On<BrinkFlowReset<()>>,
    mut page: ResMut<PageText>,
    mut choices: ResMut<PendingChoices>,
    mut banner: ResMut<Banner>,
) {
    page.0.clear();
    choices.0.clear();
    banner.0 = "Reloaded — replayed choices in the new program.".to_string();
}

// ── Input ──────────────────────────────────────────────────────────────────

fn handle_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut flows: Query<(
        Entity,
        &mut BrinkFlow<()>,
        &mut BrinkContext<()>,
        &BrinkProgram<()>,
        &BrinkLocale<()>,
        &mut BrinkReplayLog<()>,
    )>,
    programs: Res<Assets<ProgramAsset>>,
    line_tables_assets: Res<Assets<LineTablesAsset>>,
    mut page: ResMut<PageText>,
    mut choices: ResMut<PendingChoices>,
    mut banner: ResMut<Banner>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
        return;
    }

    let Ok((entity, mut flow, mut ctx, brink_program, locale, mut replay_log)) =
        flows.single_mut()
    else {
        return;
    };
    let Some(program_asset) = programs.get(&brink_program.handle) else {
        return;
    };
    let Some(lt_asset) = line_tables_assets.get(&locale.handle) else {
        return;
    };

    // Choice selection: digit 1 through 9 picks choices[0..=8] when a
    // choice prompt is active.
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

    // SPACE advances when the story is active or just done.
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

// ── Display update ─────────────────────────────────────────────────────────

fn update_displays(
    page: Res<PageText>,
    choices: Res<PendingChoices>,
    banner: Res<Banner>,
    mut q_story: Query<&mut Text, (With<StoryText>, Without<InstructionsText>, Without<BannerText>)>,
    mut q_instr: Query<&mut Text, (With<InstructionsText>, Without<StoryText>, Without<BannerText>)>,
    mut q_banner: Query<&mut Text, (With<BannerText>, Without<StoryText>, Without<InstructionsText>)>,
) {
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
