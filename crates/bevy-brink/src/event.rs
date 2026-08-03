//! Observer events fired by the runtime as flows advance.
//!
//! Bevy 0.19 distinguishes two event flavors:
//!
//! - **Observer events** (`Event` derive, `commands.trigger(...)`,
//!   `app.add_observer(...)`) — synchronous fire-and-react, no buffered
//!   queue. We use these.
//! - **Messages** (`Message` derive, `MessageReader`/`MessageWriter`) —
//!   buffered cross-tick pubsub. Heavier than we need for "the flow
//!   produced a line just now."
//!
//! All events here are split by `Step` variant so observers can target
//! exactly the situation they care about (no inline `match` on a Step
//! enum). Pattern: bladeink's `DeliverLine` / `DeliverChoices`.
//!
//! For consumers that want the historical view rather than per-event
//! reaction, see [`brink_runtime::FlowInstance::transcript`] — the
//! flow keeps an append-only log of every part it emits, which can be
//! re-rendered against any line tables (locale swap, etc.) via
//! [`brink_runtime::transcript::render_transcript`].

use std::marker::PhantomData;

use bevy_ecs::entity::Entity;
use bevy_ecs::event::EntityEvent;
use brink_runtime::Choice;

/// Fired when a flow produces a `Step::Line` — mid-stream content; more
/// may follow on subsequent steps. Typewriter-style UIs accumulate;
/// click-to-continue UIs concatenate until a terminal event arrives.
#[derive(EntityEvent)]
pub struct BrinkLineDelivered<M: Send + Sync + 'static = ()> {
    pub entity: Entity,
    pub text: String,
    pub tags: Vec<String>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkLineDelivered<M> {
    pub(crate) fn new(entity: Entity, text: String, tags: Vec<String>) -> Self {
        Self {
            entity,
            text,
            tags,
            _marker: PhantomData,
        }
    }
}

/// Fired when a flow reaches a `Step::Choices` — pick one via
/// [`BrinkFlow::choose`](crate::BrinkFlow::choose) (or
/// [`choose_recording`](crate::BrinkFlow::choose_recording) in dev
/// builds for replay-after-hot-reload).
#[derive(EntityEvent)]
pub struct BrinkChoicesPresented<M: Send + Sync + 'static = ()> {
    pub entity: Entity,
    /// Always empty — terminals carry no payload of their own
    /// (`docs/prose-dialect-spec.md` §7, RULED). Any trailing content
    /// already arrived as its own preceding [`BrinkLineDelivered`] event.
    pub text: String,
    /// Always empty; see [`Self::text`].
    pub tags: Vec<String>,
    pub choices: Vec<Choice>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkChoicesPresented<M> {
    pub(crate) fn new(
        entity: Entity,
        text: String,
        tags: Vec<String>,
        choices: Vec<Choice>,
    ) -> Self {
        Self {
            entity,
            text,
            tags,
            choices,
            _marker: PhantomData,
        }
    }
}

/// Fired when a flow reaches `Step::Done` — this turn's output is
/// complete (the ink `-> DONE` instruction). The story is *not* over;
/// call advance again for the next turn.
#[derive(EntityEvent)]
pub struct BrinkTurnDone<M: Send + Sync + 'static = ()> {
    pub entity: Entity,
    /// Always empty — terminals carry no payload of their own
    /// (`docs/prose-dialect-spec.md` §7, RULED). Any trailing content
    /// already arrived as its own preceding [`BrinkLineDelivered`] event.
    pub text: String,
    /// Always empty; see [`Self::text`].
    pub tags: Vec<String>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkTurnDone<M> {
    pub(crate) fn new(entity: Entity, text: String, tags: Vec<String>) -> Self {
        Self {
            entity,
            text,
            tags,
            _marker: PhantomData,
        }
    }
}

/// Fired when a flow reaches `Step::End` — the story has permanently
/// ended (the ink `-> END` instruction). No more advance is meaningful.
#[derive(EntityEvent)]
pub struct BrinkStoryEnded<M: Send + Sync + 'static = ()> {
    pub entity: Entity,
    /// Always empty — terminals carry no payload of their own
    /// (`docs/prose-dialect-spec.md` §7, RULED). Any trailing content
    /// already arrived as its own preceding [`BrinkLineDelivered`] event.
    pub text: String,
    /// Always empty; see [`Self::text`].
    pub tags: Vec<String>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkStoryEnded<M> {
    pub(crate) fn new(entity: Entity, text: String, tags: Vec<String>) -> Self {
        Self {
            entity,
            text,
            tags,
            _marker: PhantomData,
        }
    }
}

/// Fired by the plugin's reload-replay system *before* it starts
/// rebuilding a flow against a freshly-reloaded program.
///
/// Consumers observe this to clear UI state (page text, pending
/// choices, etc.) so the subsequent stream of line/choice events from
/// replay populates fresh state instead of concatenating with what was
/// already on screen. Fires once per reloaded flow entity.
///
/// Available only with the `dev` feature.
#[cfg(feature = "dev")]
#[derive(EntityEvent)]
pub struct BrinkFlowReset<M: Send + Sync + 'static = ()> {
    pub entity: Entity,
    _marker: PhantomData<fn() -> M>,
}

#[cfg(feature = "dev")]
impl<M: Send + Sync + 'static> BrinkFlowReset<M> {
    pub(crate) fn new(entity: Entity) -> Self {
        Self {
            entity,
            _marker: PhantomData,
        }
    }
}
