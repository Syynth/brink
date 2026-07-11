//! Context access trait and write observer.
//!
//! The `ContextAccess` trait provides the mutable state interface that the VM
//! and orchestration use. [`World`](crate::world::World) implements it
//! directly (zero-cost, monomorphized), as does the routing view
//! ([`ContextView`](crate::world::ContextView)) that composes `World` with
//! the (currently empty) per-flow `FlowLocal` layer. `ObservedContext` wraps
//! any `ContextAccess` implementor and fires `WriteObserver` callbacks on
//! every mutation.

use alloc::vec::Vec;

use brink_format::{DefinitionId, Value};

use crate::rng::StoryRng;

/// Trait for accessing and mutating story execution state.
///
/// This is the interface between the VM and the mutable story state.
/// [`World`](crate::world::World) implements it directly, as does the
/// [`ContextView`](crate::world::ContextView) routing view.
/// [`ObservedContext`] wraps an implementor and fires [`WriteObserver`]
/// callbacks on mutations. Consumers can also implement this trait
/// themselves to plug in custom observers (e.g. bevy events) or alternate
/// storage backends.
///
/// This does NOT include `Program`, resolver, or any immutable data — it's
/// purely the mutable state surface.
pub trait ContextAccess {
    fn global(&self, idx: u32) -> &Value;
    fn set_global(&mut self, idx: u32, value: Value);

    fn visit_count(&self, id: DefinitionId) -> u32;
    fn increment_visit(&mut self, id: DefinitionId);
    /// Set a visit count directly, rather than incrementing it. Used by
    /// [`crate::load_state`] to reconcile a durable save, whose entries carry
    /// absolute counts rather than deltas.
    fn set_visit_count(&mut self, id: DefinitionId, count: u32);

    fn turn_count(&self, id: DefinitionId) -> Option<u32>;
    fn set_turn_count(&mut self, id: DefinitionId, turn: u32);

    fn turn_index(&self) -> u32;
    fn increment_turn_index(&mut self);
    /// Set the turn index directly, rather than incrementing it. Used by
    /// [`crate::load_state`] to restore a saved turn index.
    fn set_turn_index(&mut self, index: u32);

    fn rng_seed(&self) -> i32;
    fn set_rng_seed(&mut self, seed: i32);

    fn previous_random(&self) -> i32;
    fn set_previous_random(&mut self, val: i32);

    fn next_random<R: StoryRng>(&self, seed: i32) -> i32;
    fn random_sequence<R: StoryRng>(&self, seed: i32, count: usize) -> Vec<i32>;
}

// ── WriteObserver ──────────────────────────────────────────────────────────

/// Observer for state mutations during story execution.
///
/// Implement this trait to intercept every write the VM makes to the story
/// state. All methods have default no-op implementations. The observer
/// receives the *new* value only — no old-value cloning is performed.
#[expect(unused_variables)]
pub trait WriteObserver {
    fn on_set_global(&mut self, idx: u32, value: &Value) {}
    fn on_increment_visit(&mut self, id: DefinitionId, new_count: u32) {}
    fn on_set_visit_count(&mut self, id: DefinitionId, count: u32) {}
    fn on_set_turn_count(&mut self, id: DefinitionId, turn: u32) {}
    fn on_increment_turn_index(&mut self, new_value: u32) {}
    fn on_set_turn_index(&mut self, index: u32) {}
    fn on_set_rng_seed(&mut self, new_seed: i32) {}
    fn on_set_previous_random(&mut self, new_val: i32) {}
}

// ── ObservedContext ────────────────────────────────────────────────────────

/// A `ContextAccess` wrapper that delegates to an inner `ContextAccess`
/// implementor (typically [`World`](crate::world::World) or the
/// [`ContextView`](crate::world::ContextView) routing view) and notifies a
/// `WriteObserver` on every mutation.
///
/// Generic over the wrapped implementor so it composes with the routing
/// view: `ObservedContext::new(&mut ContextView::new(&mut world, &mut
/// local), observer)` observes exactly what the VM sees, regardless of how
/// many layers the routing view has behind it.
pub struct ObservedContext<'a, 'o, C: ContextAccess> {
    context: &'a mut C,
    observer: &'o mut dyn WriteObserver,
}

impl<'a, 'o, C: ContextAccess> ObservedContext<'a, 'o, C> {
    pub fn new(context: &'a mut C, observer: &'o mut dyn WriteObserver) -> Self {
        Self { context, observer }
    }
}

impl<C: ContextAccess> ContextAccess for ObservedContext<'_, '_, C> {
    #[inline]
    fn global(&self, idx: u32) -> &Value {
        self.context.global(idx)
    }

    #[inline]
    fn set_global(&mut self, idx: u32, value: Value) {
        self.context.set_global(idx, value.clone());
        self.observer.on_set_global(idx, &value);
    }

    #[inline]
    fn visit_count(&self, id: DefinitionId) -> u32 {
        self.context.visit_count(id)
    }

    #[inline]
    fn increment_visit(&mut self, id: DefinitionId) {
        self.context.increment_visit(id);
        let new_count = self.context.visit_count(id);
        self.observer.on_increment_visit(id, new_count);
    }

    #[inline]
    fn set_visit_count(&mut self, id: DefinitionId, count: u32) {
        self.context.set_visit_count(id, count);
        self.observer.on_set_visit_count(id, count);
    }

    #[inline]
    fn turn_count(&self, id: DefinitionId) -> Option<u32> {
        self.context.turn_count(id)
    }

    #[inline]
    fn set_turn_count(&mut self, id: DefinitionId, turn: u32) {
        self.context.set_turn_count(id, turn);
        self.observer.on_set_turn_count(id, turn);
    }

    #[inline]
    fn turn_index(&self) -> u32 {
        self.context.turn_index()
    }

    #[inline]
    fn increment_turn_index(&mut self) {
        self.context.increment_turn_index();
        self.observer
            .on_increment_turn_index(self.context.turn_index());
    }

    #[inline]
    fn set_turn_index(&mut self, index: u32) {
        self.context.set_turn_index(index);
        self.observer.on_set_turn_index(index);
    }

    #[inline]
    fn rng_seed(&self) -> i32 {
        self.context.rng_seed()
    }

    #[inline]
    fn set_rng_seed(&mut self, seed: i32) {
        self.context.set_rng_seed(seed);
        self.observer.on_set_rng_seed(seed);
    }

    #[inline]
    fn previous_random(&self) -> i32 {
        self.context.previous_random()
    }

    #[inline]
    fn set_previous_random(&mut self, val: i32) {
        self.context.set_previous_random(val);
        self.observer.on_set_previous_random(val);
    }

    #[inline]
    fn next_random<R: StoryRng>(&self, seed: i32) -> i32 {
        self.context.next_random::<R>(seed)
    }

    fn random_sequence<R: StoryRng>(&self, seed: i32, count: usize) -> Vec<i32> {
        self.context.random_sequence::<R>(seed, count)
    }
}
