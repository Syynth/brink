//! Shared story state (`World`), the per-flow override layer (`FlowLocal`),
//! and the routing view that composes them behind [`ContextAccess`].
//!
//! This is the F1.3 stage of the scoped-flow-state restructuring
//! (`docs/scoped-flow-state-spec.md`): `World` replaces the old monolithic
//! `Context` as the core mutable-state primitive. `FlowLocal` is currently
//! an empty placeholder — F3 will give it `CoW` override storage. The
//! [`ContextView`] routing view implements [`ContextAccess`] over
//! `(&mut World, &mut FlowLocal)`; since `FlowLocal` contributes nothing
//! yet, every read and write routes straight to `World` — byte-identical to
//! today's single-`Context` behavior.

use std::collections::HashMap;

use brink_format::{DefinitionId, Value};

use crate::rng::StoryRng;
use crate::state::ContextAccess;

/// Shared game state that lives above individual flows.
///
/// Holds globals, visit/turn tracking, and RNG state. This is the natural
/// serialization boundary for save/load (deferred).
///
/// Multiple [`FlowInstance`](crate::FlowInstance)s can share a single
/// `World` (matching inklecate's semantics where flow writes are
/// immediately visible to other flows), or each flow can hold its own
/// cloned `World` if the consumer wants fork/branch/rollback semantics.
/// The runtime's step functions take `&mut World` (or any
/// `&mut impl ContextAccess`) without prescribing where it lives.
#[derive(Debug, Clone)]
pub struct World {
    pub globals: Vec<Value>,
    pub visit_counts: HashMap<DefinitionId, u32>,
    pub turn_counts: HashMap<DefinitionId, u32>,
    pub turn_index: u32,
    pub rng_seed: i32,
    pub previous_random: i32,
}

/// Per-flow override layer over the shared [`World`].
///
/// **Placeholder for F1.3.** This stage introduces the shape only — it
/// carries no overrides and contributes nothing to reads or writes.
/// [`ContextView`] (below) routes every operation straight to `World`,
/// which keeps single-flow behavior byte-identical to the old monolithic
/// `Context`. F3 fills this in with copy-on-write override storage, spawn
/// snapshots, and commit/discard semantics
/// (see `docs/scoped-flow-state-spec.md`).
#[derive(Debug, Clone, Default)]
pub struct FlowLocal {
    _private: (),
}

impl FlowLocal {
    /// Construct an empty flow-local layer. In F1.3 this is the only way
    /// to build one — there is nothing to configure yet.
    #[must_use]
    pub fn new() -> Self {
        Self { _private: () }
    }
}

/// Routing view implementing [`ContextAccess`] over `(&mut World, &mut
/// FlowLocal)`.
///
/// This is what the VM's drive path receives as its `impl ContextAccess`.
/// In F1.3, `FlowLocal` is empty, so every read/write routes straight to
/// `World` — an all-World passthrough, byte-for-byte the old `Context`
/// behavior. F2 will consult a `ResolvedPolicy` here to decide whether a
/// given unit routes to `World` or `FlowLocal`.
pub struct ContextView<'a> {
    world: &'a mut World,
    #[expect(
        dead_code,
        reason = "F1.3 placeholder — FlowLocal is empty and unread until F2/F3 add routing"
    )]
    local: &'a mut FlowLocal,
}

impl<'a> ContextView<'a> {
    /// Build a routing view over a `World` and `FlowLocal` pair for the
    /// duration of one step.
    pub fn new(world: &'a mut World, local: &'a mut FlowLocal) -> Self {
        Self { world, local }
    }
}

impl ContextAccess for World {
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

impl ContextAccess for ContextView<'_> {
    #[inline]
    fn global(&self, idx: u32) -> &Value {
        self.world.global(idx)
    }

    #[inline]
    fn set_global(&mut self, idx: u32, value: Value) {
        self.world.set_global(idx, value);
    }

    #[inline]
    fn visit_count(&self, id: DefinitionId) -> u32 {
        self.world.visit_count(id)
    }

    #[inline]
    fn increment_visit(&mut self, id: DefinitionId) {
        self.world.increment_visit(id);
    }

    #[inline]
    fn turn_count(&self, id: DefinitionId) -> Option<u32> {
        self.world.turn_count(id)
    }

    #[inline]
    fn set_turn_count(&mut self, id: DefinitionId, turn: u32) {
        self.world.set_turn_count(id, turn);
    }

    #[inline]
    fn turn_index(&self) -> u32 {
        self.world.turn_index()
    }

    #[inline]
    fn increment_turn_index(&mut self) {
        self.world.increment_turn_index();
    }

    #[inline]
    fn rng_seed(&self) -> i32 {
        self.world.rng_seed()
    }

    #[inline]
    fn set_rng_seed(&mut self, seed: i32) {
        self.world.set_rng_seed(seed);
    }

    #[inline]
    fn previous_random(&self) -> i32 {
        self.world.previous_random()
    }

    #[inline]
    fn set_previous_random(&mut self, val: i32) {
        self.world.set_previous_random(val);
    }

    #[inline]
    fn next_random<R: StoryRng>(&self, seed: i32) -> i32 {
        self.world.next_random::<R>(seed)
    }

    fn random_sequence<R: StoryRng>(&self, seed: i32, count: usize) -> Vec<i32> {
        self.world.random_sequence::<R>(seed, count)
    }
}
