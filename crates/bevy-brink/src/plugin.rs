//! The Bevy plugin for brink ink stories.

use std::marker::PhantomData;

use bevy_app::{App, Plugin, Update};
use bevy_asset::AssetApp;

use crate::asset::{ProgramAsset, ProgramLoader};
use crate::event::BrinkLineMessage;
use crate::line_tables::BrinkLineTables;
use crate::system::advance_flows;

/// A Bevy plugin that registers brink story types, systems, and asset
/// loaders for a single story instance identified by the marker type `M`.
///
/// The default `M = ()` suits the common single-story case. Declare your
/// own marker types (any `Send + Sync + 'static` ZST works) when you need
/// multiple concurrent stories in one app — each gets its own `BrinkGlobals<M>`
/// resource, `BrinkFlow<M>` component, and `BrinkLineTables<M>` resource,
/// monomorphized to distinct Bevy types with no runtime overhead.
///
/// Adding `BrinkPlugin<M>` also ensures [`BrinkAssetsPlugin`] is added
/// once to the app (for shared asset types that don't depend on `M`).
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
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<BrinkAssetsPlugin>() {
            app.add_plugins(BrinkAssetsPlugin);
        }
        app.init_resource::<BrinkLineTables<M>>();
        app.add_message::<BrinkLineMessage<M>>();
        app.add_systems(Update, advance_flows::<M>);
    }
}

/// Registers asset types and loaders that are shared across all markers.
///
/// [`BrinkPlugin::build`] adds this automatically if it's not already
/// present, so you rarely need to add it manually — but you can if you
/// want the asset machinery without any marker-specific plumbing (e.g.
/// for a headless asset-processing binary).
pub struct BrinkAssetsPlugin;

impl Plugin for BrinkAssetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<ProgramAsset>();
        app.init_asset_loader::<ProgramLoader>();
    }
}
