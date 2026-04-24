//! Story-wide mutable state: globals, visit/turn counts, RNG.

use std::marker::PhantomData;

use bevy_ecs::resource::Resource;
use brink_runtime::Context;

/// The story-wide mutable state (`Context`) for a story identified by the
/// marker `M`.
///
/// This is what inklecate calls "the story state" — globals, visit counts,
/// turn counts, and RNG seed. All flows for this marker read and write
/// against this single shared resource by default.
///
/// If you want fork/branch/rollback semantics, skip this resource and store
/// a [`brink_runtime::Context`] on the flow's component directly — the
/// runtime's step functions take `&mut Context` regardless of where it lives.
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
