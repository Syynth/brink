//! Per-flow mutable state: call stacks, output buffer, pending choices.

use std::marker::PhantomData;

use bevy_ecs::component::Component;
use brink_runtime::FlowInstance;

/// A single live ink flow, attached to an entity. Holds the VM's per-flow
/// state: call stacks, output buffer, pending choices, and the accumulated
/// transcript.
///
/// Spawn one of these per active conversation. Systems advance the flow by
/// calling methods on `inner` against the shared [`BrinkGlobals`](crate::BrinkGlobals)
/// (or a per-flow `Context` if you're doing fork/branch) and the current
/// program from `Assets<ProgramAsset>`.
#[derive(Component)]
pub struct BrinkFlow<M: Send + Sync + 'static = ()> {
    pub inner: FlowInstance,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkFlow<M> {
    /// Wrap a freshly-constructed [`FlowInstance`] (e.g. from
    /// [`FlowInstance::new_at_root`](brink_runtime::FlowInstance::new_at_root))
    /// as a Bevy component ready to spawn.
    #[must_use]
    pub fn new(flow: FlowInstance) -> Self {
        Self {
            inner: flow,
            _marker: PhantomData,
        }
    }
}
