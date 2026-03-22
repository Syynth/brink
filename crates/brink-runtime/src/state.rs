//! Context access trait and write observer.
//!
//! The `ContextAccess` trait provides the mutable state interface that the VM
//! and orchestration use. `Context` implements it directly (zero-cost,
//! monomorphized). `ObservedContext` wraps a `Context` and fires
//! `WriteObserver` callbacks on every mutation.

use brink_format::{DefinitionId, Value};

use crate::rng::StoryRng;
use crate::story::Context;

/// Trait for accessing and mutating story execution state.
///
/// This is the interface between the VM and the mutable story state.
/// `Context` implements it directly. `ObservedContext` wraps a `Context`
/// and fires `WriteObserver` callbacks on mutations.
///
/// Unlike the deleted `StoryState` trait, this does NOT include `Program`,
/// resolver, or any immutable data — it's purely the mutable state surface.
pub(crate) trait ContextAccess {
    fn global(&self, idx: u32) -> &Value;
    fn set_global(&mut self, idx: u32, value: Value);

    fn visit_count(&self, id: DefinitionId) -> u32;
    fn increment_visit(&mut self, id: DefinitionId);

    fn turn_count(&self, id: DefinitionId) -> Option<u32>;
    fn set_turn_count(&mut self, id: DefinitionId, turn: u32);

    fn turn_index(&self) -> u32;
    fn increment_turn_index(&mut self);

    fn rng_seed(&self) -> i32;
    fn set_rng_seed(&mut self, seed: i32);

    fn previous_random(&self) -> i32;
    fn set_previous_random(&mut self, val: i32);

    fn next_random<R: StoryRng>(&self, seed: i32) -> i32;
    fn random_sequence<R: StoryRng>(&self, seed: i32, count: usize) -> Vec<i32>;
}

impl ContextAccess for Context {
    #[inline]
    fn global(&self, idx: u32) -> &Value {
        &self.globals[idx as usize]
    }

    #[inline]
    fn set_global(&mut self, idx: u32, value: Value) {
        self.globals[idx as usize] = value;
    }

    #[inline]
    fn visit_count(&self, id: DefinitionId) -> u32 {
        self.visit_counts.get(&id).copied().unwrap_or(0)
    }

    #[inline]
    fn increment_visit(&mut self, id: DefinitionId) {
        *self.visit_counts.entry(id).or_insert(0) += 1;
    }

    #[inline]
    fn turn_count(&self, id: DefinitionId) -> Option<u32> {
        self.turn_counts.get(&id).copied()
    }

    #[inline]
    fn set_turn_count(&mut self, id: DefinitionId, turn: u32) {
        self.turn_counts.insert(id, turn);
    }

    #[inline]
    fn turn_index(&self) -> u32 {
        self.turn_index
    }

    #[inline]
    fn increment_turn_index(&mut self) {
        self.turn_index += 1;
    }

    #[inline]
    fn rng_seed(&self) -> i32 {
        self.rng_seed
    }

    #[inline]
    fn set_rng_seed(&mut self, seed: i32) {
        self.rng_seed = seed;
    }

    #[inline]
    fn previous_random(&self) -> i32 {
        self.previous_random
    }

    #[inline]
    fn set_previous_random(&mut self, val: i32) {
        self.previous_random = val;
    }

    #[inline]
    fn next_random<R: StoryRng>(&self, seed: i32) -> i32 {
        let mut rng = R::from_seed(seed);
        rng.next_int()
    }

    fn random_sequence<R: StoryRng>(&self, seed: i32, count: usize) -> Vec<i32> {
        let mut rng = R::from_seed(seed);
        (0..count).map(|_| rng.next_int()).collect()
    }
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
    fn on_set_turn_count(&mut self, id: DefinitionId, turn: u32) {}
    fn on_increment_turn_index(&mut self, new_value: u32) {}
    fn on_set_rng_seed(&mut self, new_seed: i32) {}
    fn on_set_previous_random(&mut self, new_val: i32) {}
}

// ── ObservedContext ────────────────────────────────────────────────────────

/// A `ContextAccess` wrapper that delegates to an inner `Context` and
/// notifies a `WriteObserver` on every mutation.
pub struct ObservedContext<'a, 'o> {
    context: &'a mut Context,
    observer: &'o mut dyn WriteObserver,
}

impl<'a, 'o> ObservedContext<'a, 'o> {
    pub fn new(context: &'a mut Context, observer: &'o mut dyn WriteObserver) -> Self {
        Self { context, observer }
    }
}

impl ContextAccess for ObservedContext<'_, '_> {
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
        Context::next_random::<R>(seed)
    }

    fn random_sequence<R: StoryRng>(&self, seed: i32, count: usize) -> Vec<i32> {
        Context::random_sequence::<R>(seed, count)
    }
}
