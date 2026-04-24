//! Localized line tables — the swappable rendering data for a story.

use std::marker::PhantomData;

use bevy_ecs::resource::Resource;
use brink_format::LineEntry;

/// The active set of line tables (localized strings and slot templates)
/// for a story identified by marker `M`.
///
/// The runtime's step functions take `&[Vec<LineEntry>]`, and a flow's
/// append-only transcript stores structural references (`LineRef { ... }`)
/// rather than resolved strings — so swapping these tables re-renders
/// previously-produced output in the new locale without re-executing
/// the story.
///
/// One active locale per marker. If you need per-flow locale overrides or
/// per-locale `Asset`s, skip this resource and store a `Vec<Vec<LineEntry>>`
/// however you like — the runtime doesn't care where the slice comes from.
#[derive(Resource)]
pub struct BrinkLineTables<M: Send + Sync + 'static = ()> {
    pub tables: Vec<Vec<LineEntry>>,
    _marker: PhantomData<fn() -> M>,
}

// Manual Default avoids requiring `M: Default` (markers are often ZSTs
// without a derive).
impl<M: Send + Sync + 'static> Default for BrinkLineTables<M> {
    fn default() -> Self {
        Self {
            tables: Vec::new(),
            _marker: PhantomData,
        }
    }
}

impl<M: Send + Sync + 'static> BrinkLineTables<M> {
    /// Wrap a `Vec<Vec<LineEntry>>` (e.g. the base tables returned by
    /// [`brink_runtime::link`]) as a Bevy `Resource`.
    #[must_use]
    pub fn new(tables: Vec<Vec<LineEntry>>) -> Self {
        Self {
            tables,
            _marker: PhantomData,
        }
    }
}
