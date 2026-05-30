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

    /// Wholesale replace the "save data" with a clone of `flow_ctx`.
    ///
    /// Use this for "save the entire game state" — typically when a
    /// scene ends and you want the main story state to mirror exactly
    /// what the in-flight flow produced.
    ///
    /// Also the right verb for a "new game" reset:
    /// `globals.commit_from(&program.initial_context)`.
    pub fn commit_from(&mut self, flow_ctx: &Context) {
        self.inner = flow_ctx.clone();
    }

    /// Merge "progress" from the flow's `Context` into the save data:
    /// globals are wholesale replaced; visit and turn counts take the
    /// elementwise max; the turn index takes the max; RNG state is
    /// pulled from the flow (most recent).
    ///
    /// Use this when a side conversation should contribute its world
    /// changes (variables, visit history, advancement) back to the
    /// shared save state without overwriting state the side
    /// conversation didn't touch.
    pub fn commit_progress(&mut self, flow_ctx: &Context) {
        self.inner.globals.clone_from(&flow_ctx.globals);
        for (id, count) in &flow_ctx.visit_counts {
            let entry = self.inner.visit_counts.entry(*id).or_insert(0);
            *entry = (*entry).max(*count);
        }
        for (id, turn) in &flow_ctx.turn_counts {
            let entry = self.inner.turn_counts.entry(*id).or_insert(0);
            *entry = (*entry).max(*turn);
        }
        self.inner.turn_index = self.inner.turn_index.max(flow_ctx.turn_index);
        self.inner.rng_seed = flow_ctx.rng_seed;
        self.inner.previous_random = flow_ctx.previous_random;
    }

    /// Replace just the named globals — leave visit/turn counts, turn
    /// index, and RNG state untouched.
    ///
    /// Use this when a flow may have changed inventory or flag-style
    /// variables but didn't progress the main plot.
    pub fn commit_globals_only(&mut self, flow_ctx: &Context) {
        self.inner.globals.clone_from(&flow_ctx.globals);
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

#[cfg(test)]
mod commit_tests {
    use super::*;
    use brink_format::{DefinitionId, DefinitionTag, Value};
    use brink_runtime::Context;
    use std::collections::HashMap;

    fn ctx_with(globals: Vec<Value>, visits: &[(u64, u32)], turn_index: u32) -> Context {
        let mut visit_counts = HashMap::new();
        for (id, count) in visits {
            visit_counts.insert(DefinitionId::new(DefinitionTag::Address, *id), *count);
        }
        Context {
            globals,
            visit_counts,
            turn_counts: HashMap::new(),
            turn_index,
            rng_seed: 0,
            previous_random: 0,
        }
    }

    #[test]
    fn commit_from_replaces_wholesale() {
        let mut globals =
            BrinkGlobals::<()>::new(ctx_with(vec![Value::Int(1), Value::Int(2)], &[(0, 5)], 10));
        let flow_ctx = ctx_with(vec![Value::Int(99), Value::Int(100)], &[(0, 1)], 3);
        globals.commit_from(&flow_ctx);
        assert!(matches!(globals.inner.globals[0], Value::Int(99)));
        assert_eq!(
            globals.inner.visit_counts[&DefinitionId::new(DefinitionTag::Address, 0)],
            1
        );
        assert_eq!(globals.inner.turn_index, 3);
    }

    #[test]
    fn commit_progress_takes_max_of_counts() {
        let mut globals =
            BrinkGlobals::<()>::new(ctx_with(vec![Value::Int(1)], &[(0, 5), (1, 2)], 10));
        let flow_ctx = ctx_with(vec![Value::Int(99)], &[(0, 3), (2, 7)], 4);
        globals.commit_progress(&flow_ctx);
        // Globals: replaced from flow.
        assert!(matches!(globals.inner.globals[0], Value::Int(99)));
        // Visit counts: max per id; ids only in self stay; ids only
        // in flow added.
        assert_eq!(
            globals.inner.visit_counts[&DefinitionId::new(DefinitionTag::Address, 0)],
            5
        );
        assert_eq!(
            globals.inner.visit_counts[&DefinitionId::new(DefinitionTag::Address, 1)],
            2
        );
        assert_eq!(
            globals.inner.visit_counts[&DefinitionId::new(DefinitionTag::Address, 2)],
            7
        );
        // Turn index: max.
        assert_eq!(globals.inner.turn_index, 10);
    }

    #[test]
    fn commit_globals_only_leaves_counts_alone() {
        let mut globals = BrinkGlobals::<()>::new(ctx_with(vec![Value::Int(1)], &[(0, 5)], 10));
        let flow_ctx = ctx_with(vec![Value::Int(99)], &[(0, 99)], 99);
        globals.commit_globals_only(&flow_ctx);
        assert!(matches!(globals.inner.globals[0], Value::Int(99)));
        assert_eq!(
            globals.inner.visit_counts[&DefinitionId::new(DefinitionTag::Address, 0)],
            5
        );
        assert_eq!(globals.inner.turn_index, 10);
    }
}
