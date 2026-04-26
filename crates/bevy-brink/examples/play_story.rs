//! Visual bevy-brink example: load a `.ink` story, render it in a Bevy
//! UI window, advance with SPACE, choose with number keys, and **edit
//! the source file mid-execution to see the program reload live**.
//!
//! Run with `cargo run --example play_story` from the repo root. While
//! the window is open, edit `crates/bevy-brink/examples/assets/story.ink`
//! (or `characters.ink`, which it INCLUDEs), save, and the displayed
//! story restarts from the beginning with the new content.
//!
//! Hot-reload works because:
//! - Bevy's `file_watcher` feature watches every asset path that
//!   `LoadContext::read_asset_bytes` was called with.
//! - `InkLoader` walks the transitive INCLUDE graph through that method,
//!   so every file in the project is registered as a dep.
//! - When any file changes, Bevy re-runs `InkLoader`, producing a new
//!   `ProgramAsset` and `LineTablesAsset`. We listen for the resulting
//!   `AssetEvent::Modified` and reset the running flow.

#![expect(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "bevy systems take Res/Query by value and naturally have many arguments and complex query filters"
)]

use std::fmt::Write as _;

use bevy::asset::AssetEvent;
use bevy::input::ButtonInput;
use bevy::prelude::*;
use bevy_brink::{
    BrinkAssetsPlugin, BrinkFlow, BrinkGlobals, BrinkLineTables, BrinkPlugin, BrinkProgram,
    BrinkStoryAsset, LineTablesAsset, ProgramAsset,
};
use brink_runtime::{ExternalFnHandler, ExternalResult, FastRng, FlowInstance, Line, StoryStatus};

// ── Resources ──────────────────────────────────────────────────────────────

/// Holds the in-flight handle to the loaded story bundle.
#[derive(Resource)]
struct StoryHandle(Handle<BrinkStoryAsset>);

/// Latch flipped once we've created the flow + globals from the loaded
/// program. Reset to `false` on hot-reload so init runs again.
#[derive(Resource, Default)]
struct Initialized(bool);

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

// ── External-function handler ──────────────────────────────────────────────

/// Default fallback handler — every external call delegates to its
/// in-story fallback container.
struct FallbackHandler;

impl ExternalFnHandler for FallbackHandler {
    fn call(&self, _name: &str, _args: &[brink_format::Value]) -> ExternalResult {
        ExternalResult::Fallback
    }
}

// ── App entry ──────────────────────────────────────────────────────────────

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            file_path: "crates/bevy-brink/examples/assets".to_string(),
            // Use the `file_watcher` cargo feature on bevy_asset to
            // watch the assets directory for live edits.
            watch_for_changes_override: Some(true),
            ..Default::default()
        }))
        // Use the asset-only plugin (registers the .ink/.inkb loaders +
        // the asset types) but do NOT auto-advance on every tick — we
        // drive advancement from SPACE input instead.
        .add_plugins(BrinkAssetsPlugin)
        .add_plugins(BrinkPlugin::<()>::default())
        .insert_resource(Initialized::default())
        .insert_resource(PageText::default())
        .insert_resource(PendingChoices::default())
        .insert_resource(Banner::default())
        .add_systems(Startup, (setup_ui, load_story))
        .add_systems(
            Update,
            (
                initialize_when_ready,
                handle_program_reload,
                handle_input,
                update_displays,
            )
                .chain(),
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
    commands.insert_resource(StoryHandle(handle));
}

// ── Initialization ─────────────────────────────────────────────────────────

/// Once the bundle and both labeled subassets are loaded, build the
/// runtime state: insert `BrinkGlobals<()>`, populate `BrinkLineTables<()>`,
/// and spawn an entity carrying `BrinkFlow<()>` + `BrinkProgram<()>`.
///
/// Idempotent — once `Initialized.0 == true` it returns immediately.
/// Re-run by setting `Initialized.0 = false` (we do this on hot-reload).
fn initialize_when_ready(
    story_handle: Option<Res<StoryHandle>>,
    story_assets: Res<Assets<BrinkStoryAsset>>,
    program_assets: Res<Assets<ProgramAsset>>,
    line_table_assets: Res<Assets<LineTablesAsset>>,
    existing_flows: Query<Entity, With<BrinkFlow<()>>>,
    mut initialized: ResMut<Initialized>,
    mut line_tables: ResMut<BrinkLineTables<()>>,
    mut page: ResMut<PageText>,
    mut choices: ResMut<PendingChoices>,
    mut banner: ResMut<Banner>,
    mut commands: Commands,
) {
    if initialized.0 {
        return;
    }
    let Some(story_handle) = story_handle else {
        return;
    };
    let Some(bundle) = story_assets.get(&story_handle.0) else {
        return;
    };
    let Some(program_asset) = program_assets.get(&bundle.program) else {
        return;
    };
    let Some(lt_asset) = line_table_assets.get(&bundle.line_tables) else {
        return;
    };

    // Despawn any existing flow entity from a previous compile.
    for e in &existing_flows {
        commands.entity(e).despawn();
    }

    let (flow, context) = FlowInstance::new_at_root(&program_asset.program);

    commands.insert_resource(BrinkGlobals::<()>::new(context));
    line_tables.tables.clone_from(&lt_asset.tables);

    commands.spawn((
        BrinkFlow::<()>::new(flow),
        BrinkProgram::<()>::new(bundle.program.clone()),
    ));

    page.0.clear();
    choices.0.clear();
    initialized.0 = true;
    if banner.0.is_empty() {
        banner.0 = "Story loaded. Press SPACE to begin.".to_string();
    }
    info!("story ready");
}

/// React to `AssetEvent::Modified<ProgramAsset>` — the file watcher saw
/// an `.ink` (or any INCLUDE'd file) change on disk, the loader re-ran,
/// and a new program was emitted. Reset the runtime so init runs again.
fn handle_program_reload(
    mut events: MessageReader<AssetEvent<ProgramAsset>>,
    mut initialized: ResMut<Initialized>,
    mut banner: ResMut<Banner>,
) {
    for event in events.read() {
        if let AssetEvent::Modified { .. } = event {
            initialized.0 = false;
            banner.0 = "Reloaded from disk. Story restarted.".to_string();
            info!("program reloaded — resetting flow");
        }
    }
}

// ── Input + advancement ────────────────────────────────────────────────────

fn handle_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut flows: Query<(&mut BrinkFlow<()>, &BrinkProgram<()>)>,
    globals: Option<ResMut<BrinkGlobals<()>>>,
    line_tables: Res<BrinkLineTables<()>>,
    programs: Res<Assets<ProgramAsset>>,
    mut page: ResMut<PageText>,
    mut choices: ResMut<PendingChoices>,
    mut banner: ResMut<Banner>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut globals) = globals else {
        return;
    };
    let Ok((mut flow, brink_program)) = flows.single_mut() else {
        return;
    };
    let Some(program_asset) = programs.get(&brink_program.handle) else {
        return;
    };

    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
        return;
    }

    // Choice selection: digit 1 through 9 picks choices[0..=8] when a
    // choice prompt is active.
    if !choices.0.is_empty() {
        const DIGIT_KEYS: &[(KeyCode, usize)] = &[
            (KeyCode::Digit1, 0),
            (KeyCode::Digit2, 1),
            (KeyCode::Digit3, 2),
            (KeyCode::Digit4, 3),
            (KeyCode::Digit5, 4),
            (KeyCode::Digit6, 5),
            (KeyCode::Digit7, 6),
            (KeyCode::Digit8, 7),
            (KeyCode::Digit9, 8),
        ];
        for (key, idx) in DIGIT_KEYS {
            if keys.just_pressed(*key) && *idx < choices.0.len() {
                if let Err(err) = flow.inner.choose(&mut globals.inner, *idx) {
                    banner.0 = format!("choose error: {err}");
                    return;
                }
                page.0.clear();
                choices.0.clear();
                advance_until_terminal(
                    &mut flow,
                    &program_asset.program,
                    &line_tables,
                    &mut globals,
                    &mut page.0,
                    &mut choices.0,
                    &mut banner.0,
                );
                return;
            }
        }
        return; // waiting for choice; SPACE does nothing here
    }

    // SPACE advances when the story is active or just done.
    if keys.just_pressed(KeyCode::Space) {
        match flow.inner.status() {
            StoryStatus::Active | StoryStatus::Done => {
                page.0.clear();
                advance_until_terminal(
                    &mut flow,
                    &program_asset.program,
                    &line_tables,
                    &mut globals,
                    &mut page.0,
                    &mut choices.0,
                    &mut banner.0,
                );
            }
            StoryStatus::Ended => {
                banner.0 = "Story ended. Edit the .ink file or press ESC to quit.".to_string();
            }
            StoryStatus::WaitingForChoice => {
                // unreachable: handled above
            }
        }
    }
}

/// Step the flow repeatedly, accumulating Text into `page`, until we
/// hit a terminal Line (Done / Choices / End). Populates `choices` if
/// a choice point is reached; sets a banner string on End.
fn advance_until_terminal(
    flow: &mut BrinkFlow<()>,
    program: &brink_runtime::Program,
    line_tables: &BrinkLineTables<()>,
    globals: &mut BrinkGlobals<()>,
    page: &mut String,
    choices: &mut Vec<String>,
    banner: &mut String,
) {
    const STEP_LIMIT: usize = 10_000;
    for _ in 0..STEP_LIMIT {
        let line = match flow.inner.step_single_line::<FastRng>(
            program,
            &line_tables.tables,
            &mut globals.inner,
            &FallbackHandler,
            None,
        ) {
            Ok(line) => line,
            Err(err) => {
                *banner = format!("runtime error: {err}");
                return;
            }
        };

        match line {
            Line::Text { text, .. } => page.push_str(&text),
            Line::Done { text, .. } => {
                page.push_str(&text);
                return;
            }
            Line::Choices {
                text,
                choices: cs,
                ..
            } => {
                page.push_str(&text);
                choices.extend(cs.into_iter().map(|c| c.text));
                return;
            }
            Line::End { text, .. } => {
                page.push_str(&text);
                *banner = "Story ended. Edit the .ink file or press ESC to quit.".to_string();
                return;
            }
        }
    }
    *banner = "step limit exceeded — likely infinite loop".to_string();
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
                text.0 = "(press SPACE)".to_string();
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
