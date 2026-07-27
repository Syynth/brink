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
//! F3.1 upgraded `FlowLocal`'s storage to a copy-on-write, frozen-base
//! read-through **chain**: `FlowLocal` gains an optional [`Arc<FrozenLocal>`]
//! base, an immutable snapshot of another `FlowLocal`'s overrides (captured
//! via [`FlowLocal::freeze`]) that can itself chain to a further base. A read
//! walks **own overrides → base (recursively) → [miss]**; a miss falls
//! through to `World` exactly as before. Writes still land only in the
//! flow's own top-layer overrides.
//!
//! **F3.2 (this stage)** adds **fork + sandbox mode + discard**: [`Mode`]
//! (`Normal`/`Sandbox`), baked onto a `FlowLocal` at construction/fork time,
//! and [`FlowLocal::fork`], which builds a child whose `base` is a frozen
//! snapshot of the parent (via `freeze`) and whose own overrides start
//! empty. `Normal` fork children route exactly like any other `FlowLocal` —
//! by policy. `Sandbox` children are the side-effect-proof primitive
//! watch/eval needs: `ContextView` treats **every** unit as `Local`
//! regardless of policy, so the shared `World` is a read-only base — reads
//! chain-read-through to `World`'s live value on a miss, but writes always
//! land in the sandboxed flow's own overrides and never reach `World`.
//! Discard is simply dropping the forked `FlowLocal`: since a `Sandbox`
//! child's writes never touched `World` (and a `Normal` child's writes never
//! touched its parent or `World` either — only its own top layer), there is
//! nothing to unwind. A deferred `commit` seam (fold a fork's writes back
//! into its parent) is documented but intentionally left unimplemented — see
//! [`CommitError`] and [`commit`].
//!
//! No existing construction path calls `fork` or requests `Mode::Sandbox` —
//! every flow the oracle corpus drives is `Mode::Normal` with `base: None`,
//! so `ContextView` takes exactly the F3.1 branch on every op. The all-
//! `World` policy (the default, and the only policy the oracle corpus
//! exercises) takes the `World` branch on every op, so `ContextView` stays
//! byte-identical to the F1.3 passthrough for every existing single-flow
//! construction path.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use brink_format::{DefinitionId, Value};

use crate::collections::Map as HashMap;
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
///
/// **Knot/stitch overrides are subtree-inclusive** (F6.1c —
/// `docs/scoped-flow-state-spec.md`'s F6 AMENDMENT, ruling 3): a knot
/// override covers the knot's own visit/turn count, every stitch nested
/// directly under it, and every interior container (weave/sequence/choice
/// container) nested anywhere under the knot or one of its stitches — not
/// just the knot's own `DefinitionId`. This matters because ink's
/// sequence/cycle/stopping machinery (`{ Halt! | Back again? }`) keys its
/// counter off the *sequence's own* interior container id, not the
/// enclosing knot's id; without subtree inclusion, a knot marked `Local`
/// would leave those interior counters silently `World`-scoped.
///
/// **Most-specific override wins.** If both a knot and one of its stitches
/// appear in `overrides` (e.g. knot `a` is `Local`, stitch `a.b` is
/// `World`), every interior container nested under `a.b` resolves `World`;
/// the rest of `a`'s subtree (the knot's own id, its other stitches, and
/// any interior container not under `a.b`) resolves `Local`. A stitch's
/// override always wins over its enclosing knot's for the stitch's own
/// subtree, regardless of which name appears earlier in `overrides` (see
/// [`ResolvedPolicy::resolve`] for how this is implemented).
#[derive(Debug, Clone, Default)]
pub struct WorldPolicy {
    /// Scope for any variable or knot/stitch not named in `overrides`.
    pub default: Scope,
    /// Per-name exceptions to `default`, for global variables (matched
    /// against `Program::global_index`'s name grammar) and knot/stitch
    /// paths (matched against `Program::find_path_target`'s path
    /// grammar). A name may appear in only one of the two — the resolver
    /// tries variables first, then knot paths. Knot/stitch overrides are
    /// subtree-inclusive with most-specific-wins precedence — see the type
    /// docs above.
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
    /// Non-default scope for a knot/stitch (or an interior weave/sequence/
    /// choice container nested under one), keyed by its defining
    /// `DefinitionId` (the same id `ContextAccess::visit_count` and friends
    /// are called with — e.g. `vm.rs`'s `handle_sequence` keys a stopping/
    /// cycle sequence's counter off the *sequence's own* interior container
    /// id, not its enclosing knot's).
    ///
    /// **Subtree-inclusive (F6.1c):** an override on a knot/stitch name is
    /// expanded at resolve time (see [`resolve`](Self::resolve)) to cover
    /// its own id, every stitch nested directly under it (if it's a knot),
    /// and every interior container nested anywhere under it or one of its
    /// stitches — not just the literal `DefinitionId` the override name
    /// resolved to. Only exceptions to `default` are stored — sparse, since
    /// most programs have far more knots/interior containers than
    /// overrides.
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
    /// **Subtree expansion (F6.1c).** Every knot/stitch override is
    /// expanded here, once, into every `DefinitionId` in its definition
    /// subtree — see [`expand_knot_scope`] for the containment mechanism
    /// (`Program::scope_ids`, the nearest-enclosing-scope table every
    /// interior container carries, plus `Program::address_by_path`'s
    /// dotted knot/stitch grammar for the one-level knot→stitch link that
    /// `scope_ids` alone doesn't carry). `overrides` is a `BTreeMap`, so
    /// this loop's iteration order is deterministic (sorted by name) — and
    /// because a knot's name is always a proper prefix of (and therefore
    /// sorts lexicographically before) any of its stitches' names, a knot
    /// override's subtree expansion always runs *before* a same-subtree
    /// stitch override in this same pass, so the stitch override's own
    /// `insert`s (which land later) win — implementing "most-specific
    /// override wins" as a natural consequence of processing order, no
    /// separate precedence pass needed.
    ///
    /// **Compiled base layer.** Before host overrides apply, `resolve`
    /// seeds scopes from the `Program`'s compiled `#@local` defaults
    /// (`docs/directive-annotations-spec.md`): globals the compiler
    /// marked flow-private seed `Local`, and every `#@local` knot/stitch
    /// expands over its subtree exactly like a host override would.
    /// Host overrides then layer on top — `base ⊕ host-overrides` — so a
    /// host name always beats the compiled bit for that name's subtree.
    ///
    /// The all-`World` default (empty `overrides`, no compiled `#@local`
    /// bits) resolves without any name lookups (see
    /// [`all_world`](Self::all_world)) — this is the fast path every
    /// unannotated single-flow program takes.
    pub fn resolve(program: &Program, policy: &WorldPolicy) -> Result<Self, PolicyError> {
        if policy.overrides.is_empty()
            && policy.default == Scope::World
            && policy.turn_index == Scope::World
            && policy.rng == Scope::World
            && !program.has_local_defaults()
        {
            return Ok(Self::all_world());
        }

        // Seed globals from the compiled base: `#@local` beats the host
        // default; everything unmarked follows the host default.
        let mut global_scopes: Vec<Scope> = (0..program.global_count())
            .map(|slot| {
                if program.global_is_local(slot) {
                    Scope::Local
                } else {
                    policy.default
                }
            })
            .collect();
        let mut knot_scopes = HashMap::new();
        let interior_by_scope = interior_containers_by_scope(program);

        // Seed knots/stitches from the compiled base. The list is sorted
        // by path at link time, so a `#@local` knot expands before any of
        // its own `#@local` stitches — same ordering argument as the
        // override loop below.
        for (path, id) in program.local_scope_defaults() {
            expand_knot_scope(
                program,
                &interior_by_scope,
                &mut knot_scopes,
                path,
                *id,
                Scope::Local,
            );
        }

        // `overrides` is a `BTreeMap`, so iteration order is deterministic
        // (sorted by name) — resolution never depends on hash-map order.
        for (name, &scope) in &policy.overrides {
            if let Some(slot) = program.global_index(name) {
                global_scopes[slot as usize] = scope;
            } else if let Some(id) = program.find_path_target(name) {
                expand_knot_scope(
                    program,
                    &interior_by_scope,
                    &mut knot_scopes,
                    name,
                    id,
                    scope,
                );
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

    /// Scope of a knot/stitch — or of an interior weave/sequence/choice
    /// container nested under one — by its defining `DefinitionId`. See the
    /// `knot_scopes` field docs and [`resolve`](Self::resolve) for how a
    /// knot/stitch override is expanded, at resolve time, to cover every id
    /// in its definition subtree.
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

// ── Subtree-inclusive knot scope (F6.1c) ────────────────────────────────
//
// Containment mechanism, verified against a compiled `Program` (see the
// `subtree_scope_tests` module below and the investigation in the F6.1c
// build log — not restated here):
//
// - `ContainerDef::scope_id` (preserved on `Program` as the parallel
//   `scope_ids`/`scope_table_idx` tables) gives every container — knot,
//   stitch, root, or an anonymous interior weave/sequence/choice
//   container, at any nesting depth — the `DefinitionId` of its *nearest
//   enclosing* knot/stitch/root scope, correctly propagated through
//   arbitrarily deep nesting by the codegen's recursive walk. A container
//   is itself a scope owner (a knot, stitch, or root) exactly when its own
//   `scope_id` equals its own id — self-scoped, no parent. This gives an
//   exact, structural (not name-based) map from any interior container to
//   its owning knot/stitch: `interior_containers_by_scope`, below.
// - `scope_id` does *not* link a stitch to its enclosing knot (a stitch is
//   self-scoped, by the same rule above) — ink only nests stitches one
//   level under a knot, and that one link has to come from
//   `Program::address_by_path`'s dotted qualified-path grammar (the same
//   table `find_address`/`find_path_target` already use): a knot named `N`
//   knows its direct stitches are exactly the `address_by_path` entries
//   `"N.<segment>"` with no further `.` in `<segment>`, whose target is
//   itself a scope owner (ruling out an author-labeled gather directly in
//   the knot, which shares the same two-segment path shape but is *not*
//   self-scoped).
//
// Both of these are compile-time/link-time structural facts already
// present on `Program` — no id arithmetic or heuristic string matching on
// unstructured names.

/// Group every non-scope-owning ("interior") container's own id by the
/// `DefinitionId` of its nearest enclosing knot/stitch/root scope.
///
/// Built once per non-fast-path [`ResolvedPolicy::resolve`] call — this is
/// resolve-time bookkeeping, not a hot-path lookup (the all-`World` fast
/// path never calls this at all).
fn interior_containers_by_scope(program: &Program) -> HashMap<DefinitionId, Vec<DefinitionId>> {
    let mut by_scope: HashMap<DefinitionId, Vec<DefinitionId>> = HashMap::new();
    for (idx, container) in program.containers.iter().enumerate() {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "container count fits in u32"
        )]
        let idx = idx as u32;
        let owner = program.scope_ids[program.scope_table_idx(idx) as usize];
        if owner != container.id {
            by_scope.entry(owner).or_default().push(container.id);
        }
    }
    by_scope
}

/// Apply `scope` to `id` and every interior container `interior_by_scope`
/// says is nested directly under it (i.e. `id`'s own subtree, one scope
/// level — not a recursive walk, since ink knots/stitches never nest
/// containers whose `scope_id` chain needs more than one enclosing-scope
/// hop to resolve; see `interior_containers_by_scope`).
fn apply_scope_to_subtree(
    interior_by_scope: &HashMap<DefinitionId, Vec<DefinitionId>>,
    knot_scopes: &mut HashMap<DefinitionId, Scope>,
    id: DefinitionId,
    scope: Scope,
) {
    knot_scopes.insert(id, scope);
    if let Some(interior) = interior_by_scope.get(&id) {
        for &child_id in interior {
            knot_scopes.insert(child_id, scope);
        }
    }
}

/// Expand a single `WorldPolicy::overrides` knot/stitch entry (`name` →
/// `id`, already resolved via `Program::find_path_target`) into every
/// `DefinitionId` in its definition subtree, writing `scope` for each into
/// `knot_scopes`.
///
/// Covers: `id`'s own subtree (its own id plus its direct interior
/// containers), then — since a stitch override's own call to this function
/// already covers everything a stitch can own, and ink never nests a
/// stitch under another stitch — cascades once to `name`'s direct child
/// stitches (found via `address_by_path`'s dotted grammar, see the module
/// docs above) and covers each of *their* subtrees too. A child stitch
/// that itself has a more specific override is still safe to cascade into
/// here: `resolve`'s `BTreeMap` iteration order guarantees the stitch's own
/// (later, more specific) entry is processed after `name`'s and overwrites
/// whatever this cascade wrote — see `resolve`'s docs.
fn expand_knot_scope(
    program: &Program,
    interior_by_scope: &HashMap<DefinitionId, Vec<DefinitionId>>,
    knot_scopes: &mut HashMap<DefinitionId, Scope>,
    name: &str,
    id: DefinitionId,
    scope: Scope,
) {
    apply_scope_to_subtree(interior_by_scope, knot_scopes, id, scope);

    let prefix = format!("{name}.");
    for (path, target) in &program.address_by_path {
        let Some(rest) = path.strip_prefix(prefix.as_str()) else {
            continue;
        };
        if rest.is_empty() || rest.contains('.') {
            continue; // Not a direct one-segment child of `name`.
        }
        if target.byte_offset != 0 {
            continue; // Not a container's own primary address.
        }
        // Confirm the target is itself a scope-owning container (a real
        // stitch), not an author-labeled gather directly in the knot that
        // happens to share the same two-segment path shape.
        let owner = program.scope_ids[program.scope_table_idx(target.container_idx) as usize];
        if owner != target.id {
            continue;
        }
        apply_scope_to_subtree(interior_by_scope, knot_scopes, target.id, scope);
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
/// base was at freeze time, so a chain of forks ([`FlowLocal::fork`]) can
/// walk arbitrarily far back through frozen ancestors. Cloning a
/// `FrozenLocal` reference is cheap — callers hold it behind an [`Arc`].
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

/// Execution mode of a [`FlowLocal`], baked in at construction/fork time and
/// read by [`ContextView`] to decide how it routes every unit.
///
/// `Mode` is orthogonal to [`WorldPolicy`]/[`ResolvedPolicy`]: policy homes a
/// *unit* (a global, a knot's visit count, …) to `World` or `Local`; `Mode`
/// decides, for *this flow*, whether that homing is honored at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Route every unit by policy, exactly as [`ContextView`]'s F2.2/F3.1
    /// docs describe: `World`-scoped units go straight to `World`,
    /// `Local`-scoped units chain-read-through/write to `FlowLocal`. Every
    /// construction path before F3.2 produces `Mode::Normal` — this is what
    /// keeps the oracle corpus byte-identical.
    #[default]
    Normal,
    /// Treat **every** unit as `Local`, regardless of policy: the shared
    /// `World` becomes a read-only base for this flow. Reads still
    /// chain-read-through to `World`'s current value on a total miss (so a
    /// sandboxed flow sees live world state), but writes always land in
    /// this flow's own top-layer overrides — `World` (and any `Normal`
    /// ancestor) is never mutated. Combined with [`FlowLocal::fork`]'s
    /// frozen base, this is the side-effect-proof primitive watch/eval
    /// needs: run a flow against current state, observe its output, then
    /// discard it (drop) with the shared world untouched.
    Sandbox,
}

/// Per-flow override layer over the shared [`World`].
///
/// **F3.1: copy-on-write, frozen-base read-through chain.** Each field is a
/// plain map/option holding this flow's own overrides for units
/// [`ResolvedPolicy`] homes to [`Scope::Local`] (or, in [`Mode::Sandbox`],
/// *every* unit — see [`Mode`]), plus an optional `base`: an immutable
/// [`FrozenLocal`] snapshot (see [`FlowLocal::freeze`]) of another
/// `FlowLocal`, captured at some earlier point. A read walks **own
/// overrides → base (recursively) → [miss]**; [`ContextView`] treats a miss
/// as "not in the local chain" and falls through to `World`, exactly as in
/// F2.2. Writes always land in the flow's own top-layer overrides — never
/// in `base`, which is immutable by construction.
///
/// A fresh `FlowLocal` (via `Default`/[`FlowLocal::new`]) has empty
/// overrides, `base: None`, and `mode: Mode::Normal`, so it contributes no
/// reads and every access falls through to `World` — this is what keeps the
/// all-`World` policy (and every construction path that doesn't call
/// [`fork`](Self::fork)) byte-identical to the F2.2 flat-storage behavior.
/// [`FlowLocal::fork`] (F3.2) is what actually populates a child's `base` by
/// freezing its parent, and what bakes in a non-`Normal` `mode`.
///
/// [`ContextView`] (below) is what actually consults these maps; see its
/// docs for the read-through/copy-on-write-increment/mode semantics.
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
    /// this layer's own overrides on a miss. Populated by [`FlowLocal::fork`];
    /// `None` for every construction path that doesn't fork.
    base: Option<Arc<FrozenLocal>>,
    /// This flow's execution mode — see [`Mode`]. Baked in at construction
    /// (`Mode::Normal`, the `Default`) or at [`FlowLocal::fork`] time.
    mode: Mode,
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
    /// Called by [`FlowLocal::fork`] to snapshot a parent into a child's
    /// `base`.
    #[must_use]
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

    /// Fork a child `FlowLocal` from this one.
    ///
    /// The child's `base` is a frozen, point-in-time snapshot of `self` (via
    /// [`freeze`](Self::freeze)) — an `O(1)`-ish operation that clones this
    /// flow's own (small) override maps and `Arc`-bumps the rest of the
    /// ancestry chain, never a full `World` copy. The child's own override
    /// layer starts empty, and it runs in `mode` for its lifetime (`Mode` is
    /// baked in here, not mutable afterward).
    ///
    /// Because the base is frozen, later mutations to `self` (the parent)
    /// are **not** visible to the child — the child sees the parent exactly
    /// as it was at fork time. Symmetrically, nothing the child does is ever
    /// visible to `self` or `World`: writes land only in the child's own top
    /// layer (see [`Mode`] for how `Sandbox` additionally diverts
    /// `World`-scoped writes there too). That makes **discard** trivial —
    /// dropping the returned `FlowLocal` is the entire discard operation, no
    /// unwinding required. Folding a child's writes back into `self` instead
    /// of discarding them is the deferred `commit` seam — see [`CommitError`]
    /// and [`commit`].
    #[must_use]
    pub fn fork(&self, mode: Mode) -> FlowLocal {
        FlowLocal {
            base: Some(self.freeze()),
            mode,
            ..FlowLocal::new()
        }
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

/// Errors from [`commit`].
///
/// A single variant today: `commit` is a documented, deferred seam (see
/// `docs/scoped-flow-state-spec.md`, "Write-back is determined by scope, not
/// a separate knob") — this release ships fork + discard only.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum CommitError {
    /// Folding a forked child's own-layer overrides back into its parent is
    /// deferred past this release. Fork's only supported terminal operation
    /// today is **discard** (drop the child); `commit` always returns this.
    #[error(
        "commit is not implemented in this release; fork's only supported terminal operation is discard (drop the child)"
    )]
    NotImplemented,
}

/// Fold a forked child's own-layer overrides back into its parent's,
/// making the child's writes visible through `parent` (and, transitively,
/// anything `parent` itself chains to or is later written through).
///
/// **This is a deferred seam, not implemented in this release.** It exists
/// so the shape of the eventual write-back API is fixed and callers can
/// write code against it now (always getting `CommitError::NotImplemented`
/// back) rather than the API appearing later with no forward-compatible
/// slot. Per the spec, only a **fork** ever commits — a root flow has no
/// parent to fold into, and its `Local`-scoped writes already persist for
/// its own lifetime; `World`-scoped writes already escape live, with no
/// "make it back" step needed.
///
/// Intended semantics, when implemented: walk `child`'s own top-layer
/// overrides (globals, visit counts, turn counts, turn index, RNG) and
/// apply each onto `parent`'s own top layer, as if `child`'s writes had
/// been made directly against `parent` — last-write-wins per unit, since a
/// fork is single-writer for its whole lifetime (no concurrent-write
/// conflict is possible, so no merge policy is needed). Commit never
/// touches `World` directly: a folded `Local`-scoped write still only
/// lands in `parent`'s overrides, reaching `World` only if `parent` is
/// itself later written through a `World`-scoped op or committed further up
/// the chain. `Sandbox`-mode writes are exactly what this would fold
/// back — commit is what would turn a sandboxed probe into a real,
/// persisted mutation, for a caller that chooses to call it instead of
/// dropping the child.
///
/// # Errors
///
/// Always returns [`CommitError::NotImplemented`] in this release.
pub fn commit(_child: FlowLocal, _parent: &mut FlowLocal) -> Result<(), CommitError> {
    Err(CommitError::NotImplemented)
}

/// Routing view implementing [`ContextAccess`] over `(&mut World, &mut
/// FlowLocal)`.
///
/// This is what the VM's drive path receives as its `impl ContextAccess`.
/// Every op computes an **effective scope** for its unit — see
/// [`ContextView::effective_scope`] — and then routes exactly as before:
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
/// **The effective scope, not the raw policy scope, drives all of the
/// above.** In [`Mode::Normal`] (every construction path before F3.2, and
/// every non-forked flow today) the effective scope of a unit *is* its
/// `ResolvedPolicy` scope — unchanged from F2.2/F3.1. In [`Mode::Sandbox`]
/// the effective scope of **every** unit is `Local`, no matter what the
/// policy says: a sandboxed flow's reads still chain-read-through to
/// `World`'s live value on a miss (so it observes current shared state),
/// but its writes — including to units the policy homes to `World` — land
/// only in its own `FlowLocal` overrides. `World` is therefore a read-only
/// base from a sandboxed flow's perspective: nothing it does can mutate the
/// shared world.
///
/// Because the all-`World` policy (the only policy the oracle corpus
/// exercises) takes the `World` branch on every op *and* no existing
/// construction path ever sets `Mode::Sandbox`, this is byte-identical to
/// the F1.3 all-`World` passthrough for every existing single-flow
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

    /// The scope a unit actually routes by: `policy_scope` in
    /// [`Mode::Normal`], or unconditionally [`Scope::Local`] in
    /// [`Mode::Sandbox`] — see the type docs above.
    #[inline]
    fn effective_scope(&self, policy_scope: Scope) -> Scope {
        match self.local.mode {
            Mode::Normal => policy_scope,
            Mode::Sandbox => Scope::Local,
        }
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

    /// Real move via [`core::mem::replace`] — `World`'s globals are a flat
    /// `Vec<Value>`, so taking is exactly as cheap as an ordinary indexed
    /// write, with no extra `Arc` clone (unlike the trait's default
    /// clone-then-null implementation).
    #[inline]
    fn take_global(&mut self, idx: u32) -> Value {
        core::mem::replace(&mut self.globals[idx as usize], Value::Null)
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
    fn set_visit_count(&mut self, id: DefinitionId, count: u32) {
        self.visit_counts.insert(id, count);
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
    fn set_turn_index(&mut self, index: u32) {
        self.turn_index = index;
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
        match self.effective_scope(self.world.policy.scope_of_global(idx)) {
            Scope::Local => self
                .local
                .chain_get_global(idx)
                .unwrap_or_else(|| self.world.global(idx)),
            Scope::World => self.world.global(idx),
        }
    }

    #[inline]
    fn set_global(&mut self, idx: u32, value: Value) {
        match self.effective_scope(self.world.policy.scope_of_global(idx)) {
            Scope::Local => {
                self.local.globals.insert(idx, value);
            }
            Scope::World => self.world.set_global(idx, value),
        }
    }

    /// `World`-scoped units (the all-`World` default policy every oracle
    /// program runs under) delegate straight to `World::take_global`'s real
    /// move. `Local`-scoped units use the trait's default clone-then-null:
    /// a real move only helps when *this* flow already owns the unique
    /// reference, which the read-through chain (own overrides → frozen
    /// base → `World`) can't generally provide — an immutable
    /// [`FrozenLocal`] ancestor can't be moved out of. Own-layer overrides
    /// (`self.local.globals`) *could* be moved out of directly, but the
    /// perf-critical path this closes (value-model-spec §5's loop-append
    /// cliff) is the common single-`World` case; a `Local`-scoped fast path
    /// is future work if profiling ever shows it matters (T1b-4/#576 scope
    /// note).
    #[inline]
    fn take_global(&mut self, idx: u32) -> Value {
        match self.effective_scope(self.world.policy.scope_of_global(idx)) {
            Scope::Local => {
                let v = self.global(idx).clone();
                self.local.globals.insert(idx, Value::Null);
                v
            }
            Scope::World => self.world.take_global(idx),
        }
    }

    #[inline]
    fn visit_count(&self, id: DefinitionId) -> u32 {
        match self.effective_scope(self.world.policy.scope_of_knot(id)) {
            Scope::Local => self
                .local
                .chain_get_visit_count(id)
                .unwrap_or_else(|| self.world.visit_count(id)),
            Scope::World => self.world.visit_count(id),
        }
    }

    #[inline]
    fn increment_visit(&mut self, id: DefinitionId) {
        match self.effective_scope(self.world.policy.scope_of_knot(id)) {
            Scope::Local => {
                let base = self.visit_count(id);
                self.local.visit_counts.insert(id, base + 1);
            }
            Scope::World => self.world.increment_visit(id),
        }
    }

    #[inline]
    fn set_visit_count(&mut self, id: DefinitionId, count: u32) {
        match self.effective_scope(self.world.policy.scope_of_knot(id)) {
            Scope::Local => {
                self.local.visit_counts.insert(id, count);
            }
            Scope::World => self.world.set_visit_count(id, count),
        }
    }

    #[inline]
    fn turn_count(&self, id: DefinitionId) -> Option<u32> {
        match self.effective_scope(self.world.policy.scope_of_knot(id)) {
            Scope::Local => self
                .local
                .chain_get_turn_count(id)
                .or_else(|| self.world.turn_count(id)),
            Scope::World => self.world.turn_count(id),
        }
    }

    #[inline]
    fn set_turn_count(&mut self, id: DefinitionId, turn: u32) {
        match self.effective_scope(self.world.policy.scope_of_knot(id)) {
            Scope::Local => {
                self.local.turn_counts.insert(id, turn);
            }
            Scope::World => self.world.set_turn_count(id, turn),
        }
    }

    #[inline]
    fn turn_index(&self) -> u32 {
        match self.effective_scope(self.world.policy.turn_index_scope()) {
            Scope::Local => self
                .local
                .chain_get_turn_index()
                .unwrap_or_else(|| self.world.turn_index()),
            Scope::World => self.world.turn_index(),
        }
    }

    #[inline]
    fn increment_turn_index(&mut self) {
        match self.effective_scope(self.world.policy.turn_index_scope()) {
            Scope::Local => {
                let base = self.turn_index();
                self.local.turn_index = Some(base + 1);
            }
            Scope::World => self.world.increment_turn_index(),
        }
    }

    #[inline]
    fn set_turn_index(&mut self, index: u32) {
        match self.effective_scope(self.world.policy.turn_index_scope()) {
            Scope::Local => {
                self.local.turn_index = Some(index);
            }
            Scope::World => self.world.set_turn_index(index),
        }
    }

    #[inline]
    fn rng_seed(&self) -> i32 {
        match self.effective_scope(self.world.policy.rng_scope()) {
            Scope::Local => self
                .local
                .chain_get_rng()
                .map_or_else(|| self.world.rng_seed(), |rng| rng.seed),
            Scope::World => self.world.rng_seed(),
        }
    }

    #[inline]
    fn set_rng_seed(&mut self, seed: i32) {
        match self.effective_scope(self.world.policy.rng_scope()) {
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
        match self.effective_scope(self.world.policy.rng_scope()) {
            Scope::Local => self
                .local
                .chain_get_rng()
                .map_or_else(|| self.world.previous_random(), |rng| rng.previous_random),
            Scope::World => self.world.previous_random(),
        }
    }

    #[inline]
    fn set_previous_random(&mut self, val: i32) {
        match self.effective_scope(self.world.policy.rng_scope()) {
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

// ── FrameStartView: borrow, don't copy ──────────────────────────────────

/// A **borrowing** view over a pinned frame-start [`World`]: reads are
/// served by reference from the shared `&World`, writes land in a private
/// per-view overlay that the shared world never sees.
///
/// This is `docs/effects-spec.md` §12.2's "borrow, don't copy" primitive
/// (issue #937). It exists for batch-mode stepping, where N flows must each
/// advance against *the same* frame-start state while their writes stay
/// private until a later, ordered Apply pass. The obvious way to get that
/// is to hand every flow its own `frame_start.clone()`; the cost of that
/// clone is `O(world size)` **per flow, per turn** — every global `Value`,
/// every visit/turn-count entry — which is nearly free for a scalar toy
/// world and emphatically not free for a real game's.
///
/// `FrameStartView` pays `O(1)` to construct and `O(cells this flow
/// actually wrote)` thereafter. It is **observationally identical** to
/// stepping against a private clone:
///
/// - a **read** returns the overlay's value if this view has written that
///   cell, else the frame-start value — i.e. `frame_start ⊕ own writes`,
///   exactly what a clone would hold;
/// - a **write** only ever mutates the overlay, so the borrowed `&World`
///   (and therefore every peer view over it) is untouched;
/// - an **increment** (`increment_visit`, `increment_turn_index`) is
///   copy-on-write from that same read-through value, so a flow's first
///   increment starts from the frame-start count rather than 0.
///
/// Because it borrows shared-immutably, many `FrameStartView`s over one
/// `&World` can run **concurrently** — the property `bevy-brink`'s parallel
/// batch driver needs, and the reason this type takes `&World` rather than
/// the `&mut World` [`ContextView`] requires. The semantics are those of
/// [`Mode::Sandbox`] (every unit treated as flow-private, the shared world
/// read-only), reachable here without a `&mut` borrow and without a
/// [`FlowLocal`] chain — this view has no frozen base and consults no
/// [`ResolvedPolicy`], because every cell is unconditionally overlaid.
///
/// The overlay is intentionally **not** readable back out: the authoritative
/// record of what a flow wrote is the
/// [`WriteObserver`](crate::WriteObserver) callback stream, which a caller
/// gets by wrapping this view in an
/// [`ObservedContext`](crate::ObservedContext). Keeping one changeset
/// record instead of two is what makes the buffered-write Apply pass
/// trivially consistent with what the flow actually observed.
pub struct FrameStartView<'a> {
    /// The pinned, shared frame-start state. Never mutated.
    frame_start: &'a World,
    /// Globals this view has written, keyed by slot index.
    globals: BTreeMap<u32, Value>,
    /// Visit counts this view has written or incremented.
    visit_counts: BTreeMap<DefinitionId, u32>,
    /// Turn counts this view has written.
    turn_counts: BTreeMap<DefinitionId, u32>,
    /// Turn index, once this view has written or incremented it.
    turn_index: Option<u32>,
    /// RNG stream, once this view has written either half of it.
    rng: Option<LocalRng>,
}

impl<'a> FrameStartView<'a> {
    /// Open a fresh view over `frame_start`. The overlay starts empty, so
    /// every read passes straight through to the borrowed world until this
    /// view writes the cell in question.
    #[must_use]
    pub fn new(frame_start: &'a World) -> Self {
        Self {
            frame_start,
            globals: BTreeMap::new(),
            visit_counts: BTreeMap::new(),
            turn_counts: BTreeMap::new(),
            turn_index: None,
            rng: None,
        }
    }

    /// The overlaid RNG stream, seeded from the frame-start values on first
    /// write — the copy-on-write half of `set_rng_seed`/`set_previous_random`
    /// (the two scalars [`WorldPolicy::rng`] scopes as one unit, so writing
    /// either must capture both).
    #[inline]
    fn rng_mut(&mut self) -> &mut LocalRng {
        self.rng.get_or_insert(LocalRng {
            seed: self.frame_start.rng_seed,
            previous_random: self.frame_start.previous_random,
        })
    }
}

impl ContextAccess for FrameStartView<'_> {
    #[inline]
    fn global(&self, idx: u32) -> &Value {
        self.globals
            .get(&idx)
            .unwrap_or_else(|| self.frame_start.global(idx))
    }

    #[inline]
    fn set_global(&mut self, idx: u32, value: Value) {
        self.globals.insert(idx, value);
    }

    /// A real [`core::mem::replace`] move once the slot is in the overlay —
    /// which is what keeps `docs/value-model-spec.md` §5's take → `make_mut`
    /// → write-back discipline `O(1)`-amortized here, exactly as it is
    /// against a private clone. The **first** take of a given slot must
    /// still clone out of the borrowed frame-start world (it is shared; this
    /// view may not move out of it), but that is one `Arc` bump for one
    /// cell — the same bump a whole-world clone would have paid for that
    /// cell up front, and every subsequent take of the slot is a move.
    #[inline]
    fn take_global(&mut self, idx: u32) -> Value {
        if let Some(slot) = self.globals.get_mut(&idx) {
            return core::mem::replace(slot, Value::Null);
        }
        let value = self.frame_start.global(idx).clone();
        self.globals.insert(idx, Value::Null);
        value
    }

    #[inline]
    fn visit_count(&self, id: DefinitionId) -> u32 {
        self.visit_counts
            .get(&id)
            .copied()
            .unwrap_or_else(|| self.frame_start.visit_count(id))
    }

    #[inline]
    fn increment_visit(&mut self, id: DefinitionId) {
        let base = self.visit_count(id);
        self.visit_counts.insert(id, base + 1);
    }

    #[inline]
    fn set_visit_count(&mut self, id: DefinitionId, count: u32) {
        self.visit_counts.insert(id, count);
    }

    #[inline]
    fn turn_count(&self, id: DefinitionId) -> Option<u32> {
        self.turn_counts
            .get(&id)
            .copied()
            .or_else(|| self.frame_start.turn_count(id))
    }

    #[inline]
    fn set_turn_count(&mut self, id: DefinitionId, turn: u32) {
        self.turn_counts.insert(id, turn);
    }

    #[inline]
    fn turn_index(&self) -> u32 {
        self.turn_index.unwrap_or(self.frame_start.turn_index)
    }

    #[inline]
    fn increment_turn_index(&mut self) {
        self.turn_index = Some(self.turn_index() + 1);
    }

    #[inline]
    fn set_turn_index(&mut self, index: u32) {
        self.turn_index = Some(index);
    }

    #[inline]
    fn rng_seed(&self) -> i32 {
        self.rng.map_or(self.frame_start.rng_seed, |rng| rng.seed)
    }

    #[inline]
    fn set_rng_seed(&mut self, seed: i32) {
        self.rng_mut().seed = seed;
    }

    #[inline]
    fn previous_random(&self) -> i32 {
        self.rng
            .map_or(self.frame_start.previous_random, |rng| rng.previous_random)
    }

    #[inline]
    fn set_previous_random(&mut self, val: i32) {
        self.rng_mut().previous_random = val;
    }

    #[inline]
    fn next_random<R: StoryRng>(&self, seed: i32) -> i32 {
        // Pure function of the explicit `seed` argument — not overlaid
        // state, so it delegates unchanged (see `ContextView`'s note).
        self.frame_start.next_random::<R>(seed)
    }

    fn random_sequence<R: StoryRng>(&self, seed: i32, count: usize) -> Vec<i32> {
        self.frame_start.random_sequence::<R>(seed, count)
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

    /// F3.2 fork isolation (`Mode::Normal` child): forking a parent
    /// `FlowLocal` gives the child a frozen view of the parent's overrides
    /// at fork time. A write the child makes to a `Local`-scoped unit lands
    /// only in the child's own top layer — the parent's `FlowLocal` and
    /// `World` are both unaffected.
    #[test]
    fn fork_isolation_normal_child_write_does_not_leak_to_parent_or_world() {
        let program = sample_program();
        let policy = mixed_policy(); // mood: Local, gold: World (default)
        let mut world = World::new(&program, &policy).expect("world builds");
        let mood_slot = program.global_index("mood").expect("mood declared");

        let mut parent = FlowLocal::new();
        {
            let mut view = ContextView::new(&mut world, &mut parent);
            view.set_global(mood_slot, Value::Int(42));
        }

        // Fork a Normal child from the parent.
        let mut child = parent.fork(Mode::Normal);

        // The child reads the parent's frozen state via `base` — it never
        // wrote `mood` itself.
        {
            let view = ContextView::new(&mut world, &mut child);
            assert_eq!(view.global(mood_slot), &Value::Int(42));
        }

        // The child writes its own override for `mood`.
        {
            let mut view = ContextView::new(&mut world, &mut child);
            view.set_global(mood_slot, Value::Int(100));
            assert_eq!(view.global(mood_slot), &Value::Int(100));
        }

        // The parent's own `FlowLocal` still reads its original write — the
        // child's write never reached it.
        {
            let view = ContextView::new(&mut world, &mut parent);
            assert_eq!(view.global(mood_slot), &Value::Int(42));
        }

        // `World` was never touched — `mood` is Local-scoped, so it was
        // never written there in the first place.
        assert_eq!(world.global(mood_slot), &Value::Int(0));
    }

    /// F3.2 frozen snapshot: a fork's `base` is a point-in-time snapshot.
    /// Mutating the parent *after* the fork must not be visible through the
    /// child, which still reads the parent's state as of the fork.
    #[test]
    fn fork_base_is_a_frozen_snapshot_later_parent_writes_invisible_to_child() {
        let program = sample_program();
        let policy = mixed_policy(); // mood: Local
        let mut world = World::new(&program, &policy).expect("world builds");
        let mood_slot = program.global_index("mood").expect("mood declared");

        let mut parent = FlowLocal::new();
        {
            let mut view = ContextView::new(&mut world, &mut parent);
            view.set_global(mood_slot, Value::Int(1));
        }

        let mut child = parent.fork(Mode::Normal);

        // Mutate the parent *after* the fork.
        {
            let mut view = ContextView::new(&mut world, &mut parent);
            view.set_global(mood_slot, Value::Int(2));
        }

        // The child's frozen base still reflects the pre-fork value.
        let view = ContextView::new(&mut world, &mut child);
        assert_eq!(view.global(mood_slot), &Value::Int(1));
    }

    /// F3.2 sandbox side-effect-proof: in `Mode::Sandbox`, a `World`-scoped
    /// unit is still readable through the live `World` value, but any write
    /// — even to a unit the policy homes to `World` — lands only in the
    /// sandboxed flow's own overrides. `World` itself is never mutated, and
    /// dropping the sandboxed `FlowLocal` leaves no trace.
    #[test]
    fn sandbox_mode_writes_never_reach_world_reads_see_live_world() {
        let program = sample_program();
        let policy = mixed_policy(); // gold: World (default), mood: Local
        let mut world = World::new(&program, &policy).expect("world builds");
        let gold_slot = program.global_index("gold").expect("gold declared");
        let shrine_id = program.find_path_target("shrine").expect("shrine exists");

        // Simulate pre-existing shared state the sandboxed flow should see.
        world.set_global(gold_slot, Value::Int(7));
        world.increment_visit(shrine_id);
        world.increment_visit(shrine_id);
        world.increment_visit(shrine_id);
        assert_eq!(world.visit_count(shrine_id), 3);

        // Fork a sandboxed child from a "live" flow's (empty) FlowLocal.
        let live = FlowLocal::new();
        {
            let mut sandboxed = live.fork(Mode::Sandbox);

            // Reads see World's current, live value even though `gold` and
            // `shrine` are World-scoped by policy.
            {
                let view = ContextView::new(&mut world, &mut sandboxed);
                assert_eq!(view.global(gold_slot), &Value::Int(7));
                assert_eq!(view.visit_count(shrine_id), 3);
            }

            // Writing the World-scoped `gold` in the sandbox diverts to the
            // sandbox's own overrides — it does not touch `World`.
            {
                let mut view = ContextView::new(&mut world, &mut sandboxed);
                view.set_global(gold_slot, Value::Int(555));
                assert_eq!(view.global(gold_slot), &Value::Int(555)); // visible locally
            }
            assert_eq!(world.global(gold_slot), &Value::Int(7)); // World unchanged

            // Incrementing a World-scoped visit count in the sandbox is
            // copy-on-write from the live World count, but the increment
            // itself stays local — World's count is untouched.
            {
                let mut view = ContextView::new(&mut world, &mut sandboxed);
                view.increment_visit(shrine_id);
                assert_eq!(view.visit_count(shrine_id), 4); // sandbox sees 4
            }
            assert_eq!(world.visit_count(shrine_id), 3); // World still 3

            // Dropping `sandboxed` here (end of scope) is discard — nothing
            // escaped to World, so there is nothing to unwind.
        }

        // World is still clean after the sandboxed child is gone.
        assert_eq!(world.global(gold_slot), &Value::Int(7));
        assert_eq!(world.visit_count(shrine_id), 3);
    }
}

#[cfg(test)]
mod save_load_tests {
    use super::*;
    use crate::link;
    use crate::rng::FastRng;
    use crate::story::{FallbackHandler, FlowInstance};
    use crate::{load_state, save_state};

    /// Compile a small ink story with the brink compiler and link it,
    /// keeping the line tables `FlowInstance::drive_to_terminal` needs.
    fn compile_for_flow(src: &str) -> (Program, Vec<Vec<brink_format::LineEntry>>) {
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
        link(&out.data).expect("link")
    }

    /// A scoped save/load roundtrip (F6.1b): `gold` (global) and `shrine`
    /// (knot) are policy-scoped `Local`; `silver` (global) stays `World`
    /// (the default). Driving the flow populates both layers; saving
    /// through the routing view captures effective values regardless of
    /// scope. Loading into a **fresh** `(World, FlowLocal)` pair through a
    /// fresh view must land each unit back in the layer its policy
    /// names — `Local` units in the new `FlowLocal`'s override maps (the new
    /// `World`'s own copy stays untouched), `World` units directly in the
    /// new `World` (the new `FlowLocal` contributes nothing for them).
    #[test]
    fn scoped_save_load_lands_each_unit_in_its_policy_layer() {
        let (program, tables) = compile_for_flow(
            "VAR gold = 0\n\
             VAR silver = 0\n\
             ~ silver = 7\n\
             -> shrine\n\
             === shrine ===\n\
             ~ gold = 5\n\
             At the shrine.\n\
             -> DONE\n\
             === reader ===\n\
             {READ_COUNT(-> shrine)}\n\
             -> DONE\n",
            // `reader` is never entered — it exists only so the compiler's
            // counting-flags pass sees a visit-count read of `shrine` and
            // sets `CountingFlags::VISITS` on it (a knot whose visit count
            // is never read anywhere in the program has counting disabled
            // entirely — an existing compiler optimization).
        );

        let mut overrides = BTreeMap::new();
        overrides.insert("gold".to_owned(), Scope::Local);
        overrides.insert("shrine".to_owned(), Scope::Local);
        let policy = WorldPolicy {
            default: Scope::World,
            overrides,
            turn_index: Scope::World,
            rng: Scope::World,
        };

        let gold_slot = program.global_index("gold").expect("gold declared");
        let silver_slot = program.global_index("silver").expect("silver declared");
        let shrine_id = program.find_path_target("shrine").expect("shrine exists");

        // Drive the flow against a World built from our policy — the
        // `FlowInstance::new_at_root`-returned World is discarded; only the
        // callstack/thread state it seeds matters here.
        let mut world = World::new(&program, &policy).expect("world builds");
        let mut local = FlowLocal::new();
        let save = {
            let (mut flow, _unused_default_world) = FlowInstance::new_at_root(&program);
            let mut view = ContextView::new(&mut world, &mut local);
            flow.drive_to_terminal::<FastRng>(&program, &tables, &mut view, &FallbackHandler, None)
                .expect("drive succeeds");
            save_state(&program, &view)
        };

        assert_eq!(save.globals.get("gold"), Some(&Value::Int(5)));
        assert_eq!(save.globals.get("silver"), Some(&Value::Int(7)));
        assert_eq!(
            save.visits
                .iter()
                .find(|e| e.id == shrine_id)
                .map(|e| e.count),
            Some(1),
            "shrine should have a captured visit entry"
        );

        // Load into a fresh (World, FlowLocal) pair, built from the same
        // policy but with none of the driven state.
        let mut world2 = World::new(&program, &policy).expect("world builds");
        let mut local2 = FlowLocal::new();
        let report = {
            let mut view2 = ContextView::new(&mut world2, &mut local2);
            load_state(&program, &mut view2, &save)
        };
        assert!(report.unknown_globals.is_empty(), "clean load: {report:?}");

        // `gold` is Local-scoped: the load must land it in `local2`'s
        // override map, leaving `world2`'s own copy at its untouched
        // default. The routing view's effective read still sees 5.
        assert_eq!(
            world2.global(gold_slot),
            &Value::Int(0),
            "gold is Local-scoped; World's own copy must stay untouched"
        );
        {
            let view2 = ContextView::new(&mut world2, &mut local2);
            assert_eq!(view2.global(gold_slot), &Value::Int(5));
        }

        // `silver` is World-scoped: the load must land it directly in
        // `world2`, readable without any FlowLocal involvement.
        assert_eq!(
            world2.global(silver_slot),
            &Value::Int(7),
            "silver is World-scoped; must land directly in World"
        );

        // `shrine`'s visit count is Local-scoped: same split as `gold`.
        assert_eq!(
            world2.visit_count(shrine_id),
            0,
            "shrine is Local-scoped; World's own visit count must stay untouched"
        );
        {
            let view2 = ContextView::new(&mut world2, &mut local2);
            assert_eq!(view2.visit_count(shrine_id), 1);
        }
    }
}

#[cfg(test)]
mod subtree_scope_tests {
    use super::*;
    use crate::link;
    use crate::rng::FastRng;
    use crate::story::{FallbackHandler, FlowInstance, Line};

    /// Compile a small ink story with the brink compiler and link it,
    /// keeping the line tables `FlowInstance::drive_to_terminal` needs.
    fn compile_for_flow(src: &str) -> (Program, Vec<Vec<brink_format::LineEntry>>) {
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
        link(&out.data).expect("link")
    }

    /// A knot `guard_talk` with its own top-level stopping sequence
    /// (`{ Halt! | Back again? }`) plus a stitch `guard_talk.inner` with its
    /// own stopping sequence, and an unrelated `other_knot` — the minimal
    /// shape needed to exercise interior-container containment, the knot→
    /// stitch cascade, and most-specific-wins precedence.
    fn story_with_stitch_and_sequences() -> (Program, Vec<Vec<brink_format::LineEntry>>) {
        compile_for_flow(
            "VAR gold = 0\n\
             -> guard_talk\n\
             === guard_talk ===\n\
             { stopping: Halt! | Back again? }\n\
             -> inner\n\
             = inner\n\
             { stopping: A | B | C }\n\
             -> DONE\n\
             === other_knot ===\n\
             Other.\n\
             -> DONE\n",
        )
    }

    /// Find the `DefinitionId` of the (single) interior container directly
    /// owned by `scope_owner` that carries `CountingFlags::VISITS` — i.e.
    /// the anonymous sequence container `handle_sequence` (`vm.rs`) keys its
    /// counter off. Panics if there isn't exactly one, since every test
    /// story here is built with exactly one stopping sequence per scope.
    fn find_owned_sequence_id(program: &Program, scope_owner: DefinitionId) -> DefinitionId {
        let mut found = None;
        for (idx, container) in program.containers.iter().enumerate() {
            #[expect(clippy::cast_possible_truncation, reason = "test fixture")]
            let idx = idx as u32;
            let owner = program.scope_ids[program.scope_table_idx(idx) as usize];
            if owner == scope_owner
                && container
                    .counting_flags
                    .contains(brink_format::CountingFlags::VISITS)
            {
                assert!(
                    found.is_none(),
                    "expected exactly one VISITS-counted interior container owned by {scope_owner:?}"
                );
                found = Some(container.id);
            }
        }
        found.expect("expected a VISITS-counted interior container")
    }

    /// A knot marked `Local` must cover not just its own `DefinitionId` but
    /// the interior sequence container nested directly under it — this is
    /// the exact bug the F6 AMENDMENT (ruling 3) describes:
    /// `handle_sequence` keys a stopping/cycle counter off the sequence's
    /// *own* container id, not the enclosing knot's, so without subtree
    /// expansion a `Local`-marked knot would silently leave that counter
    /// `World`-scoped.
    #[test]
    fn marked_local_knot_covers_its_interior_sequence_container() {
        let (program, _tables) = story_with_stitch_and_sequences();
        let guard_talk_id = program
            .find_path_target("guard_talk")
            .expect("guard_talk exists");
        let other_knot_id = program
            .find_path_target("other_knot")
            .expect("other_knot exists");
        let sequence_id = find_owned_sequence_id(&program, guard_talk_id);

        let mut overrides = BTreeMap::new();
        overrides.insert("guard_talk".to_owned(), Scope::Local);
        let policy = WorldPolicy {
            default: Scope::World,
            overrides,
            turn_index: Scope::World,
            rng: Scope::World,
        };
        let resolved = ResolvedPolicy::resolve(&program, &policy).expect("resolves");

        assert_eq!(resolved.scope_of_knot(guard_talk_id), Scope::Local);
        assert_eq!(
            resolved.scope_of_knot(sequence_id),
            Scope::Local,
            "the interior sequence container must inherit guard_talk's Local scope"
        );
        // An unrelated knot must stay at the World default — the expansion
        // must not leak scope onto unrelated containers.
        assert_eq!(resolved.scope_of_knot(other_knot_id), Scope::World);
    }

    /// Stitch-level override + most-specific-wins precedence: knot
    /// `guard_talk` is `Local`, but its stitch `guard_talk.inner` is
    /// explicitly `World`. Every container under `inner` (the stitch
    /// itself, and its own interior sequence) must resolve `World`; the
    /// rest of `guard_talk`'s subtree (the knot's own id and its own
    /// interior sequence) must resolve `Local`.
    #[test]
    fn stitch_override_wins_over_enclosing_knot_for_its_own_subtree() {
        let (program, _tables) = story_with_stitch_and_sequences();
        let guard_talk_id = program
            .find_path_target("guard_talk")
            .expect("guard_talk exists");
        let inner_id = program
            .find_path_target("guard_talk.inner")
            .expect("guard_talk.inner exists");
        let guard_talk_sequence_id = find_owned_sequence_id(&program, guard_talk_id);
        let inner_sequence_id = find_owned_sequence_id(&program, inner_id);

        let mut overrides = BTreeMap::new();
        overrides.insert("guard_talk".to_owned(), Scope::Local);
        overrides.insert("guard_talk.inner".to_owned(), Scope::World);
        let policy = WorldPolicy {
            default: Scope::World,
            overrides,
            turn_index: Scope::World,
            rng: Scope::World,
        };
        let resolved = ResolvedPolicy::resolve(&program, &policy).expect("resolves");

        assert_eq!(resolved.scope_of_knot(guard_talk_id), Scope::Local);
        assert_eq!(resolved.scope_of_knot(guard_talk_sequence_id), Scope::Local);
        assert_eq!(
            resolved.scope_of_knot(inner_id),
            Scope::World,
            "the stitch's own explicit override must win over its enclosing knot's"
        );
        assert_eq!(
            resolved.scope_of_knot(inner_sequence_id),
            Scope::World,
            "the stitch's interior sequence must follow the stitch's own override, \
             not the enclosing knot's"
        );

        // And the reverse precedence: knot World (default), stitch Local —
        // confirms precedence isn't just "whichever happens to be Local".
        let mut overrides2 = BTreeMap::new();
        overrides2.insert("guard_talk".to_owned(), Scope::World);
        overrides2.insert("guard_talk.inner".to_owned(), Scope::Local);
        let policy2 = WorldPolicy {
            default: Scope::World,
            overrides: overrides2,
            turn_index: Scope::World,
            rng: Scope::World,
        };
        let resolved2 = ResolvedPolicy::resolve(&program, &policy2).expect("resolves");
        assert_eq!(resolved2.scope_of_knot(guard_talk_id), Scope::World);
        assert_eq!(
            resolved2.scope_of_knot(guard_talk_sequence_id),
            Scope::World
        );
        assert_eq!(resolved2.scope_of_knot(inner_id), Scope::Local);
        assert_eq!(resolved2.scope_of_knot(inner_sequence_id), Scope::Local);
    }

    /// The all-`World` default policy must still resolve via
    /// `ResolvedPolicy::all_world`'s fast path — no per-slot/per-knot tables
    /// populated — even against a program with stitches and sequences that
    /// would otherwise drive subtree expansion. This is the oracle-anchored
    /// path every existing single-flow construction path takes; it must
    /// stay byte-identical.
    #[test]
    fn all_world_default_still_takes_fast_path() {
        let (program, _tables) = story_with_stitch_and_sequences();
        let resolved =
            ResolvedPolicy::resolve(&program, &WorldPolicy::default()).expect("resolves");

        // Fast path: no per-slot/per-knot table populated, matching
        // `all_world()` exactly.
        assert!(resolved.global_scopes.is_empty());
        assert!(resolved.knot_scopes.is_empty());

        let guard_talk_id = program
            .find_path_target("guard_talk")
            .expect("guard_talk exists");
        assert_eq!(resolved.scope_of_knot(guard_talk_id), Scope::World);
    }

    /// An override name that resolves to neither a global nor a knot/
    /// stitch path is still a `PolicyError::UnknownName` — subtree
    /// expansion must not swallow or change this error path.
    #[test]
    fn unknown_override_name_still_errors() {
        let (program, _tables) = story_with_stitch_and_sequences();
        let mut overrides = BTreeMap::new();
        overrides.insert("guard_talk.nonexistent_stitch".to_owned(), Scope::Local);
        let policy = WorldPolicy {
            default: Scope::World,
            overrides,
            turn_index: Scope::World,
            rng: Scope::World,
        };
        let err = ResolvedPolicy::resolve(&program, &policy).expect_err("must fail");
        assert_eq!(
            err,
            PolicyError::UnknownName("guard_talk.nonexistent_stitch".to_owned())
        );
    }

    /// End-to-end (F6.1c's motivating "per-entity memory" case): two
    /// `FlowInstance`s, each with its own `FlowLocal`, drive the *same*
    /// `guard_talk` knot (a stopping sequence, `{ Halt! | Back again? }`)
    /// over one shared `World` whose policy marks `guard_talk` `Local`.
    /// Without subtree expansion, the sequence's own interior container id
    /// isn't in `knot_scopes`, falls through to the `World` default, and
    /// the two flows' visits collide on one shared counter — the second
    /// flow would see "Back again?" on its very first encounter. With the
    /// fix, each flow's first encounter is independently the first-visit
    /// text.
    #[test]
    fn two_flows_over_shared_world_each_see_first_visit_text() {
        let (program, tables) = compile_for_flow(
            "VAR gold = 0\n\
             -> guard_talk\n\
             === guard_talk ===\n\
             { stopping: Halt! | Back again? }\n\
             -> DONE\n",
        );

        let mut overrides = BTreeMap::new();
        overrides.insert("guard_talk".to_owned(), Scope::Local);
        let policy = WorldPolicy {
            default: Scope::World,
            overrides,
            turn_index: Scope::World,
            rng: Scope::World,
        };
        let mut world = World::new(&program, &policy).expect("world builds");

        let drive = |flow: &mut FlowInstance, view: &mut ContextView<'_>| -> String {
            let lines = flow
                .drive_to_terminal::<FastRng>(&program, &tables, view, &FallbackHandler, None)
                .expect("drive succeeds");
            assert!(
                matches!(lines.last(), Some(Line::Done { .. })),
                "expected Done, got {lines:?}"
            );
            lines.iter().map(Line::text).collect::<String>()
        };

        // Flow A: first (and only, for this assertion) encounter.
        let (mut flow_a, _discarded_world_a) = FlowInstance::new_at_root(&program);
        let mut local_a = FlowLocal::new();
        let first_visit_a = {
            let mut view_a = ContextView::new(&mut world, &mut local_a);
            drive(&mut flow_a, &mut view_a)
        };

        // Flow B: independent FlowLocal, same shared World. Its first
        // encounter must read exactly like Flow A's — not "already
        // visited" — proving the interior sequence container's visit count
        // is Local per-flow, not accidentally shared through World.
        let (mut flow_b, _discarded_world_b) = FlowInstance::new_at_root(&program);
        let mut local_b = FlowLocal::new();
        let first_visit_b = {
            let mut view_b = ContextView::new(&mut world, &mut local_b);
            drive(&mut flow_b, &mut view_b)
        };

        assert_eq!(
            first_visit_a, first_visit_b,
            "both flows' first encounter with guard_talk must produce identical \
             (first-visit) text — each flow's visit count is independently local"
        );

        // Flow A, re-entered a second time (still its own FlowLocal): now
        // it must progress to the *next* branch of the stopping sequence,
        // proving Local scoping still lets a single flow's own state
        // advance normally.
        let second_visit_a = {
            let mut view_a = ContextView::new(&mut world, &mut local_a);
            flow_a
                .choose_path_string(&program, &mut view_a, "guard_talk")
                .expect("re-enter guard_talk");
            drive(&mut flow_a, &mut view_a)
        };
        assert_ne!(
            first_visit_a, second_visit_a,
            "flow A's second encounter must progress past the first-visit branch"
        );

        // Flow B's own local state must still be untouched by flow A's
        // second visit — driving B a second time reproduces A's *first*
        // progression, not A's second.
        let second_visit_b = {
            let mut view_b = ContextView::new(&mut world, &mut local_b);
            flow_b
                .choose_path_string(&program, &mut view_b, "guard_talk")
                .expect("re-enter guard_talk");
            drive(&mut flow_b, &mut view_b)
        };
        assert_eq!(
            second_visit_a, second_visit_b,
            "flow B's second encounter must match flow A's second encounter — \
             both progressed independently from the same (shared, untouched) \
             World default"
        );

        // World's own bookkeeping for the Local-scoped knot must never have
        // been touched by either flow.
        let sequence_id = find_owned_sequence_id(&program, {
            program
                .find_path_target("guard_talk")
                .expect("guard_talk exists")
        });
        assert_eq!(
            world.visit_count(sequence_id),
            0,
            "World's own copy of the Local-scoped sequence's visit count must stay untouched"
        );
    }
}

#[cfg(test)]
mod compiled_defaults_tests {
    //! Compiled `#@local` scope defaults seeding policy resolution
    //! (`docs/directive-annotations-spec.md` §4.6): the base layer of
    //! `base ⊕ host-overrides`, with zero host API involvement.

    use std::collections::BTreeMap;

    use super::*;
    use crate::link;

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

    /// `mood` and `shrine` (with a stitch and an interior sequence) are
    /// marked `#@local` in source; `gold` and `cellar` stay unmarked.
    fn annotated_program() -> Program {
        compile(
            "VAR gold = 0\n\
             #@local\n\
             VAR mood = 0\n\
             -> shrine\n\
             === shrine ===\n\
             #@local\n\
             At the shrine {&once|again}.\n\
             = inner\n\
             Deeper in.\n\
             -> END\n\
             === cellar ===\n\
             In the cellar.\n\
             -> END\n",
        )
    }

    #[test]
    fn unannotated_program_keeps_the_fast_path() {
        let program = compile("VAR gold = 0\nhello\n");
        assert!(!program.has_local_defaults());
        let resolved =
            ResolvedPolicy::resolve(&program, &WorldPolicy::default()).expect("resolves");
        // The all-World fast path allocates nothing.
        assert!(resolved.global_scopes.is_empty());
        assert!(resolved.knot_scopes.is_empty());
    }

    #[test]
    fn compiled_local_var_seeds_the_base() {
        let program = annotated_program();
        assert!(program.has_local_defaults());
        let resolved =
            ResolvedPolicy::resolve(&program, &WorldPolicy::default()).expect("resolves");

        let mood = program.global_index("mood").expect("mood declared");
        let gold = program.global_index("gold").expect("gold declared");
        assert_eq!(resolved.scope_of_global(mood), Scope::Local);
        assert_eq!(resolved.scope_of_global(gold), Scope::World);
    }

    #[test]
    fn compiled_local_knot_covers_its_subtree() {
        let program = annotated_program();
        let resolved =
            ResolvedPolicy::resolve(&program, &WorldPolicy::default()).expect("resolves");

        let shrine = program.find_path_target("shrine").expect("shrine exists");
        let inner = program
            .find_path_target("shrine.inner")
            .expect("stitch exists");
        let cellar = program.find_path_target("cellar").expect("cellar exists");

        assert_eq!(resolved.scope_of_knot(shrine), Scope::Local);
        assert_eq!(
            resolved.scope_of_knot(inner),
            Scope::Local,
            "a #@local knot covers its stitches"
        );
        assert_eq!(resolved.scope_of_knot(cellar), Scope::World);

        // Interior containers (the inline sequence) are covered too.
        let interior = interior_containers_by_scope(&program);
        let shrine_interior = interior.get(&shrine).cloned().unwrap_or_default();
        assert!(
            !shrine_interior.is_empty(),
            "the {{&…}} sequence creates interior containers under shrine"
        );
        for id in shrine_interior {
            assert_eq!(
                resolved.scope_of_knot(id),
                Scope::Local,
                "interior container {id:?} inherits the knot's compiled scope"
            );
        }
    }

    #[test]
    fn host_override_beats_the_compiled_bit() {
        let program = annotated_program();
        let mut overrides = BTreeMap::new();
        overrides.insert("mood".to_owned(), Scope::World);
        overrides.insert("shrine".to_owned(), Scope::World);
        let policy = WorldPolicy {
            default: Scope::World,
            overrides,
            turn_index: Scope::World,
            rng: Scope::World,
        };
        let resolved = ResolvedPolicy::resolve(&program, &policy).expect("resolves");

        let mood = program.global_index("mood").expect("mood declared");
        let shrine = program.find_path_target("shrine").expect("shrine exists");
        assert_eq!(
            resolved.scope_of_global(mood),
            Scope::World,
            "host override wins over the compiled #@local bit"
        );
        assert_eq!(resolved.scope_of_knot(shrine), Scope::World);
    }

    /// End to end with zero host policy: a `World` built with the default
    /// (empty) `WorldPolicy` picks up the compiled bits, and two flows
    /// sharing it get isolated `mood` but shared `gold`.
    #[test]
    fn compiled_base_isolates_flows_without_host_policy() {
        let program = annotated_program();
        let mut world = World::new(&program, &WorldPolicy::default()).expect("world builds");

        let mood = program.global_index("mood").expect("mood declared");
        let gold = program.global_index("gold").expect("gold declared");

        let mut local_a = FlowLocal::new();
        let mut local_b = FlowLocal::new();

        // Flow A writes both.
        {
            let mut view_a = ContextView::new(&mut world, &mut local_a);
            view_a.set_global(mood, Value::Int(42));
            view_a.set_global(gold, Value::Int(7));
        }
        // Flow B sees the shared `gold` but not A's private `mood`.
        {
            let view_b = ContextView::new(&mut world, &mut local_b);
            assert_eq!(view_b.global(mood), &Value::Int(0));
            assert_eq!(view_b.global(gold), &Value::Int(7));
        }
        assert_eq!(world.global(mood), &Value::Int(0));

        // Visit counts: `shrine` is flow-private by compilation.
        let shrine = program.find_path_target("shrine").expect("shrine exists");
        {
            let mut view_a = ContextView::new(&mut world, &mut local_a);
            view_a.increment_visit(shrine);
            assert_eq!(view_a.visit_count(shrine), 1);
        }
        {
            let view_b = ContextView::new(&mut world, &mut local_b);
            assert_eq!(view_b.visit_count(shrine), 0);
        }
        assert_eq!(world.visit_count(shrine), 0);
    }

    /// End to end through the VM: a `#@local` knot whose visit count is
    /// never *read* anywhere in the ink must still track visits — and
    /// track them per flow (#496). Without the compiler forcing
    /// `CountingFlags::VISITS` on marked containers, the read-site
    /// optimization compiles counting out and the VM never records the
    /// visit at all, in any layer.
    #[test]
    fn local_knot_with_no_reads_still_tracks_visits_per_flow() {
        use crate::rng::FastRng;
        use crate::story::{FallbackHandler, FlowInstance};

        let out = brink_compiler::compile("t.ink", |p| {
            if p == "t.ink" {
                Ok("-> shrine\n\
                    === shrine ===\n\
                    #@local\n\
                    At the shrine.\n\
                    -> END\n"
                    .to_string())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no such include",
                ))
            }
        })
        .expect("compile");
        let (program, tables) = link(&out.data).expect("link");
        let shrine = program.find_path_target("shrine").expect("shrine exists");

        let mut world = World::new(&program, &WorldPolicy::default()).expect("world builds");
        let mut local_a = FlowLocal::new();
        let mut local_b = FlowLocal::new();

        // Drive flow A through the knot.
        {
            let (mut flow, _unused_default_world) = FlowInstance::new_at_root(&program);
            let mut view_a = ContextView::new(&mut world, &mut local_a);
            flow.drive_to_terminal::<FastRng>(
                &program,
                &tables,
                &mut view_a,
                &FallbackHandler,
                None,
            )
            .expect("drive succeeds");
            assert_eq!(
                view_a.visit_count(shrine),
                1,
                "the VM must count the visit even though nothing reads it"
            );
        }
        // The count is flow-private: flow B and the shared World see 0.
        {
            let view_b = ContextView::new(&mut world, &mut local_b);
            assert_eq!(view_b.visit_count(shrine), 0);
        }
        assert_eq!(world.visit_count(shrine), 0);
    }
}

/// `take_global` (issue #576, `docs/value-model-spec.md` §5) mechanics:
/// proves the move is real (not a disguised clone) using the same
/// `Arc::strong_count`/pointer-identity technique
/// `brink-format::value::tests` uses for `array_make_mut`'s COW proofs —
/// the load-bearing property behind this PR's O(1)-amortized loop-append
/// claim.
#[cfg(test)]
mod take_global_tests {
    use super::*;

    fn world_with_one_global(value: Value) -> World {
        World::from_globals(vec![value], ResolvedPolicy::all_world())
    }

    /// `World::take_global` is a real move: the returned value is the exact
    /// same `Arc` allocation an external clone already pointed at (not a
    /// fresh copy), the refcount doesn't go up because of the take, and the
    /// slot is left `Value::Null`.
    #[test]
    fn world_take_global_moves_without_cloning() {
        let array = Value::array(vec![Value::Int(1), Value::Int(2)]);
        let external = Arc::clone(array.as_array().expect("array"));
        assert_eq!(Arc::strong_count(&external), 2, "world's slot + external");

        let mut world = world_with_one_global(array);
        let taken = world.take_global(0);

        assert_eq!(
            Arc::as_ptr(taken.as_array().expect("array")),
            Arc::as_ptr(&external),
            "take_global must return the SAME allocation, not a copy"
        );
        assert_eq!(
            Arc::strong_count(&external),
            2,
            "the take itself must not bump the refcount: external + taken, \
             the world's own slot reference is GONE (moved out, not cloned)"
        );
        assert_eq!(
            world.global(0),
            &Value::Null,
            "the slot must be left Value::Null after a take"
        );
    }

    /// Contrast with the ordinary `global()` read: cloning DOES bump the
    /// refcount — this is the exact COW cliff #576 closes (a `GetGlobal`
    /// clone leaves the slot AND the read both holding a reference, so a
    /// subsequent `array_make_mut` always sees itself as shared).
    #[test]
    fn ordinary_global_read_clones_and_bumps_refcount() {
        let array = Value::array(vec![Value::Int(1)]);
        let external = Arc::clone(array.as_array().expect("array"));
        assert_eq!(Arc::strong_count(&external), 2);

        let world = world_with_one_global(array);
        let read = world.global(0).clone();

        assert_eq!(
            Arc::strong_count(&external),
            3,
            "world's slot + external + this clone — the ordinary read path \
             genuinely bumps the refcount, unlike take_global"
        );
        drop(read);
    }

    /// `ContextView` routes `take_global` straight to `World::take_global`
    /// (the real move) for `World`-scoped units — the common, oracle-anchored
    /// all-`World` policy every program runs under by default.
    #[test]
    fn context_view_world_scoped_take_is_a_real_move() {
        let array = Value::array(vec![Value::Int(7)]);
        let external = Arc::clone(array.as_array().expect("array"));

        let mut world = world_with_one_global(array);
        let mut local = FlowLocal::new();
        let mut view = ContextView::new(&mut world, &mut local);

        let taken = view.take_global(0);
        assert_eq!(
            Arc::as_ptr(taken.as_array().expect("array")),
            Arc::as_ptr(&external)
        );
        assert_eq!(Arc::strong_count(&external), 2, "no extra clone");
        assert_eq!(view.global(0), &Value::Null);
    }

    /// `ContextView`'s `Local`-scoped branch (the trait default: clone then
    /// null) — correctness over a read-through miss: taking a `Local`-scoped
    /// global that's never been locally overridden reads `World`'s current
    /// value (via the read-through chain), leaves a `Value::Null` override
    /// in the flow's own layer, and never touches `World` itself.
    #[test]
    fn context_view_local_scoped_take_reads_through_and_nulls_local_override() {
        let array = Value::array(vec![Value::Int(9)]);
        let mut world = world_with_one_global(array.clone());
        // Force every global to Local scope.
        *world.policy = ResolvedPolicy {
            default: Scope::Local,
            global_scopes: vec![Scope::Local],
            knot_scopes: HashMap::new(),
            turn_index: Scope::World,
            rng: Scope::World,
        };
        let mut local = FlowLocal::new();
        let mut view = ContextView::new(&mut world, &mut local);

        let taken = view.take_global(0);
        assert_eq!(taken, array, "read-through gives the World's current value");
        assert_eq!(
            view.global(0),
            &Value::Null,
            "the flow's own override layer must now read Null"
        );
        assert_eq!(
            world.global(0),
            &array,
            "World's own copy is untouched — Local writes never land in World"
        );
    }
}

/// [`FrameStartView`] (issue #937, `docs/effects-spec.md` §12.2 "borrow,
/// don't copy"): the borrowing replacement for batch mode's per-flow
/// `frame_start.clone()`.
///
/// The load-bearing property is **observational equivalence with the clone
/// it replaces** — `frame_start ⊕ own writes`, cell for cell — so the
/// centerpiece here is `equivalent_to_stepping_against_a_private_clone`,
/// which replays one op script against both a real clone and a view and
/// compares every readable cell after every op. The rest pin the individual
/// mechanics that equivalence rests on.
#[cfg(test)]
mod frame_start_view_tests {
    use brink_format::DefinitionTag;

    use super::*;
    use crate::rng::FastRng;

    fn knot(n: u64) -> DefinitionId {
        DefinitionId::new(DefinitionTag::Address, n)
    }

    /// A frame-start world with some pre-existing state in every unit the
    /// view overlays, so a passthrough read is distinguishable from a
    /// default-initialized one.
    fn frame_start() -> World {
        let mut world = World::from_globals(
            vec![Value::Int(10), Value::Int(20), Value::Int(30)],
            ResolvedPolicy::all_world(),
        );
        world.visit_counts.insert(knot(1), 5);
        world.turn_counts.insert(knot(1), 7);
        world.turn_index = 42;
        world.rng_seed = 99;
        world.previous_random = 77;
        world
    }

    /// Every readable cell of a `ContextAccess`, as one comparable value:
    /// `(globals, visit counts, turn counts, turn index, rng seed, previous
    /// random)`. This is the observation vector the equivalence test diffs —
    /// if two contexts compare equal on it, nothing the VM can ask either one
    /// tells them apart.
    type Observation = (Vec<Value>, Vec<u32>, Vec<Option<u32>>, u32, i32, i32);

    /// Read every cell of `ctx` into one [`Observation`].
    fn snapshot(ctx: &impl ContextAccess) -> Observation {
        (
            (0..3).map(|i| ctx.global(i).clone()).collect(),
            (0..3).map(|i| ctx.visit_count(knot(i))).collect(),
            (0..3).map(|i| ctx.turn_count(knot(i))).collect(),
            ctx.turn_index(),
            ctx.rng_seed(),
            ctx.previous_random(),
        )
    }

    /// One mutation, applied identically to both sides of the equivalence
    /// comparison.
    #[derive(Clone, Copy)]
    enum Op {
        SetGlobal(u32, i32),
        TakeGlobal(u32),
        IncrementVisit(u64),
        SetVisitCount(u64, u32),
        SetTurnCount(u64, u32),
        IncrementTurnIndex,
        SetTurnIndex(u32),
        SetRngSeed(i32),
        SetPreviousRandom(i32),
    }

    /// Apply `op`, returning anything it hands back (only `TakeGlobal` does)
    /// so the two sides' return values can be compared too, not just the
    /// resulting state.
    fn apply(ctx: &mut impl ContextAccess, op: Op) -> Option<Value> {
        match op {
            Op::SetGlobal(idx, v) => {
                ctx.set_global(idx, Value::Int(v));
                None
            }
            Op::TakeGlobal(idx) => Some(ctx.take_global(idx)),
            Op::IncrementVisit(id) => {
                ctx.increment_visit(knot(id));
                None
            }
            Op::SetVisitCount(id, c) => {
                ctx.set_visit_count(knot(id), c);
                None
            }
            Op::SetTurnCount(id, t) => {
                ctx.set_turn_count(knot(id), t);
                None
            }
            Op::IncrementTurnIndex => {
                ctx.increment_turn_index();
                None
            }
            Op::SetTurnIndex(i) => {
                ctx.set_turn_index(i);
                None
            }
            Op::SetRngSeed(s) => {
                ctx.set_rng_seed(s);
                None
            }
            Op::SetPreviousRandom(v) => {
                ctx.set_previous_random(v);
                None
            }
        }
    }

    /// **The property this type exists to preserve.** Replay one op script
    /// against (a) a private `World` clone — the mechanism `FrameStartView`
    /// replaces — and (b) a view borrowing the same frame-start world, and
    /// assert the two are indistinguishable through `ContextAccess` after
    /// every single op, including each op's own return value. Covers all
    /// nine mutating entry points, exercising each cell both before and
    /// after it enters the overlay (the two branches every read has).
    #[test]
    fn equivalent_to_stepping_against_a_private_clone() {
        let script = [
            // Reads before any write: pure passthrough on the view side.
            Op::IncrementVisit(1), // CoW increment off a non-zero base
            Op::IncrementVisit(1), // ...then off the overlay's own value
            Op::IncrementVisit(2), // ...and off an absent (zero) base
            Op::SetVisitCount(0, 3),
            Op::SetTurnCount(1, 11), // overwrite a present turn count
            Op::SetTurnCount(2, 13), // set an absent one
            Op::IncrementTurnIndex,
            Op::IncrementTurnIndex,
            Op::SetTurnIndex(100),
            Op::SetRngSeed(-5), // first RNG write must capture both halves
            Op::SetPreviousRandom(6),
            Op::SetGlobal(0, 111),
            Op::TakeGlobal(0), // take an already-overlaid slot (a real move)
            Op::TakeGlobal(1), // take a slot still in the frame-start world
            Op::TakeGlobal(1), // ...and again, now that it is overlaid
            Op::SetGlobal(1, 222),
            Op::SetGlobal(2, 333),
        ];

        let world = frame_start();
        let mut cloned = world.clone();
        let mut view = FrameStartView::new(&world);

        assert_eq!(
            snapshot(&cloned),
            snapshot(&view),
            "a fresh view must read identically to a fresh clone"
        );

        for (step, op) in script.into_iter().enumerate() {
            let from_clone = apply(&mut cloned, op);
            let from_view = apply(&mut view, op);
            assert_eq!(from_clone, from_view, "op {step} returned differently");
            assert_eq!(
                snapshot(&cloned),
                snapshot(&view),
                "diverged after op {step}"
            );
        }

        // The whole point: none of that reached the borrowed world.
        assert_eq!(snapshot(&world), snapshot(&frame_start()));
    }

    /// The concurrency property the parallel batch driver depends on: peer
    /// views over one shared `&World` are mutually invisible, so the order
    /// they are stepped in cannot affect any of their outcomes.
    #[test]
    fn peer_views_over_one_world_are_independent() {
        let world = frame_start();
        let mut a = FrameStartView::new(&world);
        let mut b = FrameStartView::new(&world);

        a.set_global(0, Value::Int(1));
        a.increment_visit(knot(1));
        a.set_turn_index(1);
        a.set_rng_seed(1);

        assert_eq!(b.global(0), &Value::Int(10), "peer write must be invisible");
        assert_eq!(b.visit_count(knot(1)), 5);
        assert_eq!(b.turn_index(), 42);
        assert_eq!(b.rng_seed(), 99);

        b.set_global(0, Value::Int(2));
        assert_eq!(a.global(0), &Value::Int(1), "a still reads its own write");
    }

    /// `take_global` keeps value-model-spec §5's move discipline: the first
    /// take of a slot clones once out of the shared frame-start world (it may
    /// not move out of a borrow), and every take after that is a real
    /// `mem::replace` move of the overlay's own allocation.
    #[test]
    fn take_global_clones_once_then_moves() {
        let array = Value::array(vec![Value::Int(1), Value::Int(2)]);
        let external = Arc::clone(array.as_array().expect("array"));
        let world = World::from_globals(vec![array], ResolvedPolicy::all_world());
        assert_eq!(Arc::strong_count(&external), 2, "world's slot + external");

        let mut view = FrameStartView::new(&world);

        // First take: one clone off the shared world — the refcount rises,
        // and the world keeps its own copy.
        let first = view.take_global(0);
        assert_eq!(
            Arc::strong_count(&external),
            3,
            "world's slot + external + the clone this take made"
        );
        assert_eq!(view.global(0), &Value::Null);
        assert_eq!(
            Arc::as_ptr(world.global(0).as_array().expect("array")),
            Arc::as_ptr(&external),
            "the borrowed world still holds its own value"
        );

        // Put it back and take again: now the slot is the overlay's own, so
        // the take is a move — same allocation out, no refcount bump.
        view.set_global(0, first);
        let before = Arc::strong_count(&external);
        let second = view.take_global(0);
        assert_eq!(
            Arc::as_ptr(second.as_array().expect("array")),
            Arc::as_ptr(&external),
            "take must return the SAME allocation, not a copy"
        );
        assert_eq!(
            Arc::strong_count(&external),
            before,
            "the take itself must not bump the refcount"
        );
        assert_eq!(view.global(0), &Value::Null);
    }

    /// `next_random`/`random_sequence` are pure functions of the seed they
    /// are handed, so the view must answer exactly as the borrowed world
    /// does — including after the view has overlaid its own RNG stream.
    #[test]
    fn random_helpers_delegate_to_the_borrowed_world() {
        let world = frame_start();
        let mut view = FrameStartView::new(&world);

        assert_eq!(
            view.next_random::<FastRng>(3),
            world.next_random::<FastRng>(3)
        );
        assert_eq!(
            view.random_sequence::<FastRng>(3, 4),
            world.random_sequence::<FastRng>(3, 4)
        );

        view.set_rng_seed(1234);
        assert_eq!(
            view.next_random::<FastRng>(3),
            world.next_random::<FastRng>(3),
            "the overlaid stream must not change how an explicit seed draws"
        );
    }
}
