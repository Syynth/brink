//! Messages emitted by the flow-advance system.

use std::marker::PhantomData;

use bevy_ecs::entity::Entity;
use bevy_ecs::message::Message;
use brink_runtime::Line;

/// Emitted by [`advance_flows`](crate::system::advance_flows) for every line
/// produced on each tick — one message per [`Line`] returned by
/// [`brink_runtime::FlowInstance::step_single_line`].
///
/// Consumers read these via `MessageReader<BrinkLineMessage<M>>` to drive UI,
/// dialogue widgets, audio, etc. The `entity` field identifies which flow
/// produced the line so a single reader can route across many flows.
///
/// (Bevy 0.18 terminology: this uses the `Message` trait for buffered
/// reader/writer pubsub — what older bevy versions called `Event`.)
#[derive(Message)]
pub struct BrinkLineMessage<M: Send + Sync + 'static = ()> {
    pub entity: Entity,
    pub line: Line,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkLineMessage<M> {
    pub(crate) fn new(entity: Entity, line: Line) -> Self {
        Self {
            entity,
            line,
            _marker: PhantomData,
        }
    }
}
