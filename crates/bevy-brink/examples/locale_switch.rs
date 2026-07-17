//! Headless example: global, event-driven **locale switching** via `.inkl`
//! overlays.
//!
//! Run with `cargo run --example locale_switch`. Prints the story in the base
//! language, switches the global locale to a built-in `es` overlay, and prints
//! it again — localized — then reverts.
//!
//! The overlay is built inline via `brink-intl` (export the base line tables,
//! translate a line, compile to `.inkl`), so the example is self-contained.
//! In a real game you'd `asset_server.load("story.es.inkl")` instead and call
//! `commands.set_brink_locale::<M>(Some(handle))`.

use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy_brink::{
    BrinkBindings, BrinkCurrentLocale, BrinkFlowRequest, BrinkLocaleChanged, BrinkPlugin,
    BrinkStoryAsset, BrinkTranscript, LineTablesAsset, LocaleAsset, ProgramAsset, advance_flow,
};
use brink_intl::ContentJson;
use brink_runtime::FlowInstance;

const STORY: &str = "\
You wake at the edge of a quiet shoreline.
The tide is unhurried.
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

    let Some((story, locale)) = build_story_and_locale(&mut app) else {
        warn!("failed to build story/locale");
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

    // Drive the flow to the end so it produces a transcript.
    drive_to_end(&mut app, flow);
    app.update(); // let refresh_transcripts render the base transcript
    info!("--- base locale ---\n{}", transcript_text(&app, flow));

    // Switch the global locale. In a system you'd write
    // `commands.set_brink_locale::<()>(Some(locale.clone()))`; from `main` we
    // set the resource and fire the event directly (what that API does).
    app.world_mut()
        .resource_mut::<BrinkCurrentLocale<()>>()
        .locale = Some(locale);
    app.world_mut().trigger(BrinkLocaleChanged::<()>::default());
    app.update(); // observer swaps BrinkLocale; refresh_transcripts re-renders
    info!("--- es locale ---\n{}", transcript_text(&app, flow));

    // Revert to base.
    app.world_mut()
        .resource_mut::<BrinkCurrentLocale<()>>()
        .locale = None;
    app.world_mut().trigger(BrinkLocaleChanged::<()>::default());
    app.update();
    info!("--- reverted to base ---\n{}", transcript_text(&app, flow));

    info!("done.");
}

/// Compile the base story, build an `es` `.inkl` overlay inline, and insert all
/// the assets. Returns the story + locale handles.
fn build_story_and_locale(app: &mut App) -> Option<(Handle<BrinkStoryAsset>, Handle<LocaleAsset>)> {
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
    let data = out.data;

    // Round-trip through `.inkb` so the program carries a real checksum that
    // the overlay's `base_checksum` matches (the loader path).
    let mut inkb = Vec::new();
    brink_format::write_inkb(&data, &mut inkb);
    let loaded = brink_format::read_inkb(&inkb).ok()?;
    let (program, base_tables) = brink_runtime::link(&loaded).ok()?;
    let checksum = brink_format::read_inkb_index(&inkb).ok()?.checksum;

    // Translate the first line and compile the overlay.
    let mut lines = brink_intl::export_lines(&loaded, checksum);
    if let Some(first) = lines.scopes.first_mut().and_then(|s| s.lines.first_mut()) {
        first.content = Some(ContentJson::Plain(
            "Despiertas al borde de una orilla tranquila.\n".to_string(),
        ));
    }
    let inkl_bytes = brink_intl::compile_locale(&inkb, &lines, "es").ok()?;
    let locale_data = brink_format::read_inkl(&inkl_bytes).ok()?;

    let (_, initial_context) = FlowInstance::new_at_root(&program);
    let world = app.world_mut();
    let program_h = world
        .resource_mut::<Assets<ProgramAsset>>()
        .add(ProgramAsset {
            program,
            initial_context,
            effect_rows: Vec::new(),
        });
    let base_h = world
        .resource_mut::<Assets<LineTablesAsset>>()
        .add(LineTablesAsset {
            tables: base_tables,
        });
    let story_h = world
        .resource_mut::<Assets<BrinkStoryAsset>>()
        .add(BrinkStoryAsset {
            program: program_h,
            line_tables: base_h,
        });
    let locale_h = world
        .resource_mut::<Assets<LocaleAsset>>()
        .add(LocaleAsset { data: locale_data });
    Some((story_h, locale_h))
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

fn transcript_text(app: &App, flow: Entity) -> String {
    app.world()
        .entity(flow)
        .get::<BrinkTranscript<()>>()
        .map(BrinkTranscript::text)
        .unwrap_or_default()
}
