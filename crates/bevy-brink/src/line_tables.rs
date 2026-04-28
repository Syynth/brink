//! Per-flow locale handle: which `LineTablesAsset` a flow renders against.

use std::marker::PhantomData;

use bevy_asset::Handle;
use bevy_ecs::component::Component;

use crate::asset::LineTablesAsset;

/// The active locale for a flow — a handle to the `LineTablesAsset`
/// whose strings and slot templates render this flow's output.
///
/// Inserted by `fulfill_flow_requests` alongside `BrinkProgram<M>`,
/// typically as part of the `BrinkStory<M>` bundle. To swap locale,
/// assign a different `Handle<LineTablesAsset>`; consumers can detect
/// the swap with `Changed<BrinkLocale<M>>` and re-render the flow's
/// transcript against the new tables (the runtime's transcript stores
/// structural references, not resolved strings).
///
/// Note: hot-reload of the *content* of an existing handle does NOT
/// fire `Changed` — Bevy's asset system updates the slot in place, so
/// the handle is stable. That path is signalled by
/// `AssetEvent::Modified<LineTablesAsset>` instead.
#[derive(Component)]
pub struct BrinkLocale<M: Send + Sync + 'static = ()> {
    pub handle: Handle<LineTablesAsset>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkLocale<M> {
    #[must_use]
    pub fn new(handle: Handle<LineTablesAsset>) -> Self {
        Self {
            handle,
            _marker: PhantomData,
        }
    }
}
