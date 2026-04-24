//! Asset types and loaders for compiled brink stories.

use std::marker::PhantomData;

use bevy_asset::{Asset, AssetLoader, Handle, LoadContext, io::Reader};
use bevy_ecs::component::Component;
use bevy_reflect::TypePath;
use brink_format::LineEntry;
use brink_runtime::{Program, RuntimeError};

/// A loaded, linked brink program: the immutable bytecode plus its base
/// (unlocalized) line tables. One `ProgramAsset` is typically shared across
/// many flow entities.
///
/// Loaded by [`ProgramLoader`] from `.inkb` files. For locale overlays,
/// apply a `.inkl` to `base_line_tables` via
/// [`brink_runtime::apply_locale`] and store the result in
/// [`BrinkLineTables`](crate::BrinkLineTables).
#[derive(Asset, TypePath)]
pub struct ProgramAsset {
    pub program: Program,
    pub base_line_tables: Vec<Vec<LineEntry>>,
}

/// Asset loader for `.inkb` (compiled bytecode) files.
///
/// Reads the bytes, decodes via [`brink_format::read_inkb`], links via
/// [`brink_runtime::link`], and wraps the result in a [`ProgramAsset`].
#[derive(Default, TypePath)]
pub struct ProgramLoader;

/// Errors that can occur loading an `.inkb` file.
#[derive(Debug, thiserror::Error)]
pub enum ProgramLoaderError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid .inkb: {0:?}")]
    Decode(brink_format::DecodeError),
    #[error("link error: {0}")]
    Link(#[from] RuntimeError),
}

impl From<brink_format::DecodeError> for ProgramLoaderError {
    fn from(err: brink_format::DecodeError) -> Self {
        Self::Decode(err)
    }
}

impl AssetLoader for ProgramLoader {
    type Asset = ProgramAsset;
    type Settings = ();
    type Error = ProgramLoaderError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let story_data = brink_format::read_inkb(&bytes)?;
        let (program, base_line_tables) = brink_runtime::link(&story_data)?;
        Ok(ProgramAsset {
            program,
            base_line_tables,
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
/// commands.spawn((
///     BrinkFlow::<MyStory>::new(flow_state),
///     BrinkProgram::<MyStory>::new(program_handle),
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
