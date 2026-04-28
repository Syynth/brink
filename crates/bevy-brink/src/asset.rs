//! Asset types and loaders for compiled brink stories.

use std::marker::PhantomData;

use bevy_asset::{Asset, AssetLoader, Handle, LoadContext, io::Reader};
use bevy_ecs::component::Component;
use bevy_reflect::TypePath;
use brink_format::LineEntry;
use brink_runtime::{Context, FlowInstance, Program, RuntimeError};

/// The immutable bytecode portion of a compiled story — what the VM
/// actually executes — together with the fresh starting [`Context`]
/// (globals seeded from `VAR`/`CONST`/`LIST` defaults; zero visit and
/// turn counts).
///
/// `initial_context` is read-only "fresh start" state. Consumers use
/// it to seed [`BrinkGlobals`](crate::BrinkGlobals) on first
/// fulfillment, and can commit it back later for a "new game" reset
/// (`globals.commit_from(&program.initial_context)`).
///
/// No execution happens to produce this — it's a pure function of the
/// linked [`Program`]'s declarations. Stories with free-floating
/// top-of-file setup (`~ initialize_save_data()` etc.) need a flow at
/// root to advance through that code; the runtime doesn't pre-run it.
///
/// Produced as a labeled subasset by [`InkbLoader`] (and the `.ink`
/// source loader) under the label `program`. Reference it through
/// [`BrinkStoryAsset::program`] or load it directly via the labeled
/// path `path.inkb#program`.
#[derive(Asset, TypePath)]
pub struct ProgramAsset {
    pub program: Program,
    pub initial_context: Context,
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

/// Top-level "story" asset — a thin bundle pairing the two
/// labeled subassets ([`ProgramAsset`], [`LineTablesAsset`]) that
/// together describe a loaded story.
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
}

/// Asset loader for `.inkb` (compiled bytecode) files.
///
/// Reads the bytes, decodes via [`brink_format::read_inkb`], links via
/// [`brink_runtime::link`], computes the fresh starting [`Context`]
/// from the program's declarations, and emits labeled subassets
/// (`#program`, `#line_tables`) bundled in the returned
/// [`BrinkStoryAsset`].
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
}

impl From<brink_format::DecodeError> for InkbLoaderError {
    fn from(err: brink_format::DecodeError) -> Self {
        Self::Decode(err)
    }
}

impl AssetLoader for InkbLoader {
    type Asset = BrinkStoryAsset;
    type Settings = ();
    type Error = InkbLoaderError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let story_data = brink_format::read_inkb(&bytes)?;
        let (program, tables) = brink_runtime::link(&story_data)?;
        Ok(emit_story_assets(load_context, program, tables))
    }

    fn extensions(&self) -> &[&str] {
        &["inkb"]
    }
}

/// Compute the fresh starting [`Context`] for a program — globals seeded
/// from `VAR`/`CONST`/`LIST` defaults, zero visit and turn counts. No
/// execution; pure function of the linked program.
pub(crate) fn fresh_context(program: &Program) -> Context {
    // FlowInstance::new_at_root constructs both a flow and a fresh
    // Context; we only want the Context here.
    let (_, context) = FlowInstance::new_at_root(program);
    context
}

/// Emit the two labeled subassets (`#program`, `#line_tables`) and
/// return the bundle holding their handles. The fresh starting
/// [`Context`] is computed and stored inline on `ProgramAsset`.
pub(crate) fn emit_story_assets(
    load_context: &mut LoadContext<'_>,
    program: Program,
    tables: Vec<Vec<LineEntry>>,
) -> BrinkStoryAsset {
    let initial_context = fresh_context(&program);
    let program = load_context.add_labeled_asset(
        "program".to_string(),
        ProgramAsset {
            program,
            initial_context,
        },
    );
    let line_tables =
        load_context.add_labeled_asset("line_tables".to_string(), LineTablesAsset { tables });
    BrinkStoryAsset {
        program,
        line_tables,
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

#[cfg(test)]
mod fresh_context_tests {
    use crate::test_support::compile_test_story;

    /// `VAR` defaults are a link-time concern (`Program::global_defaults`),
    /// not an init-pass concern. The fresh Context picks them up
    /// without any execution.
    #[test]
    fn fresh_context_picks_up_var_defaults() {
        let source = "VAR score = 42\n=== start ===\nHello.\n* [Continue] -> END\n";
        let (program, tables, ctx) = compile_test_story(source);

        let mut score_value = None;
        for slot in 0..program.global_count() {
            if program.global_name(slot) == Some("score") {
                score_value = Some(ctx.globals[slot as usize].clone());
            }
        }
        assert!(score_value.is_some(), "score global should exist");
        assert!(
            matches!(score_value.unwrap(), brink_format::Value::Int(42)),
            "score should be 42 from the VAR default"
        );
        assert!(!tables.is_empty(), "compiled story should have line tables");
    }
}
