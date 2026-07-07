//! Shared story state (`World`), the per-flow override layer (`FlowLocal`),
//! and the routing view that composes them behind [`ContextAccess`].
//!
//! This is the F1.3 stage of the scoped-flow-state restructuring
//! (`docs/scoped-flow-state-spec.md`): `World` replaces the old monolithic
//! `Context` as the core mutable-state primitive. The [`ContextView`]
//! routing view implements [`ContextAccess`] over `(&mut World, &mut
//! FlowLocal)`.
//!
//! F2.1 adds the **policy** types ([`Scope`], [`WorldPolicy`],
//! [`ResolvedPolicy`]) and their resolution against a [`Program`]'s symbol
//! table.
//!
//! F2.2 gave `FlowLocal` flat override storage (plain maps/options — no
//! `CoW` chain) and wired [`ContextView`] to route every [`ContextAccess`]
//! op by consulting `World`'s [`ResolvedPolicy`] with **read-through**
//! semantics: a `Local`-scoped unit reads its `FlowLocal` override if
//! present, else falls back to `World`'s value; a `World`-scoped unit
//! always goes straight to `World`. Writes to a `Local`-scoped unit land in
//! `FlowLocal`; writes to a `World`-scoped unit land in `World`,
//! immediately visible to every flow sharing it.
//!
//! **F3.1 (this stage)** upgrades `FlowLocal`'s storage to a copy-on-write,
//! frozen-base read-through **chain**: `FlowLocal` gains an optional
//! [`Arc<FrozenLocal>`] base, an immutable snapshot of another `FlowLocal`'s
//! overrides (captured via [`FlowLocal::freeze`]) that can itself chain to
//! a further base. A read walks **own overrides → base (recursively) →
//! [miss]**; a miss falls through to `World` exactly as before. Writes
//! still land only in the flow's own top-layer overrides. This stage adds
//! **no** fork/spawn/sandbox — every existing `FlowLocal` construction path
//! produces `base: None`, so the chain never has anything to walk past the
//! top layer and every read is exactly the F2.2 read-through. Fork (F3.2)
//! is what actually populates `base` by freezing a parent.
//!
//! The all-`World` policy (the default, and the only policy the oracle
//! corpus exercises) takes the `World` branch on every op, so `ContextView`
//! stays byte-identical to the F1.3 passthrough for every existing
//! single-flow construction path.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use brink_format::{DefinitionId, Value};

use crate::program::Program;
use crate::rng::StoryRng;
use crate::state::ContextAccess;

// ── Policy ───────────────────────────────────────────────────────────────
//
// The scoped-flow-state model (`docs/scoped-flow-state-spec.md`, "The
// policy") homes every unit of story-state — globals, visit/turn counts,
// turn index, RNG — to either the shared `World` or a flow's private
// `FlowLocal`. `WorldPolicy` is the host-facing, name-based declaration of
// that split; `ResolvedPolicy` is the fast id/slot-based form the runtime
// actually consults, built once at `World` creation.
//
// F2.1 introduces both shapes and resolution only. `ResolvedPolicy` is
// stored on `World` but unread — F2.2 wires `ContextView` to consult it.

/// Where a unit of story-state lives: the shared [`World`] or a flow's
/// private [`FlowLocal`].
///
/// `World` is visible to every flow sharing that world immediately on
/// write — the coordination path. `Local` is private to one flow; it
/// persists for that flow's lifetime and only folds back into a parent via
/// an explicit (currently unimplemented, F3) `commit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    /// Shared across every flow over the world. This is the default —
    /// matches today's single-`Context` behavior byte-for-byte.
    #[default]
    World,
    /// Private to one flow.
    Local,
}

/// Host-facing, name-based declaration of the world/local split.
///
/// Resolved once (via [`ResolvedPolicy::resolve`]) against a linked
/// [`Program`]'s symbol table into a fast id/slot-based [`ResolvedPolicy`].
/// Unlisted variables and knots/stitches fall back to `default`.
///
/// The all-`World` default (via [`WorldPolicy::default`]) is the
/// degenerate, oracle-safety-anchoring policy: every unit homed to
/// `World`, no overrides — identical to today's single-flow behavior.
///
/// **Name precedence:** a name in `overrides` is tried as a global variable
/// first, then as a knot/stitch path (see [`ResolvedPolicy::resolve`]). If a
/// name is (unusually) both a declared global VAR and a resolvable knot/
/// stitch path, the override resolves against the **variable**, never the
/// knot — the knot path is not consulted once a variable of the same name
/// is found.
#[derive(Debug, Clone, Default)]
pub struct WorldPolicy {
    /// Scope for any variable or knot/stitch not named in `overrides`.
    pub default: Scope,
    /// Per-name exceptions to `default`, for global variables (matched
    /// against `Program::global_index`'s name grammar) and knot/stitch
    /// paths (matched against `Program::find_path_target`'s path
    /// grammar). A name may appear in only one of the two — the resolver
    /// tries variables first, then knot paths.
    pub overrides: BTreeMap<String, Scope>,
    /// Scope of the turn index (a single scalar field).
    pub turn_index: Scope,
    /// Scope of the RNG stream (`rng_seed` + `previous_random`, a single
    /// scalar stream). See the spec's determinism caveat: a `World`-scoped
    /// RNG interleaves draws from every flow sharing the world by
    /// execution order.
    pub rng: Scope,
}

/// Errors resolving a [`WorldPolicy`] against a [`Program`]'s symbol table.
///
/// Resolution happens once, at `World` creation — an unknown name here is
/// a host configuration error, not a runtime one.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum PolicyError {
    /// A name in `WorldPolicy::overrides` matched neither a declared
    /// global variable nor a resolvable knot/stitch path.
    #[error("unknown variable or knot/stitch in world policy overrides: {0}")]
    UnknownName(String),
}

/// Fast, id/slot-based resolution of a [`WorldPolicy`] against a specific
/// [`Program`]. Built once at `World` creation via
/// [`ResolvedPolicy::resolve`]; consulted on every state access (from F2.2
/// on) with O(1) lookups — no string matching on the hot path.
#[derive(Debug, Clone)]
pub struct ResolvedPolicy {
    /// Default scope for globals/knots not otherwise listed.
    default: Scope,
    /// Per-slot scope for every global variable, dense (length ==
    /// `Program::global_count()`). Populated with `default` for slots with
    /// no override, so lookups never need a fallback branch.
    global_scopes: Vec<Scope>,
    /// Non-default scope for a knot/stitch, keyed by its defining
    /// `DefinitionId` (the same id used for visit counting). Only
    /// exceptions to `default` are stored — sparse, since most programs
    /// have far more knots than overrides.
    knot_scopes: HashMap<DefinitionId, Scope>,
    /// Scope of the turn index.
    turn_index: Scope,
    /// Scope of the RNG stream.
    rng: Scope,
}

impl ResolvedPolicy {
    /// The all-`World` resolved policy — no name lookups needed. This is
    /// the fast path for [`WorldPolicy::default()`] and the only policy
    /// exercised by the oracle-anchored single-flow path.
    #[must_use]
    pub fn all_world() -> Self {
        Self {
            default: Scope::World,
            global_scopes: Vec::new(),
            knot_scopes: HashMap::new(),
            turn_index: Scope::World,
            rng: Scope::World,
        }
    }

    /// Resolve a host-facing [`WorldPolicy`] against a linked `Program`'s
    /// symbol table.
    ///
    /// Variable names are resolved via [`Program::global_index`]; knot/
    /// stitch paths via `Program`'s path-to-`DefinitionId` resolution
    /// (the same table `find_address`/`find_path_target` use). A name is
    /// tried as a variable first, then as a knot/stitch path; a name
    /// matching neither is a [`PolicyError::UnknownName`].
    ///
    /// The all-`World` default (empty `overrides`) resolves without any
    /// name lookups (see [`all_world`](Self::all_world)) — this is the
    /// fast path every existing construction path takes today.
    pub fn resolve(program: &Program, policy: &WorldPolicy) -> Result<Self, PolicyError> {
        if policy.overrides.is_empty()
            && policy.default == Scope::World
            && policy.turn_index == Scope::World
            && policy.rng == Scope::World
        {
            return Ok(Self::all_world());
        }

        let mut global_scopes = vec![policy.default; program.global_count() as usize];
        let mut knot_scopes = HashMap::new();

        // `overrides` is a `BTreeMap`, so iteration order is deterministic
        // (sorted by name) — resolution never depends on hash-map order.
        for (name, &scope) in &policy.overrides {
            if let Some(slot) = program.global_index(name) {
                global_scopes[slot as usize] = scope;
            } else if let Some(id) = program.find_path_target(name) {
                knot_scopes.insert(id, scope);
            } else {
                return Err(PolicyError::UnknownName(name.clone()));
            }
        }

        Ok(Self {
            default: policy.default,
            global_scopes,
            knot_scopes,
            turn_index: policy.turn_index,
            rng: policy.rng,
        })
    }

    /// Scope of a global variable by slot index.
    #[must_use]
    pub fn scope_of_global(&self, slot: u32) -> Scope {
        self.global_scopes
            .get(slot as usize)
            .copied()
            .unwrap_or(self.default)
    }

    /// Scope of a knot/stitch by its defining `DefinitionId`.
    #[must_use]
    pub fn scope_of_knot(&self, id: DefinitionId) -> Scope {
        self.knot_scopes.get(&id).copied().unwrap_or(self.default)
    }

    /// Scope of the turn index.
    #[must_use]
    pub fn turn_index_scope(&self) -> Scope {
        self.turn_index
    }

    /// Scope of the RNG stream.
    #[must_use]
    pub fn rng_scope(&self) -> Scope {
        self.rng
    }
}

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
    /// The resolved world/local scoping policy for this world.
    ///
    /// Boxed so `World` (cloned per-flow-spawn and stored inline in several
    /// call sites and enums across the crate graph) doesn't balloon in size
    /// for consumers that never touch policy — `ResolvedPolicy` carries a
    /// per-slot `Vec` and a `HashMap` that dwarf `World`'s other fields.
    ///
    /// **Consulted by [`ContextView`] (F2.2 on)** to route every
    /// [`ContextAccess`] op between `World` and `FlowLocal`. Every
    /// construction path that predates policy (`World::from_globals`,
    /// `FlowInstance::new_at*`, `Story::new`) resolves
    /// [`WorldPolicy::default()`] (all-`World`), so those paths route every
    /// op straight to `World` — unchanged from F1.3.
    policy: Box<ResolvedPolicy>,
}

impl World {
    /// Create a fresh `World` for `program`, resolving `policy` against the
    /// program's symbol table.
    ///
    /// Globals are initialized from the program's declared defaults; visit
    /// counts, turn counts, turn index, and RNG state start zeroed —
    /// identical to `FlowInstance::new_at`'s inline construction. Fails if
    /// `policy` names a variable or knot/stitch the program doesn't
    /// declare ([`PolicyError::UnknownName`]).
    ///
    /// [`WorldPolicy::default()`] (all-`World`) always resolves — see
    /// [`ResolvedPolicy::all_world`] — so passing it here can't produce a
    /// `PolicyError`.
    pub fn new(program: &Program, policy: &WorldPolicy) -> Result<Self, PolicyError> {
        let resolved = ResolvedPolicy::resolve(program, policy)?;
        Ok(Self::from_globals(program.global_defaults(), resolved))
    }

    /// Build a `World` from an explicit globals vector and an
    /// already-resolved policy. Used by [`crate::FlowInstance::new_at`] (whose
    /// signature predates policy and can't take a `Result`) to construct
    /// the all-`World` default without re-deriving it from a `Program` each
    /// time.
    pub(crate) fn from_globals(globals: Vec<Value>, policy: ResolvedPolicy) -> Self {
        Self {
            globals,
            visit_counts: HashMap::new(),
            turn_counts: HashMap::new(),
            turn_index: 0,
            rng_seed: 0,
            previous_random: 0,
            policy: Box::new(policy),
        }
    }

    /// Construct a `World` directly from its field values, with the
    /// all-`World` policy. Only for test fixtures that need to hand-build a
    /// `World` without a `Program` (e.g. `bevy-brink`'s commit-merge
    /// tests) — production code should go through [`World::new`].
    #[cfg(feature = "testing")]
    #[must_use]
    pub fn new_for_testing(
        globals: Vec<Value>,
        visit_counts: HashMap<DefinitionId, u32>,
        turn_counts: HashMap<DefinitionId, u32>,
        turn_index: u32,
        rng_seed: i32,
        previous_random: i32,
    ) -> Self {
        Self {
            globals,
            visit_counts,
            turn_counts,
            turn_index,
            rng_seed,
            previous_random,
            policy: Box::new(ResolvedPolicy::all_world()),
        }
    }
}

/// A flow-local override of the shared RNG stream (`rng_seed` +
/// `previous_random`), the two scalars [`WorldPolicy::rng`] scopes as a
/// single unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LocalRng {
    pub seed: i32,
    pub previous_random: i32,
}

/// An immutable snapshot of a [`FlowLocal`]'s override layer, frozen via
/// [`FlowLocal::freeze`].
///
/// `FrozenLocal` chains: its own `base` is whatever the source `FlowLocal`'s
/// base was at freeze time, so a chain of forks (F3.2) can walk arbitrarily
/// far back through frozen ancestors. Cloning a `FrozenLocal` reference is
/// cheap — callers hold it behind an [`Arc`].
///
/// Nothing constructs a chain longer than one link in this PR (F3.1): no
/// fork exists yet, so `freeze` is unused in production code until F3.2's
/// `World::fork` calls it to snapshot a parent `FlowLocal` as a child's
/// base.
#[derive(Debug, Clone, Default)]
pub struct FrozenLocal {
    /// Overridden values for globals `ResolvedPolicy` homes to `Local`,
    /// keyed by slot index.
    globals: BTreeMap<u32, Value>,
    /// Overridden visit counts for knots/stitches homed to `Local`.
    visit_counts: BTreeMap<DefinitionId, u32>,
    /// Overridden turn counts for knots homed to `Local`.
    turn_counts: BTreeMap<DefinitionId, u32>,
    /// Overridden turn index, when `turn_index_scope() == Local`.
    turn_index: Option<u32>,
    /// Overridden RNG stream, when `rng_scope() == Local`.
    rng: Option<LocalRng>,
    /// The next link in the chain, if this snapshot's source `FlowLocal`
    /// itself had a base at freeze time.
    base: Option<Arc<FrozenLocal>>,
}

impl FrozenLocal {
    /// Chain-lookup a global override: this snapshot's own overrides, else
    /// recurse into `base`. Returns `None` on a miss all the way down the
    /// chain — the caller falls through to `World`.
    fn chain_get_global(&self, idx: u32) -> Option<&Value> {
        self.globals
            .get(&idx)
            .or_else(|| self.base.as_deref().and_then(|b| b.chain_get_global(idx)))
    }

    /// Chain-lookup a visit-count override.
    fn chain_get_visit_count(&self, id: DefinitionId) -> Option<u32> {
        self.visit_counts.get(&id).copied().or_else(|| {
            self.base
                .as_deref()
                .and_then(|b| b.chain_get_visit_count(id))
        })
    }

    /// Chain-lookup a turn-count override.
    fn chain_get_turn_count(&self, id: DefinitionId) -> Option<u32> {
        self.turn_counts.get(&id).copied().or_else(|| {
            self.base
                .as_deref()
                .and_then(|b| b.chain_get_turn_count(id))
        })
    }

    /// Chain-lookup the overridden turn index.
    fn chain_get_turn_index(&self) -> Option<u32> {
        self.turn_index.or_else(|| {
            self.base
                .as_deref()
                .and_then(FrozenLocal::chain_get_turn_index)
        })
    }

    /// Chain-lookup the overridden RNG stream.
    fn chain_get_rng(&self) -> Option<LocalRng> {
        self.rng
            .or_else(|| self.base.as_deref().and_then(FrozenLocal::chain_get_rng))
    }
}

/// Per-flow override layer over the shared [`World`].
///
/// **F3.1: copy-on-write, frozen-base read-through chain.** Each field is a
/// plain map/option holding this flow's own overrides for units
/// [`ResolvedPolicy`] homes to [`Scope::Local`], plus an optional `base`: an
/// immutable [`FrozenLocal`] snapshot (see [`FlowLocal::freeze`]) of another
/// `FlowLocal`, captured at some earlier point. A read walks **own
/// overrides → base (recursively) → [miss]**; [`ContextView`] treats a miss
/// as "not in the local chain" and falls through to `World`, exactly as in
/// F2.2. Writes always land in the flow's own top-layer overrides — never
/// in `base`, which is immutable by construction.
///
/// A fresh `FlowLocal` (via `Default`/[`FlowLocal::new`]) has empty
/// overrides and `base: None`, so it contributes no reads and every access
/// falls through to `World` — this is what keeps the all-`World` policy
/// (and every construction path in this PR, since nothing populates `base`
/// yet) byte-identical to the F2.2 flat-storage behavior. F3.2's
/// `World::fork` is what actually populates a child's `base` by freezing
/// its parent.
///
/// [`ContextView`] (below) is what actually consults these maps; see its
/// docs for the read-through/copy-on-write-increment semantics.
#[derive(Debug, Clone, Default)]
pub struct FlowLocal {
    /// Overridden values for globals `ResolvedPolicy` homes to `Local`,
    /// keyed by slot index.
    globals: BTreeMap<u32, Value>,
    /// Overridden visit counts for knots/stitches homed to `Local`.
    visit_counts: BTreeMap<DefinitionId, u32>,
    /// Overridden turn counts for knots homed to `Local`.
    turn_counts: BTreeMap<DefinitionId, u32>,
    /// Overridden turn index, when `turn_index_scope() == Local`.
    turn_index: Option<u32>,
    /// Overridden RNG stream, when `rng_scope() == Local`.
    rng: Option<LocalRng>,
    /// Frozen snapshot of an earlier `FlowLocal`'s overrides, read *after*
    /// this layer's own overrides on a miss. Always `None` in this PR — no
    /// fork exists yet to populate it (F3.2).
    base: Option<Arc<FrozenLocal>>,
}

impl FlowLocal {
    /// Construct an empty flow-local layer — overrides nothing and has no
    /// base, so every access routes through to `World`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Freeze this `FlowLocal`'s current state into an immutable
    /// [`FrozenLocal`] snapshot, suitable for use as another `FlowLocal`'s
    /// `base`.
    ///
    /// Captures the override maps (cloned — cheap, since only `Local`-scoped
    /// units are ever present) and cheap-clones the current `base` `Arc` so
    /// the new snapshot chains to the same ancestry this `FlowLocal` had.
    ///
    /// Nothing calls this yet in this PR — it exists as the foundation
    /// F3.2's `World::fork` builds on to snapshot a parent into a child's
    /// `base`.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "F3.2's World::fork calls this to snapshot a parent's FlowLocal into a child's base; F3.1's chain read-through test exercises it"
        )
    )]
    fn freeze(&self) -> Arc<FrozenLocal> {
        Arc::new(FrozenLocal {
            globals: self.globals.clone(),
            visit_counts: self.visit_counts.clone(),
            turn_counts: self.turn_counts.clone(),
            turn_index: self.turn_index,
            rng: self.rng,
            base: self.base.clone(),
        })
    }

    /// Chain-lookup a global override: own overrides → `base` (recursively)
    /// → `None` on a total miss, which [`ContextView`] treats as "fall
    /// through to `World`".
    fn chain_get_global(&self, idx: u32) -> Option<&Value> {
        self.globals
            .get(&idx)
            .or_else(|| self.base.as_deref().and_then(|b| b.chain_get_global(idx)))
    }

    /// Chain-lookup a visit-count override.
    fn chain_get_visit_count(&self, id: DefinitionId) -> Option<u32> {
        self.visit_counts.get(&id).copied().or_else(|| {
            self.base
                .as_deref()
                .and_then(|b| b.chain_get_visit_count(id))
        })
    }

    /// Chain-lookup a turn-count override.
    fn chain_get_turn_count(&self, id: DefinitionId) -> Option<u32> {
        self.turn_counts.get(&id).copied().or_else(|| {
            self.base
                .as_deref()
                .and_then(|b| b.chain_get_turn_count(id))
        })
    }

    /// Chain-lookup the overridden turn index.
    fn chain_get_turn_index(&self) -> Option<u32> {
        self.turn_index.or_else(|| {
            self.base
                .as_deref()
                .and_then(FrozenLocal::chain_get_turn_index)
        })
    }

    /// Chain-lookup the overridden RNG stream.
    fn chain_get_rng(&self) -> Option<LocalRng> {
        self.rng
            .or_else(|| self.base.as_deref().and_then(FrozenLocal::chain_get_rng))
    }
}

/// Routing view implementing [`ContextAccess`] over `(&mut World, &mut
/// FlowLocal)`.
///
/// This is what the VM's drive path receives as its `impl ContextAccess`.
/// Every op consults `world.policy` (a [`ResolvedPolicy`]) to decide, per
/// unit, whether it routes to the shared `World` or the private `FlowLocal`:
///
/// - **`World`-scoped**: always routes straight to `World` — reads and
///   writes are immediately visible to every flow sharing that `World`.
/// - **`Local`-scoped, read**: **chain read-through** — walks the
///   `FlowLocal`'s own overrides, then its frozen `base` (recursively, see
///   [`FlowLocal::chain_get_global`] and friends), then falls back to
///   `World`'s current value on a total miss (so a flow that has never
///   written a Local unit, nor inherited one from a base, sees the shared
///   default until its first local write).
/// - **`Local`-scoped, write**: lands in the `FlowLocal`'s own top-layer
///   overrides only; `World` (and any frozen `base`) is untouched.
/// - **`Local`-scoped, increment** (`increment_visit`,
///   `increment_turn_index`): copy-on-write from the *chain read-through*
///   value — read the current value (own override, else base chain, else
///   World fallback), add one, store the result as the new top-layer
///   override. This is what makes a flow's first local increment start
///   from the chain's (or World's) count rather than 0.
///
/// Because the all-`World` policy (the only policy the oracle corpus
/// exercises) takes the `World` branch on every op, this is byte-identical
/// to the F1.3 all-`World` passthrough for every existing single-flow
/// construction path.
pub struct ContextView<'a> {
    world: &'a mut World,
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
        match self.world.policy.scope_of_global(idx) {
            Scope::Local => self
                .local
                .chain_get_global(idx)
                .unwrap_or_else(|| self.world.global(idx)),
            Scope::World => self.world.global(idx),
        }
    }

    #[inline]
    fn set_global(&mut self, idx: u32, value: Value) {
        match self.world.policy.scope_of_global(idx) {
            Scope::Local => {
                self.local.globals.insert(idx, value);
            }
            Scope::World => self.world.set_global(idx, value),
        }
    }

    #[inline]
    fn visit_count(&self, id: DefinitionId) -> u32 {
        match self.world.policy.scope_of_knot(id) {
            Scope::Local => self
                .local
                .chain_get_visit_count(id)
                .unwrap_or_else(|| self.world.visit_count(id)),
            Scope::World => self.world.visit_count(id),
        }
    }

    #[inline]
    fn increment_visit(&mut self, id: DefinitionId) {
        match self.world.policy.scope_of_knot(id) {
            Scope::Local => {
                let base = self.visit_count(id);
                self.local.visit_counts.insert(id, base + 1);
            }
            Scope::World => self.world.increment_visit(id),
        }
    }

    #[inline]
    fn turn_count(&self, id: DefinitionId) -> Option<u32> {
        match self.world.policy.scope_of_knot(id) {
            Scope::Local => self
                .local
                .chain_get_turn_count(id)
                .or_else(|| self.world.turn_count(id)),
            Scope::World => self.world.turn_count(id),
        }
    }

    #[inline]
    fn set_turn_count(&mut self, id: DefinitionId, turn: u32) {
        match self.world.policy.scope_of_knot(id) {
            Scope::Local => {
                self.local.turn_counts.insert(id, turn);
            }
            Scope::World => self.world.set_turn_count(id, turn),
        }
    }

    #[inline]
    fn turn_index(&self) -> u32 {
        match self.world.policy.turn_index_scope() {
            Scope::Local => self
                .local
                .chain_get_turn_index()
                .unwrap_or_else(|| self.world.turn_index()),
            Scope::World => self.world.turn_index(),
        }
    }

    #[inline]
    fn increment_turn_index(&mut self) {
        match self.world.policy.turn_index_scope() {
            Scope::Local => {
                let base = self.turn_index();
                self.local.turn_index = Some(base + 1);
            }
            Scope::World => self.world.increment_turn_index(),
        }
    }

    #[inline]
    fn rng_seed(&self) -> i32 {
        match self.world.policy.rng_scope() {
            Scope::Local => self
                .local
                .chain_get_rng()
                .map_or_else(|| self.world.rng_seed(), |rng| rng.seed),
            Scope::World => self.world.rng_seed(),
        }
    }

    #[inline]
    fn set_rng_seed(&mut self, seed: i32) {
        match self.world.policy.rng_scope() {
            Scope::Local => {
                // CoW from the chain read-through value: seed a fresh local
                // override from the base chain's RNG if present, else World.
                // (base is always None in F3.1, so this reduces to World.)
                let fallback = self.local.chain_get_rng().unwrap_or(LocalRng {
                    seed: self.world.rng_seed(),
                    previous_random: self.world.previous_random(),
                });
                let rng = self.local.rng.get_or_insert(fallback);
                rng.seed = seed;
            }
            Scope::World => self.world.set_rng_seed(seed),
        }
    }

    #[inline]
    fn previous_random(&self) -> i32 {
        match self.world.policy.rng_scope() {
            Scope::Local => self
                .local
                .chain_get_rng()
                .map_or_else(|| self.world.previous_random(), |rng| rng.previous_random),
            Scope::World => self.world.previous_random(),
        }
    }

    #[inline]
    fn set_previous_random(&mut self, val: i32) {
        match self.world.policy.rng_scope() {
            Scope::Local => {
                // CoW from the chain read-through value (see `set_rng_seed`).
                let fallback = self.local.chain_get_rng().unwrap_or(LocalRng {
                    seed: self.world.rng_seed(),
                    previous_random: self.world.previous_random(),
                });
                let rng = self.local.rng.get_or_insert(fallback);
                rng.previous_random = val;
            }
            Scope::World => self.world.set_previous_random(val),
        }
    }

    #[inline]
    fn next_random<R: StoryRng>(&self, seed: i32) -> i32 {
        // Pure function of the explicit `seed` argument — not routed
        // state, so this delegates to `World`'s implementation unchanged.
        // The routed `rng_seed()` above is what call sites read to obtain
        // `seed` in the first place.
        self.world.next_random::<R>(seed)
    }

    fn random_sequence<R: StoryRng>(&self, seed: i32, count: usize) -> Vec<i32> {
        self.world.random_sequence::<R>(seed, count)
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    use crate::link;

    /// Compile a small ink story with the brink compiler and link it, for
    /// resolving policies against a real `Program` symbol table.
    fn compile(src: &str) -> Program {
        let out = brink_compiler::compile("t.ink", |p| {
            if p == "t.ink" {
                Ok(src.to_string())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no such include",
                ))
            }
        })
        .expect("compile");
        let (program, _line_tables) = link(&out.data).expect("link");
        program
    }

    fn sample_program() -> Program {
        compile(
            "VAR gold = 0\n\
             VAR mood = 0\n\
             -> shrine\n\
             === shrine ===\n\
             At the shrine.\n\
             -> END\n\
             === cellar ===\n\
             In the cellar.\n\
             -> END\n",
        )
    }

    /// The default `WorldPolicy` (all-`World`) must resolve via the fast
    /// path — no name lookups — and every scope must read back as `World`.
    #[test]
    fn all_world_default_resolves_via_fast_path() {
        let program = sample_program();
        let policy = WorldPolicy::default();

        let resolved = ResolvedPolicy::resolve(&program, &policy).expect("resolves");

        // Fast path: no per-slot table populated.
        assert!(resolved.global_scopes.is_empty());
        assert!(resolved.knot_scopes.is_empty());

        let gold_slot = program.global_index("gold").expect("gold declared");
        let mood_slot = program.global_index("mood").expect("mood declared");
        let shrine_id = program.find_path_target("shrine").expect("shrine exists");
        let cellar_id = program.find_path_target("cellar").expect("cellar exists");

        assert_eq!(resolved.scope_of_global(gold_slot), Scope::World);
        assert_eq!(resolved.scope_of_global(mood_slot), Scope::World);
        assert_eq!(resolved.scope_of_knot(shrine_id), Scope::World);
        assert_eq!(resolved.scope_of_knot(cellar_id), Scope::World);
        assert_eq!(resolved.turn_index_scope(), Scope::World);
        assert_eq!(resolved.rng_scope(), Scope::World);
    }

    /// A policy with `default: Local` plus explicit variable and knot
    /// overrides resolves each override to its named scope and leaves
    /// everything else at the default.
    #[test]
    fn resolves_valid_variable_and_knot_overrides() {
        let program = sample_program();
        let mut overrides = BTreeMap::new();
        overrides.insert("gold".to_owned(), Scope::World);
        overrides.insert("shrine".to_owned(), Scope::World);
        let policy = WorldPolicy {
            default: Scope::Local,
            overrides,
            turn_index: Scope::Local,
            rng: Scope::Local,
        };

        let resolved = ResolvedPolicy::resolve(&program, &policy).expect("resolves");

        let gold_slot = program.global_index("gold").expect("gold declared");
        let mood_slot = program.global_index("mood").expect("mood declared");
        let shrine_id = program.find_path_target("shrine").expect("shrine exists");
        let cellar_id = program.find_path_target("cellar").expect("cellar exists");

        // Named overrides win.
        assert_eq!(resolved.scope_of_global(gold_slot), Scope::World);
        assert_eq!(resolved.scope_of_knot(shrine_id), Scope::World);

        // Unlisted variable/knot fall back to `default`.
        assert_eq!(resolved.scope_of_global(mood_slot), Scope::Local);
        assert_eq!(resolved.scope_of_knot(cellar_id), Scope::Local);

        // Scalars resolve independently of `overrides`.
        assert_eq!(resolved.turn_index_scope(), Scope::Local);
        assert_eq!(resolved.rng_scope(), Scope::Local);
    }

    /// A name in `overrides` that matches neither a declared variable nor a
    /// resolvable knot/stitch path is a `PolicyError`, not a silent
    /// fallback to `default`.
    #[test]
    fn unknown_override_name_is_an_error() {
        let program = sample_program();
        let mut overrides = BTreeMap::new();
        overrides.insert("not_a_real_name".to_owned(), Scope::World);
        let policy = WorldPolicy {
            default: Scope::World,
            overrides,
            turn_index: Scope::World,
            rng: Scope::World,
        };

        let err = ResolvedPolicy::resolve(&program, &policy).expect_err("must fail");
        assert_eq!(err, PolicyError::UnknownName("not_a_real_name".to_owned()));
    }

    /// `World::new` resolves the policy and constructs a `World` whose
    /// globals match the program's declared defaults.
    #[test]
    fn world_new_resolves_policy_and_initializes_globals() {
        let program = sample_program();
        let world = World::new(&program, &WorldPolicy::default()).expect("world builds");
        assert_eq!(world.globals, program.global_defaults());
    }

    /// `World::new` propagates the resolver's error for an unknown override
    /// name rather than panicking or silently ignoring it.
    #[test]
    fn world_new_propagates_unknown_name_error() {
        let program = sample_program();
        let mut overrides = BTreeMap::new();
        overrides.insert("nonexistent".to_owned(), Scope::Local);
        let policy = WorldPolicy {
            overrides,
            ..WorldPolicy::default()
        };

        let err = World::new(&program, &policy).expect_err("must fail");
        assert_eq!(err, PolicyError::UnknownName("nonexistent".to_owned()));
    }
}

#[cfg(test)]
mod routing_tests {
    use super::*;
    use crate::link;

    /// Compile a small ink story with the brink compiler and link it, for
    /// resolving policies against a real `Program` symbol table.
    fn compile(src: &str) -> Program {
        let out = brink_compiler::compile("t.ink", |p| {
            if p == "t.ink" {
                Ok(src.to_string())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no such include",
                ))
            }
        })
        .expect("compile");
        let (program, _line_tables) = link(&out.data).expect("link");
        program
    }

    fn sample_program() -> Program {
        compile(
            "VAR gold = 0\n\
             VAR mood = 0\n\
             -> shrine\n\
             === shrine ===\n\
             At the shrine.\n\
             -> END\n\
             === cellar ===\n\
             In the cellar.\n\
             -> END\n",
        )
    }

    /// `mood` is Local, `gold` stays World (the default). `shrine`/`cellar`
    /// are left at the World default too, so visit counts there stay
    /// shared — exercised separately below.
    fn mixed_policy() -> WorldPolicy {
        let mut overrides = BTreeMap::new();
        overrides.insert("mood".to_owned(), Scope::Local);
        WorldPolicy {
            default: Scope::World,
            overrides,
            turn_index: Scope::World,
            rng: Scope::World,
        }
    }

    /// Writing a `Local`-scoped global in one flow must not affect another
    /// flow's `ContextView` over the same `World`, nor the `World` itself.
    /// Writing a `World`-scoped global in one flow must be immediately
    /// visible through another flow's `ContextView`.
    #[test]
    fn local_write_isolated_world_write_shared() {
        let program = sample_program();
        let policy = mixed_policy();
        let mut world = World::new(&program, &policy).expect("world builds");

        let gold_slot = program.global_index("gold").expect("gold declared");
        let mood_slot = program.global_index("mood").expect("mood declared");

        let mut local_a = FlowLocal::new();
        let mut local_b = FlowLocal::new();

        // Flow A writes its Local `mood`.
        {
            let mut view_a = ContextView::new(&mut world, &mut local_a);
            view_a.set_global(mood_slot, Value::Int(42));
            assert_eq!(view_a.global(mood_slot), &Value::Int(42));
        }

        // Flow B's view over the same World must not see A's local write —
        // it read-throughs to World's untouched default.
        {
            let view_b = ContextView::new(&mut world, &mut local_b);
            assert_eq!(view_b.global(mood_slot), &Value::Int(0));
            // World itself is untouched too.
            assert_eq!(world.global(mood_slot), &Value::Int(0));
        }

        // Flow A writes its World-scoped `gold` — this must be immediately
        // visible via Flow B's view (and via World directly).
        {
            let mut view_a = ContextView::new(&mut world, &mut local_a);
            view_a.set_global(gold_slot, Value::Int(7));
        }
        {
            let view_b = ContextView::new(&mut world, &mut local_b);
            assert_eq!(view_b.global(gold_slot), &Value::Int(7));
        }
        assert_eq!(world.global(gold_slot), &Value::Int(7));
    }

    /// Local visit counts increment independently per flow, while a
    /// World-scoped knot's visits are shared across flows.
    #[test]
    fn local_visits_independent_world_visits_shared() {
        let program = sample_program();

        // shrine: Local: cellar stays at the World default.
        let mut overrides = BTreeMap::new();
        overrides.insert("shrine".to_owned(), Scope::Local);
        let policy = WorldPolicy {
            default: Scope::World,
            overrides,
            turn_index: Scope::World,
            rng: Scope::World,
        };
        let mut world = World::new(&program, &policy).expect("world builds");

        let shrine_id = program.find_path_target("shrine").expect("shrine exists");
        let cellar_id = program.find_path_target("cellar").expect("cellar exists");

        let mut local_a = FlowLocal::new();
        let mut local_b = FlowLocal::new();

        // Flow A visits `shrine` (Local) twice.
        {
            let mut view_a = ContextView::new(&mut world, &mut local_a);
            view_a.increment_visit(shrine_id);
            view_a.increment_visit(shrine_id);
            assert_eq!(view_a.visit_count(shrine_id), 2);
        }
        // Flow B's local shrine count is independent — still 0.
        {
            let view_b = ContextView::new(&mut world, &mut local_b);
            assert_eq!(view_b.visit_count(shrine_id), 0);
        }
        // World's own bookkeeping for a Local-scoped knot is never touched.
        assert_eq!(world.visit_count(shrine_id), 0);

        // `cellar` (World-scoped): Flow A's increment is visible to Flow B.
        {
            let mut view_a = ContextView::new(&mut world, &mut local_a);
            view_a.increment_visit(cellar_id);
        }
        {
            let view_b = ContextView::new(&mut world, &mut local_b);
            assert_eq!(view_b.visit_count(cellar_id), 1);
        }
        assert_eq!(world.visit_count(cellar_id), 1);
    }

    /// A `Local`-scoped global reads through to World's current value until
    /// the flow performs its first local write.
    #[test]
    fn local_read_through_returns_world_default_before_first_write() {
        let program = sample_program();
        let policy = mixed_policy();
        let mut world = World::new(&program, &policy).expect("world builds");
        let mood_slot = program.global_index("mood").expect("mood declared");

        // World's `mood` starts at the program default (0). A later World
        // write (e.g. host bootstrapping) should read through too, since A
        // hasn't written its own local override yet.
        world.set_global(mood_slot, Value::Int(99));

        let mut local_a = FlowLocal::new();
        let view_a = ContextView::new(&mut world, &mut local_a);
        assert_eq!(view_a.global(mood_slot), &Value::Int(99));
    }

    /// Local visit-count increment is copy-on-write from the read-through
    /// value: if World already has a nonzero count when Local is scoped
    /// in, the flow's first local increment starts from that base, not 0.
    #[test]
    fn local_increment_is_cow_from_read_through_base() {
        let program = sample_program();
        let mut overrides = BTreeMap::new();
        overrides.insert("shrine".to_owned(), Scope::Local);
        let policy = WorldPolicy {
            default: Scope::World,
            overrides,
            turn_index: Scope::World,
            rng: Scope::World,
        };
        let mut world = World::new(&program, &policy).expect("world builds");
        let shrine_id = program.find_path_target("shrine").expect("shrine exists");

        // Seed World's bookkeeping directly (simulating pre-existing shared
        // state before this knot was scoped Local for this flow).
        world.increment_visit(shrine_id);
        world.increment_visit(shrine_id);
        assert_eq!(world.visit_count(shrine_id), 2);

        let mut local_a = FlowLocal::new();
        let mut view_a = ContextView::new(&mut world, &mut local_a);
        // First local increment must start from World's base (2), not 0.
        view_a.increment_visit(shrine_id);
        assert_eq!(view_a.visit_count(shrine_id), 3);

        // World's own count is untouched by the Local increment.
        assert_eq!(world.visit_count(shrine_id), 2);
    }

    /// F3.1 chain read-through: a `FlowLocal` with a frozen `base` reads a
    /// value from the base when its own top layer has no override, and its
    /// own top-layer override shadows the base. Values that appear in
    /// neither layer still fall through to `World`.
    ///
    /// Built by hand (no fork exists yet — that's F3.2): freeze a parent
    /// `FlowLocal` that has some overrides, then attach that snapshot as a
    /// child's `base`.
    #[test]
    fn chain_read_through_reads_base_and_top_shadows() {
        let program = sample_program();

        // Home both globals, both knots, turn index, and RNG to Local so the
        // chain (not World) is what's exercised on every read.
        let mut overrides = BTreeMap::new();
        overrides.insert("gold".to_owned(), Scope::Local);
        overrides.insert("mood".to_owned(), Scope::Local);
        overrides.insert("shrine".to_owned(), Scope::Local);
        overrides.insert("cellar".to_owned(), Scope::Local);
        let policy = WorldPolicy {
            default: Scope::World,
            overrides,
            turn_index: Scope::Local,
            rng: Scope::Local,
        };
        let mut world = World::new(&program, &policy).expect("world builds");

        let gold_slot = program.global_index("gold").expect("gold declared");
        let mood_slot = program.global_index("mood").expect("mood declared");
        let shrine_id = program.find_path_target("shrine").expect("shrine exists");
        let cellar_id = program.find_path_target("cellar").expect("cellar exists");

        // World defaults are distinct from anything we put in the chain, so a
        // read hitting World rather than the chain would be visible.
        world.set_global(gold_slot, Value::Int(1));
        world.set_global(mood_slot, Value::Int(1));

        // Build a parent FlowLocal with overrides, then freeze it.
        let mut parent = FlowLocal::new();
        {
            let mut pv = ContextView::new(&mut world, &mut parent);
            pv.set_global(gold_slot, Value::Int(100));
            pv.set_global(mood_slot, Value::Int(200));
            pv.increment_visit(shrine_id); // parent shrine visit = 1
            pv.set_turn_count(cellar_id, 5);
            pv.increment_turn_index(); // parent turn index = 1
            pv.set_rng_seed(777);
        }
        let base = parent.freeze();

        // Child inherits the frozen parent as its base, with its own empty
        // top layer.
        let mut child = FlowLocal {
            base: Some(base),
            ..FlowLocal::new()
        };

        // Reads with an empty top layer see the base's values (not World's).
        {
            let view = ContextView::new(&mut world, &mut child);
            assert_eq!(view.global(gold_slot), &Value::Int(100));
            assert_eq!(view.global(mood_slot), &Value::Int(200));
            assert_eq!(view.visit_count(shrine_id), 1);
            assert_eq!(view.turn_count(cellar_id), Some(5));
            assert_eq!(view.turn_index(), 1);
            assert_eq!(view.rng_seed(), 777);
        }

        // A top-layer override shadows the base for that one unit; other
        // units keep reading through to the base.
        {
            let mut view = ContextView::new(&mut world, &mut child);
            view.set_global(gold_slot, Value::Int(999));
            assert_eq!(view.global(gold_slot), &Value::Int(999)); // shadowed
            assert_eq!(view.global(mood_slot), &Value::Int(200)); // still base
        }

        // A knot with no override anywhere in the chain falls through to
        // World (whose count for the Local-scoped `cellar` is untouched: 0).
        {
            let view = ContextView::new(&mut world, &mut child);
            assert_eq!(view.visit_count(cellar_id), 0);
        }
    }
}
