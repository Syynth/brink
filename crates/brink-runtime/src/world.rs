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
//!
//! F2.1 adds the **policy** types ([`Scope`], [`WorldPolicy`],
//! [`ResolvedPolicy`]) and their resolution against a [`Program`]'s symbol
//! table. This PR is pure addition: `ResolvedPolicy` is stored on `World`
//! but not yet consulted by [`ContextView`] — routing still goes straight
//! to `World` for everything. F2.2 wires the routing view to consult the
//! policy.

use std::collections::{BTreeMap, HashMap};

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
    /// **F2.1: stored but not yet consulted.** `ContextView` still routes
    /// every read/write straight to `World` regardless of this field's
    /// content — F2.2 wires that routing. Every construction path today
    /// (`World::from_globals`, `FlowInstance::new_at*`, `Story::new`)
    /// resolves [`WorldPolicy::default()`] (all-`World`), so this field is
    /// inert until F2.2 lands.
    #[expect(
        dead_code,
        reason = "F2.1 stores the resolved policy; F2.2 wires ContextView routing to consult it"
    )]
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
