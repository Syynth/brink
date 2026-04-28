//! Story-wide and per-flow `Context` state.
//!
//! - [`BrinkGlobals<M>`] is a `Resource` — the "save data" snapshot for
//!   marker `M`. New flows seed their `Context` from this; consumers
//!   commit a flow's `Context` back to it explicitly.
//! - [`BrinkContext<M>`] is a `Component` — the in-flight `Context` of
//!   a single flow on its entity. The flow advances against this; it
//!   only touches `BrinkGlobals` when explicitly committed.

use std::marker::PhantomData;

use bevy_ecs::component::Component;
use bevy_ecs::resource::Resource;
use brink_runtime::Context;

/// The "save data" `Context` for a story identified by marker `M`.
///
/// Holds globals, visit/turn counts, RNG seed — the canonical state
/// new flows seed from. The plugin auto-inserts this on first
/// fulfillment, seeded from [`ProgramAsset::initial_context`](crate::ProgramAsset).
/// After that the plugin doesn't touch it during play; consumers commit
/// changes from a flow's [`BrinkContext`] back into it explicitly via
/// the `commit_*` helpers.
#[derive(Resource)]
pub struct BrinkGlobals<M: Send + Sync + 'static = ()> {
    pub inner: Context,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkGlobals<M> {
    /// Wrap a freshly-created [`Context`] (e.g. from
    /// [`FlowInstance::new_at_root`](brink_runtime::FlowInstance::new_at_root))
    /// in a Bevy `Resource`.
    #[must_use]
    pub fn new(context: Context) -> Self {
        Self {
            inner: context,
            _marker: PhantomData,
        }
    }
}

/// The in-flight `Context` of a single flow on its entity.
///
/// Inserted by `fulfill_flow_requests` alongside [`BrinkFlow`](crate::BrinkFlow).
/// The flow's `step_one`/`advance_until_terminal`/`choose` methods read
/// and write this `Context` directly. Multiple concurrent flows each
/// have their own — globals are NOT auto-shared. Use
/// [`BrinkGlobals::commit_*`] to merge a flow's changes back into the
/// shared "save data" resource.
#[derive(Component)]
pub struct BrinkContext<M: Send + Sync + 'static = ()> {
    pub inner: Context,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkContext<M> {
    #[must_use]
    pub fn new(context: Context) -> Self {
        Self {
            inner: context,
            _marker: PhantomData,
        }
    }
}
