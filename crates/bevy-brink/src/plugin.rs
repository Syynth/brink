//! The Bevy plugin for brink ink stories.

use std::marker::PhantomData;

use bevy_app::{App, Plugin};

/// A Bevy plugin that registers brink story types, systems, and asset
/// loaders for a single story instance identified by the marker type `M`.
///
/// The default `M = ()` suits the common single-story case. Declare your
/// own marker types (any `Send + Sync + 'static` ZST works) when you need
/// multiple concurrent stories in one app — each gets its own `BrinkGlobals<M>`
/// resource, `BrinkFlow<M>` component, and `BrinkLineTables<M>` resource,
/// monomorphized to distinct Bevy types with no runtime overhead.
pub struct BrinkPlugin<M: Send + Sync + 'static = ()> {
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for BrinkPlugin<M> {
    fn default() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<M: Send + Sync + 'static> Plugin for BrinkPlugin<M> {
    fn build(&self, _app: &mut App) {
        // Registration of resources, components, events, asset loaders, and
        // systems will be added as each piece lands.
    }
}
