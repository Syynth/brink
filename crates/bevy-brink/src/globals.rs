//! Story-wide `World` (shared) and per-flow `FlowLocal` (private) state.
//!
//! (F6.2 — see `docs/scoped-flow-state-spec.md`'s F6 AMENDMENT.) Every flow
//! spawned under a marker `M` advances against the **same** shared
//! [`World`], carried on [`BrinkGlobals<M>`] — a single `Resource`, not a
//! per-flow clone. [`BrinkContext<M>`] holds a flow's own private
//! [`FlowLocal`] override layer, fresh (empty) at spawn.
//!
//! Story-state is routed World-vs-Local per unit (globals, visit/turn
//! counts, turn index, RNG) by the [`ResolvedPolicy`](brink_runtime::ResolvedPolicy)
//! `BrinkGlobals`'s `World` was created with (see
//! [`BrinkPlugin::with_policy`](crate::BrinkPlugin::with_policy) /
//! [`BrinkWorldPolicy`]). The **default policy homes every unit to World** —
//! byte-identical to plain ink, zero-surprise for the common single-flow
//! case: reads and writes to a World-scoped unit are immediately visible to
//! every flow sharing that `World`, with no "commit" step, because they were
//! never forked in the first place.
//!
//! A flow's `FlowLocal` is that flow's own durable memory for the
//! **Local**-scoped units a host opts into via policy overrides (see
//! `docs/scoped-flow-state-spec.md`'s "The policy"): it persists for the
//! flow's lifetime, is never auto-merged anywhere, and there is no
//! `commit_from`/`commit_progress`/`commit_globals_only`-style verb — those
//! compensated for the old full-`World`-clone-per-flow model, which this
//! scoping removes outright. When private state needs to become shared
//! (an NPC's private mood counter raising a global "hostile" flag), that
//! promotion is written **in ink**, where it's visible, not bolted on as a
//! Bevy-side merge helper.
//!
//! Build the per-step routing view with [`flow_context_view`].

use std::marker::PhantomData;

use bevy_ecs::component::Component;
use bevy_ecs::resource::Resource;
use brink_runtime::{ContextView, FlowLocal, World, WorldPolicy};

/// The single shared [`World`] for a story identified by marker `M`.
///
/// Holds globals, visit/turn counts, RNG seed, and the
/// [`ResolvedPolicy`](brink_runtime::ResolvedPolicy) that routes every unit
/// World-vs-Local (resolved once, at creation, from the host's
/// [`BrinkWorldPolicy<M>`]). The plugin auto-inserts this on first
/// fulfillment (see [`fulfill_flow_requests`](crate::fulfill_flow_requests))
/// and never replaces it afterward — every flow spawned under `M` advances
/// against this same `World` for the app's lifetime.
#[derive(Resource)]
pub struct BrinkGlobals<M: Send + Sync + 'static = ()> {
    pub inner: World,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkGlobals<M> {
    /// Wrap an already-created [`World`] (e.g. from
    /// [`brink_runtime::World::new`], resolved against a program + policy)
    /// in a Bevy `Resource`.
    #[must_use]
    pub fn new(world: World) -> Self {
        Self {
            inner: world,
            _marker: PhantomData,
        }
    }
}

/// Host-supplied [`WorldPolicy`] for marker `M`'s shared [`BrinkGlobals`]
/// `World`, installed once at plugin setup via
/// [`BrinkPlugin::with_policy`](crate::BrinkPlugin::with_policy) and read by
/// [`fulfill_flow_requests`](crate::fulfill_flow_requests) when it creates
/// `BrinkGlobals<M>` on first fulfillment.
///
/// **Base ⊕ host-overrides, from day one:** base is an empty `WorldPolicy`
/// today (`WorldPolicy::default()` — every unit `World`-scoped); a
/// compiler-emitted base (a flow-private storage class for `VAR`s + knot
/// marking) is future work (#473) — the *only* place `brink-format` would
/// change for it. Until then this resource's `policy` field **is** the
/// whole installed policy.
#[derive(Resource, Clone, Default)]
pub struct BrinkWorldPolicy<M: Send + Sync + 'static = ()> {
    pub policy: WorldPolicy,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkWorldPolicy<M> {
    #[must_use]
    pub(crate) fn new(policy: WorldPolicy) -> Self {
        Self {
            policy,
            _marker: PhantomData,
        }
    }
}

/// A single flow's private override layer over the shared
/// [`BrinkGlobals<M>`] `World`.
///
/// Inserted by `fulfill_flow_requests` alongside [`BrinkFlow`](crate::BrinkFlow),
/// always fresh (empty) — spawning a flow takes no policy or seed parameter;
/// see the F6 AMENDMENT ruling 1 in `docs/scoped-flow-state-spec.md`. Reads
/// of a `Local`-scoped unit fall through to `World`'s value until this
/// flow's first local write; writes to a `World`-scoped unit always land in
/// the shared `World`, immediately visible to every other flow sharing it.
#[derive(Component, Default)]
pub struct BrinkContext<M: Send + Sync + 'static = ()> {
    pub inner: FlowLocal,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkContext<M> {
    #[must_use]
    pub fn new(local: FlowLocal) -> Self {
        Self {
            inner: local,
            _marker: PhantomData,
        }
    }
}

/// Build the [`ContextView`] routing view for one flow's step: `World`-scoped
/// units go straight to the shared `globals`; `Local`-scoped units read
/// through / write to `ctx`'s own override layer (see
/// `docs/scoped-flow-state-spec.md`).
///
/// Construct fresh for each step/call — it's a transient, step-scoped borrow
/// of both `&mut World` and `&mut FlowLocal`, never stored.
pub fn flow_context_view<'a, M: Send + Sync + 'static>(
    globals: &'a mut BrinkGlobals<M>,
    ctx: &'a mut BrinkContext<M>,
) -> ContextView<'a> {
    ContextView::new(&mut globals.inner, &mut ctx.inner)
}
