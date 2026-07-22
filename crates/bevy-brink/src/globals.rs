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
//!
//! ## Save/load (F6.3)
//!
//! A save is **one [`SaveState`] for the shared `World`, plus one per entity
//! flow**, composed **host-side** — this module exposes thin per-context
//! helpers, not a save-file format or a pre-composed bundle type (see the
//! F6 AMENDMENT, ruling 4, in `docs/scoped-flow-state-spec.md`): a host
//! collects the map of `SaveState`s (e.g. `entity -> SaveState` plus one
//! world `SaveState`) into whatever container/on-disk format it wants.
//!
//! - [`BrinkGlobals::save_state`] / [`BrinkGlobals::load_state`] — the
//!   shared `World`, direct (`World` implements `ContextAccess` itself, no
//!   routing view needed).
//! - [`save_flow_state`] / [`load_flow_state`] — one flow, routed through
//!   its [`ContextView`] (built the same way [`flow_context_view`] builds
//!   one for a step): saving captures **effective** values (a `Local`
//!   override where the flow has one, else the live `World` value on a
//!   read-through miss); loading routes by scope, so `Local`-scoped entries
//!   land in the flow's own `FlowLocal` and `World`-scoped entries write
//!   straight through to the shared `World`.
//!
//! **Load order matters for the `World`-scoped entries to converge
//! correctly:** load the world `SaveState` via [`BrinkGlobals::load_state`]
//! first, then each entity's `SaveState` via [`load_flow_state`] second.
//! Because every entity snapshot taken at the same save moment carries
//! *identical* `World`-scoped values, each entity's load rewrites those
//! same values back into the shared `World` — idempotent, not a conflict —
//! while routing that entity's own `Local`-scoped entries into its private
//! `FlowLocal`. Loading entities in any order (or omitting the world load
//! entirely, relying on entity loads alone) still converges to the same
//! `World` state, since every entity carries the same World-scoped values;
//! the explicit world-first load is the documented, simplest-to-reason-about
//! order.
//!
//! **State-only, not position.** Save/load captures *game state* — globals,
//! visit/turn counts, turn index, RNG (see [`brink_runtime::save_state`]'s
//! docs for the precise contents) — never a flow's execution position
//! (call stack / program counter). A loaded entity does not resume
//! mid-line: the host re-enters it at a knot of its choosing (typically via
//! [`FlowStart::Address`](crate::FlowStart::Address) on a fresh
//! [`BrinkFlowRequest`](crate::BrinkFlowRequest)), and the restored state
//! (a private "have I greeted them" visit count, a private mood variable)
//! is what makes that re-entry pick up where the entity left off, not a
//! restored call stack.
//!
//! **[`LoadReport`] tolerance.** Both load paths return a [`LoadReport`]
//! rather than an error: a saved global the current program no longer
//! declares is dropped and named in
//! [`LoadReport::unknown_globals`](brink_runtime::LoadReport::unknown_globals)
//! so the host can surface it (e.g. after a story patch that renamed or
//! removed a `VAR`); saved visit/turn counts for scopes the program no
//! longer has are retained harmlessly rather than reported. See
//! [`brink_runtime::load_state`]'s docs for the full reconciliation
//! semantics, including the one behavioral note worth restating here: a
//! stale saved entry (a global/scope the *current* program lacks) is not
//! re-emitted by a later save — [`brink_runtime::save_state`] enumerates
//! the current program's own globals/containers, not whatever the live
//! context happens to hold — so ghost entries from an old program version
//! don't round-trip through save after save indefinitely.

use std::marker::PhantomData;

use bevy_ecs::component::Component;
use bevy_ecs::resource::Resource;
use brink_runtime::{
    ContextAccess, ContextView, ExecMode, FlowLocal, LoadReport, Program, SaveState, World,
    WorldPolicy,
};

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

    /// Capture the shared `World`'s durable game state as a [`SaveState`] —
    /// see the module docs' "Save/load" section. Thin wrapper over
    /// [`brink_runtime::save_state`] with `World` itself as the
    /// [`ContextAccess`](brink_runtime::ContextAccess) implementor, so this
    /// reads raw `World` storage: any unit a policy homes to `Local` was
    /// never written here (`Local` writes land only in a flow's own
    /// `FlowLocal`), so this captures exactly the `World`-scoped half of the
    /// story's state. Does **not** capture any entity's private state —
    /// pair with [`save_flow_state`] per entity to save a complete world.
    #[must_use]
    pub fn save_state(&self, program: &Program) -> SaveState {
        brink_runtime::save_state(program, &self.inner)
    }

    /// Reconcile a [`SaveState`] into the shared `World` directly (no
    /// routing view — every unit lands in `World` storage regardless of
    /// what the policy says, mirroring [`save_state`](Self::save_state)'s
    /// symmetric read). Load the world state **before** any entity's, via
    /// [`load_flow_state`] — see the module docs' "Save/load" section for
    /// why load order matters (it doesn't change the converged result, but
    /// world-first is the simplest order to reason about).
    pub fn load_state(&mut self, program: &Program, save: &SaveState) -> LoadReport {
        brink_runtime::load_state(program, &mut self.inner, save)
    }

    /// Read an ink global by name — the ergonomic host-side read seam (G2,
    /// issue #1059). Collapses the manual `Program::global_index(name)` +
    /// `ContextAccess::global(idx)` reach (which also requires importing the
    /// `ContextAccess` trait) into one call.
    ///
    /// **Panic-free miss behavior:** returns `None`, never panics, when
    /// `program` declares no global named `name` — a typo'd or renamed
    /// global reads as "absent" rather than crashing the host. Callers that
    /// need to distinguish "no such global" from a real `Value::Null` should
    /// check `program.global_index(name)` directly.
    ///
    /// This always reads the shared `World`-scoped value (`self.inner`),
    /// matching `save_state`'s scope — it does not route through a flow's
    /// `Local` override layer. For a flow-scoped read use
    /// [`flow_context_view`] and read through the resulting [`ContextView`]
    /// instead.
    #[must_use]
    pub fn get(&self, program: &Program, name: &str) -> Option<&brink_format::Value> {
        let idx = program.global_index(name)?;
        Some(self.inner.global(idx))
    }
}

/// Capture one flow's *effective* durable game state as a [`SaveState`] —
/// see the module docs' "Save/load" section.
///
/// Builds a [`ContextView`] over `globals` and `ctx` exactly like
/// [`flow_context_view`] does for a step, then delegates to
/// [`brink_runtime::save_state`] through it: a `World`-scoped unit reads
/// `globals`' live shared value; a `Local`-scoped unit reads `ctx`'s own
/// override where present, else falls through to `globals`' value. So two
/// entities saved at the same moment carry byte-identical values for every
/// `World`-scoped unit (the idempotent-rewrite property [`load_flow_state`]
/// relies on) while each carries its own distinct `Local`-scoped values.
#[must_use]
pub fn save_flow_state<M: Send + Sync + 'static>(
    globals: &mut BrinkGlobals<M>,
    ctx: &mut BrinkContext<M>,
    program: &Program,
) -> SaveState {
    let view = flow_context_view(globals, ctx);
    brink_runtime::save_state(program, &view)
}

/// Reconcile a [`SaveState`] into one flow, routed by scope — see the
/// module docs' "Save/load" section.
///
/// Builds a [`ContextView`] over `globals` and `ctx` exactly like
/// [`flow_context_view`] does for a step, then delegates to
/// [`brink_runtime::load_state`] through it: a `Local`-scoped entry writes
/// into `ctx`'s own [`FlowLocal`] overrides (this flow's private memory,
/// invisible to every other flow); a `World`-scoped entry writes straight
/// through to `globals`' shared `World`, immediately visible to every flow
/// sharing it. Loading several entities' saves in sequence (all taken at
/// the same save moment, so all carrying the same `World`-scoped values) is
/// therefore an idempotent rewrite of the shared `World`, not a conflict.
pub fn load_flow_state<M: Send + Sync + 'static>(
    globals: &mut BrinkGlobals<M>,
    ctx: &mut BrinkContext<M>,
    program: &Program,
    save: &SaveState,
) -> LoadReport {
    let mut view = flow_context_view(globals, ctx);
    brink_runtime::load_state(program, &mut view, save)
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

/// The host-selected [`ExecMode`] every flow of marker `M` starts in
/// (F35, ruled 2026-07-19).
///
/// Inserted once by [`BrinkPlugin::build`](crate::BrinkPlugin) and applied
/// to each [`FlowInstance`](brink_runtime::FlowInstance) at spawn by
/// `fulfill_flow_requests`. Unlike core `brink-runtime` — whose
/// [`ExecMode::default`] is always [`Dev`](ExecMode::Dev) — bevy-brink's
/// default keys off the build profile: `Dev` under `debug_assertions`
/// (editor / `cargo run`), `Prod` in a release build (`cargo build
/// --release`), so a shipped game defaults to the keep-moving posture and
/// an in-editor session to the fault-loud one. A host overrides either way
/// with [`BrinkPlugin::with_exec_mode`](crate::BrinkPlugin::with_exec_mode).
#[derive(Resource, Clone, Copy)]
pub struct BrinkExecMode<M: Send + Sync + 'static = ()> {
    pub mode: ExecMode,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkExecMode<M> {
    #[must_use]
    pub(crate) fn new(mode: ExecMode) -> Self {
        Self {
            mode,
            _marker: PhantomData,
        }
    }
}

impl<M: Send + Sync + 'static> Default for BrinkExecMode<M> {
    /// The profile-keyed default (F35): `Dev` under `debug_assertions`,
    /// `Prod` otherwise. This is the one place bevy-brink diverges from the
    /// core runtime's always-`Dev` [`ExecMode::default`].
    fn default() -> Self {
        Self::new(if cfg!(debug_assertions) {
            ExecMode::Dev
        } else {
            ExecMode::Prod
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::compile_test_story;

    #[test]
    fn get_reads_a_declared_global_by_name() {
        let (program, _tables, world) = compile_test_story("VAR mood = 7\n-> END\n");
        let globals = BrinkGlobals::<()>::new(world);
        assert_eq!(
            globals.get(&program, "mood"),
            Some(&brink_format::Value::Int(7))
        );
    }

    #[test]
    fn get_sees_writes_made_through_context_access() {
        let (program, _tables, world) = compile_test_story("VAR mood = 7\n-> END\n");
        let mut globals = BrinkGlobals::<()>::new(world);
        let idx = program.global_index("mood").expect("mood is declared");
        globals.inner.set_global(idx, brink_format::Value::Int(41));
        assert_eq!(
            globals.get(&program, "mood"),
            Some(&brink_format::Value::Int(41))
        );
    }

    #[test]
    fn get_returns_none_for_an_unknown_name_without_panicking() {
        let (program, _tables, world) = compile_test_story("VAR mood = 7\n-> END\n");
        let globals = BrinkGlobals::<()>::new(world);
        assert_eq!(globals.get(&program, "does_not_exist"), None);
    }
}
