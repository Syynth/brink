//! Shared test helpers used by inline `#[cfg(test)]` modules across
//! the crate. Compiled only under `cfg(test)`.
//!
//! Provides:
//! - `compile_test_story`: compile a small ink source into the trio of
//!   (Program, line_tables, initial_globals_context) needed to set up
//!   asset state in a test.
//! - `make_test_app` / `add_story_assets`: build a minimal Bevy `App`
//!   wired with `BrinkPlugin`, plus directly insert pre-built story
//!   assets so tests don't have to round-trip through the file-watcher
//!   loaders.

#![cfg(test)]

use bevy_app::App;
use bevy_asset::{AssetPlugin, Assets, Handle};
use brink_runtime::{Context, Program};

use crate::asset::{
    BrinkStoryAsset, InitialGlobalsAsset, InkLoaderSettings, LineTablesAsset, ProgramAsset,
    run_init_pass,
};

/// Compile an inline ink source through the full pipeline (compile +
/// link + run init pass) and return the artifacts needed to construct
/// a `BrinkStoryAsset` in a test.
///
/// Panics on any failure — tests should provide valid ink sources.
#[expect(
    clippy::expect_used,
    reason = "test helper: panic on bad fixtures is fine"
)]
pub fn compile_test_story(source: &str) -> (Program, Vec<Vec<brink_format::LineEntry>>, Context) {
    let output = brink_compiler::compile("test.ink", |path| {
        if path == "test.ink" {
            Ok(source.to_string())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("unexpected include: {path}"),
            ))
        }
    })
    .expect("test fixture should compile");
    let (program, tables) =
        brink_runtime::link(&output.data).expect("test fixture should link");
    let initial_globals = run_init_pass(&program, &tables, &InkLoaderSettings::default())
        .expect("test fixture init pass should succeed");
    (program, tables, initial_globals)
}

/// Build an `App` with the minimum plugins needed to exercise
/// `BrinkPlugin<()>`'s systems without spinning up a full Bevy game.
pub fn make_test_app() -> App {
    let mut app = App::new();
    app.add_plugins(AssetPlugin::default());
    app.add_plugins(crate::BrinkPlugin::<()>::default());
    app
}

/// Insert pre-built story assets directly into `Assets<...>` and
/// return a `Handle<BrinkStoryAsset>` pointing at them.
///
/// Lets tests bypass the loaders entirely so they can focus on the
/// fulfillment / replay logic.
pub fn add_story_assets(
    app: &mut App,
    program: Program,
    tables: Vec<Vec<brink_format::LineEntry>>,
    initial_context: Context,
) -> Handle<BrinkStoryAsset> {
    let world = app.world_mut();
    let program_handle = world
        .resource_mut::<Assets<ProgramAsset>>()
        .add(ProgramAsset { program });
    let tables_handle = world
        .resource_mut::<Assets<LineTablesAsset>>()
        .add(LineTablesAsset { tables });
    let initial_globals_handle = world
        .resource_mut::<Assets<InitialGlobalsAsset>>()
        .add(InitialGlobalsAsset {
            context: initial_context,
        });
    world.resource_mut::<Assets<BrinkStoryAsset>>().add(BrinkStoryAsset {
        program: program_handle,
        line_tables: tables_handle,
        initial_globals: initial_globals_handle,
    })
}
