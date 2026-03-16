//! Story state trait and runtime implementation.

use std::marker::PhantomData;

use brink_format::{DefinitionId, PluralResolver, Value};

use crate::program::Program;
use crate::rng::StoryRng;
use crate::story::Context;

/// Trait bundling immutable program data with mutable execution state.
///
/// Provides a uniform interface for the VM to access program metadata
/// and runtime state (globals, visit counts, RNG) without threading
/// separate `&Program` and `&mut Context` parameters through every function.
pub trait StoryState {
    /// Access the immutable program data.
    fn program(&self) -> &Program;

    /// Read a global variable by resolved index.
    fn global(&self, idx: u32) -> &Value;

    /// Set a global variable by resolved index.
    fn set_global(&mut self, idx: u32, value: Value);

    /// Get the visit count for a definition.
    fn visit_count(&self, id: DefinitionId) -> u32;

    /// Increment the visit count for a definition.
    fn increment_visit(&mut self, id: DefinitionId);

    /// Get the turn on which a definition was last visited.
    fn turn_count(&self, id: DefinitionId) -> Option<u32>;

    /// Record the turn on which a definition was visited.
    fn set_turn_count(&mut self, id: DefinitionId, turn: u32);

    /// Get the current turn index.
    fn turn_index(&self) -> u32;

    /// Increment the turn index by one.
    fn increment_turn_index(&mut self);

    /// Get the RNG seed.
    fn rng_seed(&self) -> i32;

    /// Set the RNG seed.
    fn set_rng_seed(&mut self, seed: i32);

    /// Get the previous random value.
    fn previous_random(&self) -> i32;

    /// Set the previous random value.
    fn set_previous_random(&mut self, val: i32);

    /// Generate a single random integer from the given seed.
    fn next_random(&mut self, seed: i32) -> i32;

    /// Generate a sequence of random integers from the given seed.
    ///
    /// Creates a single RNG instance from `seed` and calls `next_int()`
    /// `count` times, returning all values. Used by shuffle sequences
    /// that need multiple correlated random values from one RNG instance.
    fn random_sequence(&mut self, seed: i32, count: usize) -> Vec<i32>;

    /// Access the plural resolver for Select resolution.
    ///
    /// Returns `None` if no resolver is configured (Select falls back to default).
    fn plural_resolver(&self) -> Option<&dyn PluralResolver>;
}

/// Runtime implementation of [`StoryState`].
///
/// Bundles an immutable `&Program` reference with a mutable `&mut Context`,
/// parametrized over the RNG type `R`.
pub(crate) struct RuntimeState<'a, R: StoryRng> {
    program: &'a Program,
    context: &'a mut Context,
    resolver: Option<&'a dyn PluralResolver>,
    _rng: PhantomData<R>,
}

impl<'a, R: StoryRng> RuntimeState<'a, R> {
    pub fn new(
        program: &'a Program,
        context: &'a mut Context,
        resolver: Option<&'a dyn PluralResolver>,
    ) -> Self {
        Self {
            program,
            context,
            resolver,
            _rng: PhantomData,
        }
    }
}

impl<R: StoryRng> StoryState for RuntimeState<'_, R> {
    #[inline]
    fn program(&self) -> &Program {
        self.program
    }

    #[inline]
    fn global(&self, idx: u32) -> &Value {
        &self.context.globals[idx as usize]
    }

    #[inline]
    fn set_global(&mut self, idx: u32, value: Value) {
        self.context.globals[idx as usize] = value;
    }

    #[inline]
    fn visit_count(&self, id: DefinitionId) -> u32 {
        self.context.visit_counts.get(&id).copied().unwrap_or(0)
    }

    #[inline]
    fn increment_visit(&mut self, id: DefinitionId) {
        *self.context.visit_counts.entry(id).or_insert(0) += 1;
    }

    #[inline]
    fn turn_count(&self, id: DefinitionId) -> Option<u32> {
        self.context.turn_counts.get(&id).copied()
    }

    #[inline]
    fn set_turn_count(&mut self, id: DefinitionId, turn: u32) {
        self.context.turn_counts.insert(id, turn);
    }

    #[inline]
    fn turn_index(&self) -> u32 {
        self.context.turn_index
    }

    #[inline]
    fn increment_turn_index(&mut self) {
        self.context.turn_index += 1;
    }

    #[inline]
    fn rng_seed(&self) -> i32 {
        self.context.rng_seed
    }

    #[inline]
    fn set_rng_seed(&mut self, seed: i32) {
        self.context.rng_seed = seed;
    }

    #[inline]
    fn previous_random(&self) -> i32 {
        self.context.previous_random
    }

    #[inline]
    fn set_previous_random(&mut self, val: i32) {
        self.context.previous_random = val;
    }

    #[inline]
    fn next_random(&mut self, seed: i32) -> i32 {
        let mut rng = R::from_seed(seed);
        rng.next_int()
    }

    fn random_sequence(&mut self, seed: i32, count: usize) -> Vec<i32> {
        let mut rng = R::from_seed(seed);
        (0..count).map(|_| rng.next_int()).collect()
    }

    fn plural_resolver(&self) -> Option<&dyn PluralResolver> {
        self.resolver
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

// ── ObservedState ──────────────────────────────────────────────────────────

/// A [`StoryState`] wrapper that delegates to an inner [`RuntimeState`] and
/// notifies a [`WriteObserver`] on every mutation.
pub(crate) struct ObservedState<'a, 'o, R: StoryRng> {
    inner: RuntimeState<'a, R>,
    observer: &'o mut dyn WriteObserver,
}

impl<'a, 'o, R: StoryRng> ObservedState<'a, 'o, R> {
    pub fn new(
        program: &'a Program,
        context: &'a mut Context,
        resolver: Option<&'a dyn PluralResolver>,
        observer: &'o mut dyn WriteObserver,
    ) -> Self {
        Self {
            inner: RuntimeState::new(program, context, resolver),
            observer,
        }
    }
}

impl<R: StoryRng> StoryState for ObservedState<'_, '_, R> {
    #[inline]
    fn program(&self) -> &Program {
        self.inner.program()
    }

    #[inline]
    fn global(&self, idx: u32) -> &Value {
        self.inner.global(idx)
    }

    #[inline]
    fn set_global(&mut self, idx: u32, value: Value) {
        self.inner.set_global(idx, value.clone());
        self.observer.on_set_global(idx, &value);
    }

    #[inline]
    fn visit_count(&self, id: DefinitionId) -> u32 {
        self.inner.visit_count(id)
    }

    #[inline]
    fn increment_visit(&mut self, id: DefinitionId) {
        self.inner.increment_visit(id);
        let new_count = self.inner.visit_count(id);
        self.observer.on_increment_visit(id, new_count);
    }

    #[inline]
    fn turn_count(&self, id: DefinitionId) -> Option<u32> {
        self.inner.turn_count(id)
    }

    #[inline]
    fn set_turn_count(&mut self, id: DefinitionId, turn: u32) {
        self.inner.set_turn_count(id, turn);
        self.observer.on_set_turn_count(id, turn);
    }

    #[inline]
    fn turn_index(&self) -> u32 {
        self.inner.turn_index()
    }

    #[inline]
    fn increment_turn_index(&mut self) {
        self.inner.increment_turn_index();
        self.observer
            .on_increment_turn_index(self.inner.turn_index());
    }

    #[inline]
    fn rng_seed(&self) -> i32 {
        self.inner.rng_seed()
    }

    #[inline]
    fn set_rng_seed(&mut self, seed: i32) {
        self.inner.set_rng_seed(seed);
        self.observer.on_set_rng_seed(seed);
    }

    #[inline]
    fn previous_random(&self) -> i32 {
        self.inner.previous_random()
    }

    #[inline]
    fn set_previous_random(&mut self, val: i32) {
        self.inner.set_previous_random(val);
        self.observer.on_set_previous_random(val);
    }

    #[inline]
    fn next_random(&mut self, seed: i32) -> i32 {
        self.inner.next_random(seed)
    }

    fn random_sequence(&mut self, seed: i32, count: usize) -> Vec<i32> {
        self.inner.random_sequence(seed, count)
    }

    fn plural_resolver(&self) -> Option<&dyn PluralResolver> {
        self.inner.plural_resolver()
    }
}
