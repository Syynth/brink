//! Asset types and loaders for compiled brink stories.

use std::marker::PhantomData;

use bevy_asset::{Asset, AssetLoader, Handle, LoadContext, io::Reader};
use bevy_ecs::component::Component;
use bevy_reflect::TypePath;
use brink_format::LineEntry;
use brink_runtime::{Program, RuntimeError};

/// The immutable bytecode portion of a compiled story — what the VM
/// actually executes.
///
/// Produced as a labeled subasset by [`InkbLoader`] (and by the upcoming
/// `.ink` source loader) under the label `program`. Reference it through
/// [`BrinkStoryAsset::program`] or load it directly via the labeled path
/// `path.inkb#program`.
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

/// Top-level "story" asset — a thin bundle pairing a [`ProgramAsset`]
/// handle with its companion [`LineTablesAsset`] handle.
///
/// `.inkb` (and the upcoming `.ink` source loader) produce this as their
/// main asset. The compiler and linker emit program and line tables
/// together, so consumers want to refer to "the story" — but they also
/// want to be able to swap line tables independently for locale changes
/// without forcing program reloads, which is why the two pieces are
/// separate sub-assets.
#[derive(Asset, TypePath)]
pub struct BrinkStoryAsset {
    pub program: Handle<ProgramAsset>,
    pub line_tables: Handle<LineTablesAsset>,
}

/// Asset loader for `.inkb` (compiled bytecode) files.
///
/// Reads the bytes, decodes via [`brink_format::read_inkb`], links via
/// [`brink_runtime::link`], registers the resulting `Program` and line
/// tables as labeled subassets (`#program` and `#line_tables`), and
/// returns a [`BrinkStoryAsset`] bundling handles to both.
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

        let program_handle =
            load_context.add_labeled_asset("program".to_string(), ProgramAsset { program });
        let line_tables_handle = load_context
            .add_labeled_asset("line_tables".to_string(), LineTablesAsset { tables });

        Ok(BrinkStoryAsset {
            program: program_handle,
            line_tables: line_tables_handle,
        })
    }

    fn extensions(&self) -> &[&str] {
        &["inkb"]
    }
}

/// Component holding the `Handle<ProgramAsset>` a [`BrinkFlow<M>`](crate::BrinkFlow)
/// executes against.
///
/// In Bevy 0.18 `Handle<T>` is no longer a `Component` directly, so flow
/// entities need a wrapper to associate a flow with its program. Spawn this
/// alongside `BrinkFlow<M>`:
///
/// ```ignore
/// // Once the BrinkStoryAsset has resolved, grab the program handle:
/// let bundle = story_assets.get(&story_handle).unwrap();
/// commands.spawn((
///     BrinkFlow::<MyStory>::new(flow_state),
///     BrinkProgram::<MyStory>::new(bundle.program.clone()),
/// ));
/// ```
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
