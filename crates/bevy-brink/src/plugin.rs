//! The Bevy plugin for brink ink stories.

use std::marker::PhantomData;

use bevy_app::{App, Plugin, Update};
use bevy_asset::AssetApp;
use bevy_ecs::schedule::IntoScheduleConfigs as _;

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
        // Resolve deferred engine→ink calls (commands.brink_call). Exclusive
        // (needs &mut World to run query bindings), gated so it only runs
        // when a call is actually pending.
        app.add_systems(
            Update,
            crate::call::resolve_brink_calls::<M>.run_if(
                bevy_ecs::schedule::common_conditions::any_with_component::<
                    crate::call::BrinkCallRequest<M>,
                >,
            ),
        );
        // Service flows that paused on a pending external during normal
        // playback (a non-exclusive step_one yielded AwaitingQuery): resolve
        // world-access queries inline, fire BrinkExternalAwaited for async
        // (event) bindings, spawn tasks for task bindings. Exclusive (needs
        // &mut World), gated so it only runs when a flow is actually awaiting.
        app.add_systems(
            Update,
            crate::bindings::resolve_pending_externals::<M>
                .run_if(crate::bindings::any_flow_awaiting_external::<M>),
        );
        // Poll detached bind_brink_task futures; resolve the flow when one
        // finishes. Gated so it only runs while a task is pending.
        app.add_systems(
            Update,
            crate::async_bind::poll_brink_tasks::<M>.run_if(
                bevy_ecs::schedule::common_conditions::any_with_component::<
                    crate::async_bind::BrinkPendingTask<M>,
                >,
            ),
        );
        // Global, event-driven locale switching: the current-locale resource,
        // an observer that reconciles flows when it changes, and a catch-up
        // system for `.inkl`s that finish loading after a switch.
        app.init_resource::<crate::locale::BrinkCurrentLocale<M>>();
        app.init_resource::<crate::locale::LocalizedTablesCache<M>>();
        app.add_observer(crate::locale::on_locale_changed::<M>);
        app.add_systems(Update, crate::locale::catch_up_loaded_locales::<M>);
        #[cfg(feature = "dev")]
        app.init_resource::<crate::replay::BrinkReplayConfig>();
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
        app.init_asset::<crate::locale::LocaleAsset>();
        app.init_asset::<crate::brkt::TranscriptAsset>();
        app.init_asset_loader::<InkbLoader>();
        app.init_asset_loader::<crate::locale::InklLoader>();
        app.init_asset_loader::<crate::brkt::BrktLoader>();
        #[cfg(feature = "dev")]
        app.init_asset_loader::<crate::source_loader::InkLoader>();
    }
}
