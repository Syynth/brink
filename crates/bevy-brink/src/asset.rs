//! Asset types and loaders for compiled brink stories.

use std::marker::PhantomData;

use bevy_asset::{Asset, AssetLoader, Handle, LoadContext, io::Reader};
use bevy_ecs::component::Component;
use bevy_reflect::TypePath;
use brink_format::LineEntry;
use brink_runtime::{
    Context, FallbackHandler, FastRng, FlowInstance, Line, Program, RuntimeError, StoryStatus,
};
use serde::{Deserialize, Serialize};

/// The immutable bytecode portion of a compiled story — what the VM
/// actually executes.
///
/// Produced as a labeled subasset by [`InkbLoader`] (and the `.ink`
/// source loader) under the label `program`. Reference it through
/// [`BrinkStoryAsset::program`] or load it directly via the labeled
/// path `path.inkb#program`.
#[derive(Asset, TypePath)]
pub struct ProgramAsset {
    pub program: Program,
}

/// The localized line-table portion of a compiled story — the swappable
/// rendering data.
///
/// Every `.inkb` carries its source-language line tables embedded; the
/// loader splits them out as their own asset so future hot-reload
/// machinery can update tables independently of the program. Additional
/// `.inkl` overlays will load as standalone `LineTablesAsset`s when that
/// loader lands.
///
/// Loaded as a labeled subasset under the label `line_tables` from
/// [`InkbLoader`], or directly via `path.inkb#line_tables`.
#[derive(Asset, TypePath)]
pub struct LineTablesAsset {
    pub tables: Vec<Vec<LineEntry>>,
}

/// The starting [`Context`] that results from running the story's init
/// pass at load time — globals declared with `VAR`/`CONST`/`LIST` and
/// any free-floating top-of-file setup are evaluated once during load,
/// and the resulting state is captured here.
///
/// When a flow is spawned via [`BrinkFlowRequest`](crate::BrinkFlowRequest),
/// the fulfillment system uses this snapshot to seed
/// [`BrinkGlobals`](crate::BrinkGlobals) the first time. Subsequent
/// flow spawns reuse the existing globals — they don't replay init.
///
/// When [`InkLoaderSettings::run_init`] is `false`, this contains the
/// raw post-`global_defaults` Context with no execution applied, so
/// callers that want to perform their own init can do so.
///
/// Loaded as a labeled subasset under the label `initial_globals`.
#[derive(Asset, TypePath)]
pub struct InitialGlobalsAsset {
    pub context: Context,
}

/// Top-level "story" asset — a thin bundle pairing the three
/// labeled subassets ([`ProgramAsset`], [`LineTablesAsset`],
/// [`InitialGlobalsAsset`]) that together describe a loaded story.
///
/// `.inkb` and `.ink` loaders emit this. Consumers usually don't need
/// to load the labeled subassets directly — they spawn an entity with
/// a [`BrinkFlowRequest`](crate::BrinkFlowRequest) carrying a
/// `Handle<BrinkStoryAsset>` and let the fulfillment system wire
/// everything up.
#[derive(Asset, TypePath)]
pub struct BrinkStoryAsset {
    pub program: Handle<ProgramAsset>,
    pub line_tables: Handle<LineTablesAsset>,
    pub initial_globals: Handle<InitialGlobalsAsset>,
}

/// Loader-time configuration for both [`InkbLoader`] and the `.ink`
/// source loader.
///
/// Use [`AssetServer::load_with_settings`](bevy_asset::AssetServer::load_with_settings)
/// to override the defaults:
///
/// ```ignore
/// let handle: Handle<BrinkStoryAsset> = asset_server.load_with_settings(
///     "story.ink",
///     |s: &mut InkLoaderSettings| { s.run_init = false; },
/// );
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InkLoaderSettings {
    /// Run the story's init pass at load time to evaluate global
    /// declarations and any top-of-file setup, capturing the resulting
    /// `Context` as [`InitialGlobalsAsset`].
    ///
    /// Default: `true`. Set to `false` if your story's init code calls
    /// host-provided external functions that aren't registered yet at
    /// load time — in that case run init yourself once your bindings
    /// are ready.
    pub run_init: bool,
    /// Safety cap on the number of VM steps the init pass may execute
    /// before erroring out. Guards against runaway bytecode.
    pub init_step_limit: usize,
}

impl Default for InkLoaderSettings {
    fn default() -> Self {
        Self {
            run_init: true,
            init_step_limit: 10_000,
        }
    }
}

/// Asset loader for `.inkb` (compiled bytecode) files.
///
/// Reads the bytes, decodes via [`brink_format::read_inkb`], links via
/// [`brink_runtime::link`], optionally runs the init pass, and emits
/// labeled subassets (`#program`, `#line_tables`, `#initial_globals`)
/// bundled in the returned [`BrinkStoryAsset`].
#[derive(Default, TypePath)]
pub struct InkbLoader;

/// Errors that can occur loading an `.inkb` file.
#[derive(Debug, thiserror::Error)]
pub enum InkbLoaderError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid .inkb: {0:?}")]
    Decode(brink_format::DecodeError),
    #[error("link error: {0}")]
    Link(#[from] RuntimeError),
    #[error("init pass failed: {0}")]
    Init(#[from] InitError),
}

impl From<brink_format::DecodeError> for InkbLoaderError {
    fn from(err: brink_format::DecodeError) -> Self {
        Self::Decode(err)
    }
}

impl AssetLoader for InkbLoader {
    type Asset = BrinkStoryAsset;
    type Settings = InkLoaderSettings;
    type Error = InkbLoaderError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let story_data = brink_format::read_inkb(&bytes)?;
        let (program, tables) = brink_runtime::link(&story_data)?;
        let initial_context = run_init_pass(&program, &tables, settings)?;
        Ok(emit_story_assets(
            load_context,
            program,
            tables,
            initial_context,
        ))
    }

    fn extensions(&self) -> &[&str] {
        &["inkb"]
    }
}

/// Run the story's init pass: spawn a fresh flow at root, step until
/// the first terminal Line, and capture the resulting `Context`.
///
/// When `settings.run_init` is `false`, returns the un-executed Context
/// straight from `program.global_defaults()` — useful for stories whose
/// init code depends on host-provided externals not yet registered.
pub(crate) fn run_init_pass(
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    settings: &InkLoaderSettings,
) -> Result<Context, InitError> {
    let (mut flow, mut context) = FlowInstance::new_at_root(program);
    if !settings.run_init {
        return Ok(context);
    }

    for _ in 0..settings.init_step_limit {
        let line = flow
            .step_single_line::<FastRng>(
                program,
                line_tables,
                &mut context,
                &FallbackHandler,
                None,
            )
            .map_err(InitError::Runtime)?;

        // Stop at the first non-Text line — that's where init naturally
        // hands control back to the player (a choice, a Done, or End).
        if !matches!(line, Line::Text { .. }) {
            return Ok(context);
        }
        // Some stories never reach a terminal but go Active → Ended; bail.
        if matches!(flow.status(), StoryStatus::Ended) {
            return Ok(context);
        }
    }
    Err(InitError::StepLimitExceeded(settings.init_step_limit))
}

/// Errors from the init pass.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("runtime error during init: {0}")]
    Runtime(RuntimeError),
    #[error("init pass exceeded step limit ({0}); story may have an infinite loop")]
    StepLimitExceeded(usize),
}

/// Emit the three labeled subassets (`#program`, `#line_tables`,
/// `#initial_globals`) and return the bundle holding their handles.
pub(crate) fn emit_story_assets(
    load_context: &mut LoadContext<'_>,
    program: Program,
    tables: Vec<Vec<LineEntry>>,
    initial_context: Context,
) -> BrinkStoryAsset {
    let program = load_context.add_labeled_asset("program".to_string(), ProgramAsset { program });
    let line_tables =
        load_context.add_labeled_asset("line_tables".to_string(), LineTablesAsset { tables });
    let initial_globals = load_context.add_labeled_asset(
        "initial_globals".to_string(),
        InitialGlobalsAsset {
            context: initial_context,
        },
    );
    BrinkStoryAsset {
        program,
        line_tables,
        initial_globals,
    }
}

/// Component holding the `Handle<ProgramAsset>` a [`BrinkFlow<M>`](crate::BrinkFlow)
/// executes against.
///
/// In Bevy 0.18 `Handle<T>` is no longer a `Component` directly, so
/// flow entities need a wrapper to associate a flow with its program.
/// The fulfillment system inserts this when consuming a
/// [`BrinkFlowRequest`](crate::BrinkFlowRequest); manual usage is
/// possible but rare.
#[derive(Component)]
pub struct BrinkProgram<M: Send + Sync + 'static = ()> {
    pub handle: Handle<ProgramAsset>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkProgram<M> {
    #[must_use]
    pub fn new(handle: Handle<ProgramAsset>) -> Self {
        Self {
            handle,
            _marker: PhantomData,
        }
    }
}
