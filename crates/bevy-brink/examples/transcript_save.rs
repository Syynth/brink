//! Headless example: **`.brkt` transcript persistence** — capture a flow's
//! output history to bytes (the visible-history half of a save file), then
//! reload and re-render it without re-running the story.
//!
//! Run with `cargo run --example transcript_save`. Drives a short story,
//! prints the live transcript, captures it to `.brkt` bytes, reloads them,
//! and re-renders — demonstrating that a saved transcript round-trips. In a
//! real game you'd write the bytes into your save file and `read_transcript`
//! (or the `.brkt` asset loader) them back on load.

use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy_brink::{
    BrinkBindings, BrinkFlow, BrinkFlowRequest, BrinkPlugin, BrinkStoryAsset, BrinkTranscript,
    LineTablesAsset, ProgramAsset, TranscriptAsset, advance_flow, capture_transcript,
    render_transcript_asset,
};
use brink_runtime::{FlowInstance, transcript::read_transcript};

const STORY: &str = "\
You wake at the edge of a quiet shoreline.
The tide is unhurried; the gulls are not.
-> END
";

fn main() {
    let mut app = App::new();
    app.add_plugins((
        LogPlugin::default(),
        AssetPlugin::default(),
        BrinkPlugin::<()>::default(),
    ));
    // advance_flow drives flows through the binding registry; this story has
    // no bindings, but the resource must exist.
    app.init_resource::<BrinkBindings<()>>();

    let Some((story, program_h, base_h)) = build_story(&mut app) else {
        warn!("failed to build story");
        return;
    };

    let flow = app
        .world_mut()
        .spawn((
            BrinkFlowRequest::<()>::builder().story(story).build(),
            BrinkTranscript::<()>::default(),
        ))
        .id();
    app.update(); // fulfill the request

    // Play the story to the end.
    drive_to_end(&mut app, flow);
    app.update(); // refresh_transcripts renders the live transcript
    let live = app
        .world()
        .entity(flow)
        .get::<BrinkTranscript<()>>()
        .map(BrinkTranscript::text)
        .unwrap_or_default();
    info!("--- live transcript ---\n{live}");

    // Capture the flow's output history to `.brkt` bytes (what you'd save).
    let Some(bytes) = capture(&app, flow, &program_h) else {
        warn!("capture failed (flow/program not ready)");
        return;
    };
    info!("captured {} .brkt bytes", bytes.len());

    // Reload the bytes and re-render — no flow, no re-execution.
    let Some(reloaded) = reload_and_render(&app, &bytes, &program_h, &base_h) else {
        warn!("reload/render failed");
        return;
    };
    info!("--- reloaded transcript ---\n{reloaded}");

    info!("done.");
}

fn build_story(
    app: &mut App,
) -> Option<(
    Handle<BrinkStoryAsset>,
    Handle<ProgramAsset>,
    Handle<LineTablesAsset>,
)> {
    let out = brink_compiler::compile("demo.ink", |p| {
        if p == "demo.ink" {
            Ok(STORY.to_string())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no includes",
            ))
        }
    })
    .ok()?;
    let (program, tables) = brink_runtime::link(&out.data).ok()?;
    let (_, initial_context) = FlowInstance::new_at_root(&program);

    let world = app.world_mut();
    let program_h = world
        .resource_mut::<Assets<ProgramAsset>>()
        .add(ProgramAsset {
            program,
            initial_context,
        });
    let base_h = world
        .resource_mut::<Assets<LineTablesAsset>>()
        .add(LineTablesAsset { tables });
    let story_h = world
        .resource_mut::<Assets<BrinkStoryAsset>>()
        .add(BrinkStoryAsset {
            program: program_h.clone(),
            line_tables: base_h.clone(),
        });
    Some((story_h, program_h, base_h))
}

fn drive_to_end(app: &mut App, flow: Entity) {
    loop {
        match advance_flow::<()>(app.world_mut(), flow) {
            Ok(line) => {
                if line.is_terminal() {
                    break;
                }
            }
            Err(err) => {
                warn!("advance_flow failed: {err}");
                break;
            }
        }
    }
}

/// Serialize the live flow's transcript to `.brkt` bytes.
fn capture(app: &App, flow: Entity, program_h: &Handle<ProgramAsset>) -> Option<Vec<u8>> {
    let flow_comp = app.world().entity(flow).get::<BrinkFlow<()>>()?;
    let program = app
        .world()
        .resource::<Assets<ProgramAsset>>()
        .get(program_h)?;
    Some(capture_transcript::<()>(flow_comp, program))
}

/// Decode `.brkt` bytes and re-render them against the program + base tables.
fn reload_and_render(
    app: &App,
    bytes: &[u8],
    program_h: &Handle<ProgramAsset>,
    base_h: &Handle<LineTablesAsset>,
) -> Option<String> {
    let data = read_transcript(bytes).ok()?;
    let asset = TranscriptAsset { data };
    let program = app
        .world()
        .resource::<Assets<ProgramAsset>>()
        .get(program_h)?;
    let base = app
        .world()
        .resource::<Assets<LineTablesAsset>>()
        .get(base_h)?;
    let lines = render_transcript_asset(&asset, program, base, None).ok()?;
    Some(
        lines
            .iter()
            .map(|(t, _)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    )
}
