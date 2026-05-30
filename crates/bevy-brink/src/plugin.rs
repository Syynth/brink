//! The Bevy plugin for brink ink stories.

use std::marker::PhantomData;

use bevy_app::{App, Plugin, Update};
use bevy_asset::AssetApp;

use crate::asset::{BrinkStoryAsset, InkbLoader, LineTablesAsset, ProgramAsset};
use crate::request::fulfill_flow_requests;

/// A Bevy plugin that registers brink story types, messages, and asset
/// loaders for a single story instance identified by the marker type `M`.
///
/// The default `M = ()` suits the common single-story case. Declare your
/// own marker types (any `Send + Sync + 'static` ZST works) when you need
/// multiple concurrent stories in one app — each gets its own
/// `BrinkGlobals<M>` resource and `BrinkFlow<M>`/`BrinkContext<M>`/
/// `BrinkLocale<M>` components, monomorphized to distinct Bevy types
/// with no runtime overhead.
///
/// Adding `BrinkPlugin<M>` also ensures [`BrinkAssetsPlugin`] is added
/// once to the app (for shared asset types that don't depend on `M`).
///
/// **This plugin does not register an auto-advance system.** Most games
/// drive advancement from input or game-state events, not every tick.
/// Apps that want per-tick advancement can register
/// [`advance_flows`](crate::advance_flows) themselves:
///
/// ```ignore
/// app.add_systems(Update, advance_flows::<MyStory>);
/// ```
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
        app.add_systems(Update, fulfill_flow_requests::<M>);
        // Auto-render BrinkTranscript<M> for any flow that has it.
        // No-op for flows that don't (the query just yields nothing).
        app.add_systems(Update, crate::transcript::refresh_transcripts::<M>);
        #[cfg(feature = "dev")]
        app.add_systems(Update, crate::replay::replay_on_reload::<M>);
        #[cfg(debug_assertions)]
        app.add_systems(Update, crate::request::warn_post_fulfillment_mutations::<M>);
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
        app.init_asset::<BrinkStoryAsset>();
        app.init_asset::<ProgramAsset>();
        app.init_asset::<LineTablesAsset>();
        app.init_asset_loader::<InkbLoader>();
        #[cfg(feature = "dev")]
        app.init_asset_loader::<crate::source_loader::InkLoader>();
    }
}
