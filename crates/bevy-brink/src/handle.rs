//! T1d-3: `bevy-brink` handle integration (`docs/t1d-spec.md` §4).
//!
//! Host resources (entities, audio instances, timers) reach an ink script
//! as opaque [`brink_format::Value::Handle`] tokens: `{kind, id}` scalars
//! with value semantics. This module owns the host half of that boundary:
//!
//! - [`HandleKind`] — the two-halved per-kind trait. `save_key` captures a
//!   live resource as a durable *reconstruction recipe*; `resolve` rebuilds
//!   a resource from one at load time. A kind that returns `None` from
//!   `save_key` is choosing ephemerality — never a spec-assigned category.
//! - [`HandleRegistry<K>`] — the per-kind token registry (a `Resource`):
//!   opaque `u64` id allocation, live-resource storage.
//! - [`HandleKinds`]/[`BrinkHandleAppExt::register_handle_kind`] — the
//!   type-erased index over every registered kind, needed anywhere a
//!   binding only has a runtime `Value::Handle` (kind known only as a
//!   string) rather than a static `K`: [`is_valid_system`], the dead-deref
//!   event, registry GC, and save/load.
//! - [`save_handles`]/[`load_handles`] — token→[`HandleSaveKey`]
//!   persistence beside the ink [`SaveState`], and the load-time
//!   [`RehydrationReport`] (rebound / dead-by-resolve / dead-ephemeral /
//!   dead-by-unregistered-kind) gated by [`RehydrationPolicy`].
//! - [`gc_on_turn_done`] — registry GC via a reachable-token scan, run at
//!   `-> DONE` quiescent sweeps (spec §4, value-model §6 license).
//! - [`HandleEntityRemap`] — an [`EntityMapper`] a `Resource`-typed
//!   [`HandleKind`]'s `resolve` can consult/populate when reconstructing
//!   scene-based entities whose cross-references used the old session's
//!   `Entity` ids.

use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;

use bevy_app::App;
use bevy_ecs::entity::{Entity, EntityMapper};
use bevy_ecs::event::EntityEvent;
use bevy_ecs::observer::On;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, In, Query, Res, ResMut};
use bevy_ecs::world::World;
use brink_format::Value;
use brink_runtime::{Program, SaveState};
use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::asset::{BrinkProgram, ProgramAsset};
use crate::bindings::BrinkQueryInput;
use crate::event::BrinkTurnDone;
use crate::globals::{BrinkContext, BrinkGlobals, save_flow_state};
use bevy_asset::Assets;

// ── HandleKind — the two-halved per-kind trait (spec §4) ────────────────

/// Per-kind rehydration contract: save-side keying (live resource → durable
/// [`SaveKey`](Self::SaveKey)) and load-side resolution (`SaveKey` → new
/// resource). Verbatim from `docs/t1d-spec.md` §4 (2026-07-14 mechanics
/// ruling).
///
/// `SaveKey` is a **reconstruction recipe, not just a foreign key** — the
/// implementor picks a point on the spectrum: identity lookup (an NPC GUID),
/// reconstruction (a timer saves its remaining duration; `resolve` spawns a
/// fresh one — timers *are* resumable, the canonical example), or
/// deliberate ephemerality (`save_key` returns `None`: "this resource is
/// meaningless across sessions"). Ephemerality is an implementor choice,
/// never a category the spec assigns.
pub trait HandleKind: Send + Sync + 'static {
    /// The manifest-declared kind name (`handle<KIND>` in ink source).
    const KIND: &'static str;
    /// The live host resource this kind's tokens dereference to.
    type Resource: Send + Sync + 'static;
    /// The durable reconstruction recipe persisted beside the ink
    /// [`SaveState`].
    type SaveKey: Serialize + DeserializeOwned + Clone + Send + Sync + 'static;

    /// Capture `res` as a durable [`SaveKey`](Self::SaveKey). `None` means
    /// this particular live resource is ephemeral — it will not round-trip
    /// through save/load at all (not an error; a deliberate choice).
    fn save_key(&self, world: &World, res: &Self::Resource) -> Option<Self::SaveKey>;

    /// Rebuild a resource from a persisted [`SaveKey`](Self::SaveKey) at
    /// load time. `None` means the recipe no longer resolves to anything
    /// live (e.g. the NPC GUID it names despawned) — a normal, expected
    /// outcome, reported as `dead_by_resolve`, never a fault.
    fn resolve(&self, world: &mut World, key: &Self::SaveKey) -> Option<Self::Resource>;
}

// ── HandleRegistry<K> — per-kind token storage ───────────────────────────

/// Per-kind token registry: opaque `u64` id allocation plus live-resource
/// storage. A `Resource`, inserted by
/// [`register_handle_kind`](BrinkHandleAppExt::register_handle_kind).
///
/// `BTreeMap` (not `HashMap`) for deterministic iteration — GC and snapshot
/// walk `live` in id order (CLAUDE.md determinism rule).
#[derive(Resource)]
pub struct HandleRegistry<K: HandleKind> {
    implementor: K,
    next_id: u64,
    live: BTreeMap<u64, K::Resource>,
}

impl<K: HandleKind> HandleRegistry<K> {
    #[must_use]
    pub fn new(implementor: K) -> Self {
        Self {
            implementor,
            next_id: 0,
            live: BTreeMap::new(),
        }
    }

    /// Mint a fresh opaque token id for `resource`, storing it live.
    pub fn mint(&mut self, resource: K::Resource) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.live.insert(id, resource);
        id
    }

    /// Mint a token and build the ink-facing [`Value::Handle`] for it,
    /// resolving this kind's name id against `program`'s name table.
    /// `None` if this compile never interned [`HandleKind::KIND`] (no
    /// `handle<K>`-typed signature/annotation anywhere in the source graph
    /// — see [`Program::name_id`](brink_runtime::Program::name_id)).
    pub fn mint_value(&mut self, program: &Program, resource: K::Resource) -> Option<Value> {
        let kind = program.name_id(K::KIND)?;
        let id = self.mint(resource);
        Some(Value::handle(kind, id))
    }

    #[must_use]
    pub fn get(&self, id: u64) -> Option<&K::Resource> {
        self.live.get(&id)
    }

    #[must_use]
    pub fn contains(&self, id: u64) -> bool {
        self.live.contains_key(&id)
    }

    pub fn remove(&mut self, id: u64) -> Option<K::Resource> {
        self.live.remove(&id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.live.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// Look up a token, firing [`BrinkDeadHandleDeref`] at `flow` when it's
    /// dead. Opt-in dead-deref telemetry (spec §4): a binding that just
    /// wants a silent `None` should use [`get`](Self::get) instead.
    pub fn get_or_dead<M: Send + Sync + 'static>(
        &self,
        id: u64,
        commands: &mut Commands,
        flow: Entity,
    ) -> Option<&K::Resource> {
        let found = self.live.get(&id);
        if found.is_none() {
            commands.trigger(BrinkDeadHandleDeref::<M>::new(flow, K::KIND, id));
        }
        found
    }
}

// ── Type-erased dispatch — needed wherever the kind is only a runtime string ─

/// One `(id, SaveKey)` entry, `SaveKey` erased to JSON so heterogeneous
/// kinds can share one persisted table ([`HandleSaveState`]).
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct HandleSaveEntry {
    pub id: u64,
    pub key: serde_json::Value,
}

/// Per-kind rehydration outcome, folded into a [`RehydrationReport`] by
/// [`load_handles`].
#[derive(Debug, Default)]
struct KindRehydrateOutcome {
    rebound: Vec<u64>,
    dead_by_resolve: Vec<u64>,
}

/// Type-erased per-kind operations, so [`HandleKinds`] can dispatch on a
/// runtime kind name without knowing `K` statically — exactly the situation
/// a binding is in when it only has a `Value::Handle` (spec §4: `is_valid`,
/// dead-deref, GC, save/load all work from the wire token, not a static
/// type).
trait ErasedHandleRegistry: Send + Sync + 'static {
    fn kind_name(&self) -> &'static str;
    fn is_valid(&self, world: &World, id: u64) -> bool;
    /// Drop every live entry whose id isn't in `keep`. Returns
    /// `(dropped, retained)`.
    fn gc_retain(&self, world: &mut World, keep: &BTreeSet<u64>) -> (usize, usize);
    /// Every live token's `SaveKey`, JSON-erased. Ephemeral tokens
    /// (`save_key` returned `None`) are silently omitted — by design, they
    /// never round-trip.
    fn snapshot(&self, world: &World) -> Vec<HandleSaveEntry>;
    /// Resolve `referenced` ids against `persisted` (this kind's slice of
    /// the loaded [`HandleSaveState`]), inserting resolved resources back
    /// into the registry **under the same id** (token-id stability, spec
    /// §4). Ids in `referenced` absent from `persisted` are not reported
    /// here — the caller treats them as `dead_ephemeral` (a registered kind
    /// with no persisted entry for that id).
    fn rebind_selected(
        &self,
        world: &mut World,
        referenced: &BTreeSet<u64>,
        persisted: &[HandleSaveEntry],
    ) -> KindRehydrateOutcome;
}

struct RegistryOps<K: HandleKind>(PhantomData<fn() -> K>);

impl<K: HandleKind> Default for RegistryOps<K> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<K: HandleKind> ErasedHandleRegistry for RegistryOps<K> {
    fn kind_name(&self) -> &'static str {
        K::KIND
    }

    fn is_valid(&self, world: &World, id: u64) -> bool {
        world
            .get_resource::<HandleRegistry<K>>()
            .is_some_and(|reg| reg.contains(id))
    }

    fn gc_retain(&self, world: &mut World, keep: &BTreeSet<u64>) -> (usize, usize) {
        let Some(mut reg) = world.get_resource_mut::<HandleRegistry<K>>() else {
            return (0, 0);
        };
        let before = reg.live.len();
        reg.live.retain(|id, _| keep.contains(id));
        let after = reg.live.len();
        (before - after, after)
    }

    fn snapshot(&self, world: &World) -> Vec<HandleSaveEntry> {
        let Some(reg) = world.get_resource::<HandleRegistry<K>>() else {
            return Vec::new();
        };
        reg.live
            .iter()
            .filter_map(|(id, resource)| {
                let key = reg.implementor.save_key(world, resource)?;
                let key = serde_json::to_value(&key).ok()?;
                Some(HandleSaveEntry { id: *id, key })
            })
            .collect()
    }

    fn rebind_selected(
        &self,
        world: &mut World,
        referenced: &BTreeSet<u64>,
        persisted: &[HandleSaveEntry],
    ) -> KindRehydrateOutcome {
        let mut outcome = KindRehydrateOutcome::default();
        let by_id: BTreeMap<u64, &serde_json::Value> =
            persisted.iter().map(|e| (e.id, &e.key)).collect();
        world.resource_scope(
            |world, mut reg: bevy_ecs::change_detection::Mut<HandleRegistry<K>>| {
                for &id in referenced {
                    let Some(key_json) = by_id.get(&id) else {
                        // No persisted entry: dead_ephemeral, handled by the caller
                        // (it already knows this id was referenced but not in
                        // `persisted`).
                        continue;
                    };
                    let resolved = serde_json::from_value::<K::SaveKey>((*key_json).clone())
                        .ok()
                        .and_then(|key| reg.implementor.resolve(world, &key));
                    match resolved {
                        Some(resource) => {
                            reg.live.insert(id, resource);
                            reg.next_id = reg.next_id.max(id + 1);
                            outcome.rebound.push(id);
                        }
                        None => outcome.dead_by_resolve.push(id),
                    }
                }
            },
        );
        outcome
    }
}

/// Type-erased index over every kind registered via
/// [`BrinkHandleAppExt::register_handle_kind`] for marker `M`. `BTreeMap`
/// keyed by [`HandleKind::KIND`] for deterministic iteration.
#[derive(Resource)]
pub struct HandleKinds<M: Send + Sync + 'static = ()> {
    kinds: BTreeMap<&'static str, Box<dyn ErasedHandleRegistry>>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for HandleKinds<M> {
    fn default() -> Self {
        Self {
            kinds: BTreeMap::new(),
            _marker: PhantomData,
        }
    }
}

impl<M: Send + Sync + 'static> HandleKinds<M> {
    /// The kind names registered so far, in deterministic order.
    pub fn kind_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.kinds.keys().copied()
    }
}

/// App-builder extension for registering [`HandleKind`] implementors.
pub trait BrinkHandleAppExt {
    /// Register a [`HandleKind`] implementor for marker `M`: inserts its
    /// [`HandleRegistry<K>`] resource and indexes it in [`HandleKinds<M>`]
    /// for the type-erased operations (`is_valid`, GC, save/load).
    fn register_handle_kind<M: Send + Sync + 'static, K: HandleKind>(
        &mut self,
        implementor: K,
    ) -> &mut Self;
}

impl BrinkHandleAppExt for App {
    fn register_handle_kind<M: Send + Sync + 'static, K: HandleKind>(
        &mut self,
        implementor: K,
    ) -> &mut Self {
        self.world_mut()
            .insert_resource(HandleRegistry::<K>::new(implementor));
        self.world_mut()
            .get_resource_or_insert_with(HandleKinds::<M>::default);
        self.world_mut()
            .resource_mut::<HandleKinds<M>>()
            .kinds
            .insert(K::KIND, Box::new(RegistryOps::<K>::default()));
        self
    }
}

// ── Persistence beside SaveState ─────────────────────────────────────────

/// The token→`SaveKey` table, persisted beside the ink [`SaveState`] (spec
/// §4: "bevy-brink owns opaque token ids and the per-kind registries,
/// persists the token → `SaveKey` table beside the ink `SaveState`"). Keyed by
/// [`HandleKind::KIND`]; `BTreeMap`/sorted-by-id `Vec` for deterministic
/// serialization.
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct HandleSaveState {
    pub entries: BTreeMap<String, Vec<HandleSaveEntry>>,
}

/// Snapshot every registered kind's live tokens as a [`HandleSaveState`],
/// to be persisted alongside a [`SaveState`] (e.g.
/// [`BrinkGlobals::save_state`](crate::BrinkGlobals::save_state)).
#[must_use]
pub fn save_handles<M: Send + Sync + 'static>(world: &World) -> HandleSaveState {
    let mut out = HandleSaveState::default();
    if let Some(kinds) = world.get_resource::<HandleKinds<M>>() {
        for ops in kinds.kinds.values() {
            let entries = ops.snapshot(world);
            if !entries.is_empty() {
                out.entries.insert(ops.kind_name().to_string(), entries);
            }
        }
    }
    out
}

/// Host policy for handling a token whose kind isn't currently registered
/// at load time (spec §4). `Lenient` is the production default —
/// unregistered kinds are just reported, never-fail-load holds. `StrictKinds`
/// is the dev/CI knob: an unregistered kind fails the load loudly (a
/// registration drifted out of sync with a save file, which is a bug worth
/// surfacing immediately rather than silently dropping state).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RehydrationPolicy {
    #[default]
    Lenient,
    StrictKinds,
}

/// Load-time outcome for every handle token referenced by the ink state
/// being loaded, bucketed per spec §4.
#[derive(Debug, Clone, Default)]
pub struct RehydrationReport {
    /// Resolved to a live resource under the same token id.
    pub rebound: Vec<(String, u64)>,
    /// A registered kind, a persisted `SaveKey`, but `resolve` returned
    /// `None` — normal (the recipe no longer names anything live).
    pub dead_by_resolve: Vec<(String, u64)>,
    /// A registered kind with no persisted entry for this id — the kind
    /// chose ephemerality (`save_key` returned `None`) for this token.
    pub dead_ephemeral: Vec<(String, u64)>,
    /// The token's kind isn't registered at all under `Lenient` — suspicious
    /// (integration drift), surfaced for the host to log, never a fault.
    pub dead_by_unregistered_kind: Vec<(String, u64)>,
}

impl RehydrationReport {
    #[must_use]
    pub fn is_fully_rebound(&self) -> bool {
        self.dead_by_resolve.is_empty()
            && self.dead_ephemeral.is_empty()
            && self.dead_by_unregistered_kind.is_empty()
    }
}

/// [`load_handles`] failure — only reachable under
/// [`RehydrationPolicy::StrictKinds`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum HandleLoadError {
    /// One or more kinds referenced by the loaded state aren't registered.
    /// No registry was mutated — the load is atomic under `StrictKinds`.
    #[error("unregistered handle kind(s) at load: {0:?}")]
    UnregisteredKinds(Vec<String>),
}

/// Rehydrate every handle token `referenced` (the [`SaveState`] about to be
/// loaded — [`BrinkGlobals`](crate::BrinkGlobals)'s or one flow's) against
/// `persisted` (the companion [`HandleSaveState`] loaded alongside it),
/// keeping token ids stable (spec §4: "rebinds registries at load keeping
/// token ids stable — ink state is untouched; only the registry's
/// right-hand side rebinds").
///
/// Under [`RehydrationPolicy::StrictKinds`], any kind referenced by
/// `referenced` that isn't registered fails the whole call atomically
/// (nothing is mutated) with [`HandleLoadError::UnregisteredKinds`]. Under
/// `Lenient` (the production default — never-fail-load holds), those ids
/// land in [`RehydrationReport::dead_by_unregistered_kind`] instead.
pub fn load_handles<M: Send + Sync + 'static>(
    world: &mut World,
    program: &Program,
    referenced: &SaveState,
    persisted: &HandleSaveState,
    policy: RehydrationPolicy,
) -> Result<RehydrationReport, HandleLoadError> {
    let mut by_kind: BTreeMap<String, BTreeSet<u64>> = BTreeMap::new();
    collect_from_save_state(referenced, program, &mut by_kind);

    let registered: BTreeSet<&str> = world
        .get_resource::<HandleKinds<M>>()
        .map(|kinds| kinds.kind_names().collect())
        .unwrap_or_default();

    if policy == RehydrationPolicy::StrictKinds {
        let unregistered: Vec<String> = by_kind
            .keys()
            .filter(|k| !registered.contains(k.as_str()))
            .cloned()
            .collect();
        if !unregistered.is_empty() {
            return Err(HandleLoadError::UnregisteredKinds(unregistered));
        }
    }

    world.get_resource_or_insert_with(HandleEntityRemap::default);
    if let Some(mut remap) = world.get_resource_mut::<HandleEntityRemap>() {
        remap.clear();
    }

    let mut report = RehydrationReport::default();
    world.resource_scope::<HandleKinds<M>, _>(|world, kinds| {
        for (kind_name, ids) in &by_kind {
            let Some(ops) = kinds.kinds.get(kind_name.as_str()) else {
                report
                    .dead_by_unregistered_kind
                    .extend(ids.iter().map(|id| (kind_name.clone(), *id)));
                continue;
            };
            let persisted_for_kind = persisted
                .entries
                .get(kind_name)
                .map_or(&[][..], Vec::as_slice);
            let by_persisted_id: BTreeSet<u64> = persisted_for_kind.iter().map(|e| e.id).collect();
            let outcome = ops.rebind_selected(world, ids, persisted_for_kind);
            report.rebound.extend(
                outcome
                    .rebound
                    .into_iter()
                    .map(|id| (kind_name.clone(), id)),
            );
            report.dead_by_resolve.extend(
                outcome
                    .dead_by_resolve
                    .into_iter()
                    .map(|id| (kind_name.clone(), id)),
            );
            report.dead_ephemeral.extend(
                ids.iter()
                    .filter(|id| !by_persisted_id.contains(id))
                    .map(|id| (kind_name.clone(), *id)),
            );
        }
    });

    Ok(report)
}

// ── Reachable-token scan (shared by GC and load) ─────────────────────────

/// Recursively collect every [`Value::Handle`] token reachable from `value`
/// — including tokens nested in arrays, maps, records, and closure
/// bound-args — resolving each token's kind name against `program`.
fn collect_handles(value: &Value, program: &Program, out: &mut BTreeMap<String, BTreeSet<u64>>) {
    match value {
        Value::Handle { kind, id } => {
            if let Some(name) = program.name_checked(*kind) {
                out.entry(name.to_string()).or_default().insert(*id);
            }
        }
        Value::Array(items) => {
            for v in items.iter() {
                collect_handles(v, program, out);
            }
        }
        Value::Map(map) => {
            for v in map.values() {
                collect_handles(v, program, out);
            }
        }
        Value::Record { fields, .. } => {
            for v in fields.iter() {
                collect_handles(v, program, out);
            }
        }
        Value::Closure(closure) => {
            for entry in &closure.env {
                collect_handles(&entry.payload, program, out);
            }
        }
        _ => {}
    }
}

fn collect_from_save_state(
    save: &SaveState,
    program: &Program,
    out: &mut BTreeMap<String, BTreeSet<u64>>,
) {
    for value in save.globals.values() {
        collect_handles(value, program, out);
    }
}

// ── EntityMapper integration (spec §4) ───────────────────────────────────

/// An [`EntityMapper`] a `Resource = Entity` [`HandleKind`]'s `resolve` can
/// consult (`world.resource::<HandleEntityRemap>()`) and populate
/// (`set_mapped`) when reconstructing scene-based entities whose
/// cross-references named another handle-entity by its *old* session's
/// `Entity` id. Reset at the start of every [`load_handles`] call.
///
/// `BTreeMap` for deterministic iteration if a consumer ever walks it.
#[derive(Resource, Default, Debug)]
pub struct HandleEntityRemap {
    map: BTreeMap<Entity, Entity>,
}

impl HandleEntityRemap {
    pub fn clear(&mut self) {
        self.map.clear();
    }
}

impl EntityMapper for HandleEntityRemap {
    fn get_mapped(&mut self, source: Entity) -> Entity {
        self.map.get(&source).copied().unwrap_or(source)
    }

    fn set_mapped(&mut self, source: Entity, target: Entity) {
        self.map.insert(source, target);
    }
}

// ── Dead-deref host event ────────────────────────────────────────────────

/// Fired (opt-in — see [`HandleRegistry::get_or_dead`]) when a binding
/// dereferences a dead handle. Telemetry only: the binding itself still
/// returns whatever declared failure value it chooses; this event doesn't
/// change that value, it just lets a host observe the miss.
#[derive(EntityEvent)]
pub struct BrinkDeadHandleDeref<M: Send + Sync + 'static = ()> {
    pub entity: Entity,
    pub kind: &'static str,
    pub id: u64,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkDeadHandleDeref<M> {
    pub(crate) fn new(entity: Entity, kind: &'static str, id: u64) -> Self {
        Self {
            entity,
            kind,
            id,
            _marker: PhantomData,
        }
    }
}

// ── is_valid — standard world-query binding (spec §4) ────────────────────

/// The `is_valid(h)` binding body — ships as a standard
/// [`bind_brink_query`](crate::BrinkBindingsAppExt::bind_brink_query)
/// binding (not a language intrinsic, per spec §4). Registered
/// automatically by [`BrinkPlugin`](crate::BrinkPlugin) under the name
/// `"is_valid"`.
///
/// Returns `Value::Bool(false)` for anything that isn't a live, registered
/// handle: a non-handle argument, an unregistered kind, or a dead token —
/// `is_valid` never faults.
pub fn is_valid_system<M: Send + Sync + 'static>(
    In((entity, args)): In<BrinkQueryInput>,
    world: &World,
) -> Value {
    let Some((kind, id)) = args.first().and_then(Value::as_handle) else {
        return Value::Bool(false);
    };
    let Some(program_component) = world.get::<BrinkProgram<M>>(entity) else {
        return Value::Bool(false);
    };
    let Some(program) = world
        .get_resource::<Assets<ProgramAsset>>()
        .and_then(|assets| assets.get(&program_component.handle))
    else {
        return Value::Bool(false);
    };
    let Some(kind_name) = program.program.name_checked(kind) else {
        return Value::Bool(false);
    };
    let Some(kinds) = world.get_resource::<HandleKinds<M>>() else {
        return Value::Bool(false);
    };
    let Some(ops) = kinds.kinds.get(kind_name) else {
        return Value::Bool(false);
    };
    Value::Bool(ops.is_valid(world, id))
}

// ── Snapshot-retention dev metric (spec §8) ──────────────────────────────

/// Per-kind live/GC counters, updated by [`gc_on_turn_done`]. A diagnostics
/// feature, not a semantic (spec §8: "the dev-build snapshot-retention
/// metric rides the bevy-brink slice as a diagnostics feature").
#[derive(Debug, Clone, Default)]
pub struct KindRetention {
    /// Live token count as of the last sweep.
    pub live: usize,
    /// Tokens dropped by the last sweep.
    pub last_gc_dropped: usize,
    /// Total sweeps this kind has been through.
    pub sweeps: u64,
}

#[derive(Resource, Debug, Clone)]
pub struct HandleRetentionMetrics<M: Send + Sync + 'static = ()> {
    pub per_kind: BTreeMap<String, KindRetention>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for HandleRetentionMetrics<M> {
    fn default() -> Self {
        Self {
            per_kind: BTreeMap::new(),
            _marker: PhantomData,
        }
    }
}

impl<M: Send + Sync + 'static> HandleRetentionMetrics<M> {
    fn record(&mut self, kind: &str, dropped: usize, live: usize) {
        let entry = self.per_kind.entry(kind.to_string()).or_default();
        entry.live = live;
        entry.last_gc_dropped = dropped;
        entry.sweeps += 1;
    }
}

// ── Registry GC at -> DONE quiescent sweeps (spec §4) ────────────────────

/// Drop every registered kind's unreachable registry entries, given the
/// already-computed reachable token set (kind name → live ids) — the
/// mutating half of [`gc_on_turn_done`], run through `Commands::queue` so
/// the type-erased dispatch gets its own `&mut World`.
fn sweep_registries<M: Send + Sync + 'static>(
    world: &mut World,
    reachable: &BTreeMap<String, BTreeSet<u64>>,
) {
    let empty = BTreeSet::new();
    world.resource_scope::<HandleKinds<M>, _>(|world, kinds| {
        for ops in kinds.kinds.values() {
            let keep = reachable.get(ops.kind_name()).unwrap_or(&empty);
            let (dropped, live) = ops.gc_retain(world, keep);
            if let Some(mut metrics) = world.get_resource_mut::<HandleRetentionMetrics<M>>() {
                metrics.record(ops.kind_name(), dropped, live);
            }
        }
    });
}

/// Observer: at every `-> DONE` (spec §4's quiescent sweep point), computes
/// the currently-reachable handle-token set — every token in the shared
/// `World`'s globals plus every flow's own local state, script state being
/// fully enumerable (value-model §6 license) — and drops every registered
/// kind's unreachable entries. No script-side destructors exist or are
/// needed.
///
/// Registered automatically by [`BrinkPlugin`](crate::BrinkPlugin).
#[expect(
    clippy::needless_pass_by_value,
    reason = "bevy systems take Res params by value"
)]
pub fn gc_on_turn_done<M: Send + Sync + 'static>(
    _on: On<BrinkTurnDone<M>>,
    mut globals: Option<ResMut<BrinkGlobals<M>>>,
    programs: Res<Assets<ProgramAsset>>,
    mut contexts: Query<(&BrinkProgram<M>, &mut BrinkContext<M>)>,
    mut commands: Commands,
) {
    let Some(globals) = globals.as_deref_mut() else {
        return;
    };

    let mut reachable: BTreeMap<String, BTreeSet<u64>> = BTreeMap::new();
    // Every flow's own program contributes its globals-view + local state —
    // deliberately every flow under `M` (not just the one that just
    // quiesced) so a still-mid-conversation flow's private handle
    // references aren't GC'd out from under it just because a *different*
    // flow reached `-> DONE`.
    for (program_component, mut ctx) in &mut contexts {
        let Some(program_asset) = programs.get(&program_component.handle) else {
            continue;
        };
        let state = save_flow_state(globals, &mut ctx, &program_asset.program);
        collect_from_save_state(&state, &program_asset.program, &mut reachable);
    }

    commands.queue(move |world: &mut World| {
        sweep_registries::<M>(world, &reachable);
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet as Set;

    use bevy_ecs::system::RunSystemOnce as _;
    use brink_format::SaveState;
    use brink_runtime::ContextAccess as _;
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::BrinkFlowRequest;
    use crate::bindings::advance_flow;
    use crate::test_support::{add_story_assets, compile_test_story, make_test_app};

    // ── Canonical example kinds ───────────────────────────────────────────

    /// The reconstruction-recipe canonical example (spec §4): a timer isn't
    /// looked up, it's rebuilt from its remaining duration — resumable.
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
    struct TimerSaveKey {
        remaining_secs: f32,
    }
    #[derive(Debug, Clone, Copy, PartialEq)]
    struct TimerState {
        remaining_secs: f32,
    }
    struct TimerKind;
    impl HandleKind for TimerKind {
        const KIND: &'static str = "Timer";
        type Resource = TimerState;
        type SaveKey = TimerSaveKey;
        fn save_key(&self, _world: &World, res: &Self::Resource) -> Option<Self::SaveKey> {
            Some(TimerSaveKey {
                remaining_secs: res.remaining_secs,
            })
        }
        fn resolve(&self, _world: &mut World, key: &Self::SaveKey) -> Option<Self::Resource> {
            Some(TimerState {
                remaining_secs: key.remaining_secs,
            })
        }
    }

    /// An identity-lookup kind whose `resolve` depends on a host-side
    /// "still alive" registry the test controls — models the ordinary
    /// "the named resource may or may not still exist" case.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct NpcSaveKey {
        guid: String,
    }
    #[derive(Debug, Clone, PartialEq)]
    struct NpcState {
        guid: String,
    }
    struct NpcKind;
    impl HandleKind for NpcKind {
        const KIND: &'static str = "Npc";
        type Resource = NpcState;
        type SaveKey = NpcSaveKey;
        fn save_key(&self, _world: &World, res: &Self::Resource) -> Option<Self::SaveKey> {
            Some(NpcSaveKey {
                guid: res.guid.clone(),
            })
        }
        fn resolve(&self, world: &mut World, key: &Self::SaveKey) -> Option<Self::Resource> {
            let alive = world.get_resource::<AliveNpcs>()?;
            alive.0.contains(&key.guid).then(|| NpcState {
                guid: key.guid.clone(),
            })
        }
    }
    #[derive(Resource, Default)]
    struct AliveNpcs(Set<String>);

    /// The deliberate-ephemerality kind: `save_key` always returns `None`.
    struct TransientKind;
    impl HandleKind for TransientKind {
        const KIND: &'static str = "Transient";
        type Resource = ();
        type SaveKey = ();
        fn save_key(&self, _world: &World, (): &Self::Resource) -> Option<Self::SaveKey> {
            None
        }
        fn resolve(&self, _world: &mut World, (): &Self::SaveKey) -> Option<Self::Resource> {
            Some(())
        }
    }

    fn empty_save_state() -> SaveState {
        SaveState {
            version: brink_runtime::SAVE_FORMAT_VERSION,
            globals: BTreeMap::new(),
            visits: Vec::new(),
            turns: Vec::new(),
            turn_index: 0,
            rng_seed: 0,
            previous_random: 0,
        }
    }

    fn referencing(global: &str, value: Value) -> SaveState {
        let mut save = empty_save_state();
        save.globals.insert(global.to_string(), value);
        save
    }

    // ── HandleRegistry basics ─────────────────────────────────────────────

    #[test]
    fn mint_and_get_roundtrip() {
        let mut reg = HandleRegistry::<TimerKind>::new(TimerKind);
        let id = reg.mint(TimerState {
            remaining_secs: 3.0,
        });
        assert_eq!(
            reg.get(id),
            Some(&TimerState {
                remaining_secs: 3.0
            })
        );
        assert!(reg.contains(id));
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn mint_value_none_when_kind_never_interned() {
        let (program, tables, ctx) = compile_test_story("Hi.\n-> DONE\n");
        let mut app = make_test_app();
        add_story_assets(&mut app, program, tables, ctx);
        let program = &app
            .world()
            .resource::<Assets<ProgramAsset>>()
            .iter()
            .next()
            .expect("one program asset")
            .1
            .program;
        let mut reg = HandleRegistry::<TimerKind>::new(TimerKind);
        // "Timer" is never mentioned in the compiled source, so it was
        // never interned — no NameId to build a token against.
        assert!(
            reg.mint_value(
                program,
                TimerState {
                    remaining_secs: 1.0
                }
            )
            .is_none()
        );
    }

    #[test]
    fn mint_value_resolves_interned_kind_name() {
        let (program, tables, ctx) = compile_test_story("VAR Timer = 0\nHi.\n-> DONE\n");
        let mut reg = HandleRegistry::<TimerKind>::new(TimerKind);
        let value = reg
            .mint_value(
                &program,
                TimerState {
                    remaining_secs: 5.0,
                },
            )
            .expect("Timer was interned via the VAR declaration");
        let (kind, _id) = value.as_handle().expect("a handle value");
        assert_eq!(program.name_checked(kind), Some("Timer"));
        drop(tables);
        drop(ctx);
    }

    // ── is_valid ────────────────────────────────────────────────────────

    #[test]
    fn is_valid_true_for_live_registered_handle() {
        let (program, tables, ctx) = compile_test_story("VAR Timer = 0\nHi.\n-> DONE\n");
        let mut app = make_test_app();
        app.register_handle_kind::<(), TimerKind>(TimerKind);
        let story = add_story_assets(&mut app, program, tables, ctx);
        let entity = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();
        app.update(); // fulfill: entity gains BrinkProgram<()>

        let handle_value = {
            let world = app.world_mut();
            let program = &world
                .resource::<Assets<ProgramAsset>>()
                .iter()
                .next()
                .expect("program asset")
                .1
                .program;
            let kind = program.name_id("Timer").expect("interned");
            let mut reg = world.resource_mut::<HandleRegistry<TimerKind>>();
            Value::handle(
                kind,
                reg.mint(TimerState {
                    remaining_secs: 2.0,
                }),
            )
        };

        let result = app
            .world_mut()
            .run_system_once_with(is_valid_system::<()>, (entity, vec![handle_value]))
            .expect("is_valid runs");
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn is_valid_false_for_dead_or_non_handle() {
        let (program, tables, ctx) = compile_test_story("VAR Timer = 0\nHi.\n-> DONE\n");
        let mut app = make_test_app();
        app.register_handle_kind::<(), TimerKind>(TimerKind);
        let story = add_story_assets(&mut app, program, tables, ctx);
        let entity = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();
        app.update();

        // Not a handle at all.
        let result = app
            .world_mut()
            .run_system_once_with(is_valid_system::<()>, (entity, vec![Value::Int(1)]))
            .expect("is_valid runs");
        assert_eq!(result, Value::Bool(false));

        // A well-formed but never-minted token id.
        let kind = {
            let world = app.world_mut();
            let program = &world
                .resource::<Assets<ProgramAsset>>()
                .iter()
                .next()
                .expect("program asset")
                .1
                .program;
            program.name_id("Timer").expect("interned")
        };
        let result = app
            .world_mut()
            .run_system_once_with(
                is_valid_system::<()>,
                (entity, vec![Value::handle(kind, 9999)]),
            )
            .expect("is_valid runs");
        assert_eq!(result, Value::Bool(false));
    }

    // ── save_handles / load_handles: the three round-trips from #775 ──────

    #[test]
    fn save_resolve_live() {
        let (program, _tables, _ctx) =
            compile_test_story("VAR npc_ref = 0\nVAR Npc = 0\nHi.\n-> DONE\n");
        let mut world = World::new();
        world.insert_resource(HandleKinds::<()>::default());
        world.insert_resource(AliveNpcs(Set::from(["abc".to_string()])));
        world.get_resource_or_insert_with(HandleKinds::<()>::default);
        // Register directly (no App needed for this pure round-trip).
        world.insert_resource(HandleRegistry::<NpcKind>::new(NpcKind));
        world
            .resource_mut::<HandleKinds<()>>()
            .kinds
            .insert(NpcKind::KIND, Box::new(RegistryOps::<NpcKind>::default()));

        let kind = program.name_id("Npc").expect("interned");
        let id = world
            .resource_mut::<HandleRegistry<NpcKind>>()
            .mint(NpcState {
                guid: "abc".to_string(),
            });

        let persisted = save_handles::<()>(&world);
        assert_eq!(persisted.entries["Npc"].len(), 1);

        let referenced = referencing("npc_ref", Value::handle(kind, id));
        let report = load_handles::<()>(
            &mut world,
            &program,
            &referenced,
            &persisted,
            RehydrationPolicy::Lenient,
        )
        .expect("lenient load never errors");

        assert_eq!(report.rebound, vec![("Npc".to_string(), id)]);
        assert!(report.is_fully_rebound());
        assert_eq!(
            world.resource::<HandleRegistry<NpcKind>>().get(id),
            Some(&NpcState {
                guid: "abc".to_string()
            })
        );
    }

    #[test]
    fn save_despawn_load_dead_declared_fallback() {
        let (program, _tables, _ctx) =
            compile_test_story("VAR npc_ref = 0\nVAR Npc = 0\nHi.\n-> DONE\n");
        let mut world = World::new();
        world.insert_resource(HandleKinds::<()>::default());
        world.insert_resource(AliveNpcs(Set::from(["abc".to_string()])));
        world.insert_resource(HandleRegistry::<NpcKind>::new(NpcKind));
        world
            .resource_mut::<HandleKinds<()>>()
            .kinds
            .insert(NpcKind::KIND, Box::new(RegistryOps::<NpcKind>::default()));

        let kind = program.name_id("Npc").expect("interned");
        let id = world
            .resource_mut::<HandleRegistry<NpcKind>>()
            .mint(NpcState {
                guid: "abc".to_string(),
            });
        let persisted = save_handles::<()>(&world);

        // Despawn: the NPC is gone by load time (not in the "alive" set
        // any more), and the live registry entry from the old session
        // doesn't survive a real process restart either.
        world.resource_mut::<AliveNpcs>().0.clear();
        world.resource_mut::<HandleRegistry<NpcKind>>().remove(id);

        let referenced = referencing("npc_ref", Value::handle(kind, id));
        let report = load_handles::<()>(
            &mut world,
            &program,
            &referenced,
            &persisted,
            RehydrationPolicy::Lenient,
        )
        .expect("lenient load never errors");

        assert_eq!(report.dead_by_resolve, vec![("Npc".to_string(), id)]);
        assert!(!report.is_fully_rebound());
        assert_eq!(world.resource::<HandleRegistry<NpcKind>>().get(id), None);

        // The binding-side declared-failure-value pattern: a binding that
        // dereferences this now-dead token falls back to whatever value it
        // has chosen to declare (here, -1) rather than faulting.
        let declared_fallback = world
            .resource::<HandleRegistry<NpcKind>>()
            .get(id)
            .map_or(Value::Int(-1), |_| Value::Int(0));
        assert_eq!(declared_fallback, Value::Int(-1));
    }

    #[test]
    fn timer_reconstruction_after_restart() {
        let (program, _tables, _ctx) =
            compile_test_story("VAR timer_ref = 0\nVAR Timer = 0\nHi.\n-> DONE\n");
        let mut world = World::new();
        world.insert_resource(HandleKinds::<()>::default());
        world.insert_resource(HandleRegistry::<TimerKind>::new(TimerKind));
        world.resource_mut::<HandleKinds<()>>().kinds.insert(
            TimerKind::KIND,
            Box::new(RegistryOps::<TimerKind>::default()),
        );

        let kind = program.name_id("Timer").expect("interned");
        let id = world
            .resource_mut::<HandleRegistry<TimerKind>>()
            .mint(TimerState {
                remaining_secs: 12.5,
            });
        let persisted = save_handles::<()>(&world);
        assert_eq!(
            persisted.entries["Timer"][0].key,
            serde_json::json!({ "remaining_secs": 12.5 })
        );

        // Simulate a fresh process: the old TimerState instance is gone,
        // only the persisted recipe (remaining duration) survives.
        world.resource_mut::<HandleRegistry<TimerKind>>().remove(id);

        let referenced = referencing("timer_ref", Value::handle(kind, id));
        let report = load_handles::<()>(
            &mut world,
            &program,
            &referenced,
            &persisted,
            RehydrationPolicy::Lenient,
        )
        .expect("lenient load never errors");

        assert_eq!(report.rebound, vec![("Timer".to_string(), id)]);
        // Same token id (stability), freshly reconstructed resource with
        // the recipe's remaining duration — the timer resumed, not just
        // "found again".
        assert_eq!(
            world.resource::<HandleRegistry<TimerKind>>().get(id),
            Some(&TimerState {
                remaining_secs: 12.5
            })
        );
    }

    #[test]
    fn dead_ephemeral_when_kind_declines_to_persist() {
        let (program, _tables, _ctx) =
            compile_test_story("VAR t_ref = 0\nVAR Transient = 0\nHi.\n-> DONE\n");
        let mut world = World::new();
        world.insert_resource(HandleKinds::<()>::default());
        world.insert_resource(HandleRegistry::<TransientKind>::new(TransientKind));
        world.resource_mut::<HandleKinds<()>>().kinds.insert(
            TransientKind::KIND,
            Box::new(RegistryOps::<TransientKind>::default()),
        );

        let kind = program.name_id("Transient").expect("interned");
        let id = world
            .resource_mut::<HandleRegistry<TransientKind>>()
            .mint(());

        let persisted = save_handles::<()>(&world);
        // Ephemeral by choice: save_key always returns None, so nothing
        // was ever written for this kind.
        assert!(!persisted.entries.contains_key("Transient"));

        let referenced = referencing("t_ref", Value::handle(kind, id));
        let report = load_handles::<()>(
            &mut world,
            &program,
            &referenced,
            &persisted,
            RehydrationPolicy::Lenient,
        )
        .expect("lenient load never errors");

        assert_eq!(report.dead_ephemeral, vec![("Transient".to_string(), id)]);
        assert!(report.rebound.is_empty());
        assert!(report.dead_by_resolve.is_empty());
    }

    #[test]
    fn unregistered_kind_lenient_reports_strict_fails() {
        let (program, _tables, _ctx) =
            compile_test_story("VAR ghost_ref = 0\nVAR Ghost = 0\nHi.\n-> DONE\n");
        let kind = program.name_id("Ghost").expect("interned");
        let referenced = referencing("ghost_ref", Value::handle(kind, 7));
        let persisted = HandleSaveState::default();

        let mut lenient_world = World::new();
        lenient_world.insert_resource(HandleKinds::<()>::default());
        let report = load_handles::<()>(
            &mut lenient_world,
            &program,
            &referenced,
            &persisted,
            RehydrationPolicy::Lenient,
        )
        .expect("lenient never errors, even for unregistered kinds");
        assert_eq!(
            report.dead_by_unregistered_kind,
            vec![("Ghost".to_string(), 7)]
        );

        let mut strict_world = World::new();
        strict_world.insert_resource(HandleKinds::<()>::default());
        let err = load_handles::<()>(
            &mut strict_world,
            &program,
            &referenced,
            &persisted,
            RehydrationPolicy::StrictKinds,
        )
        .expect_err("StrictKinds fails loudly on an unregistered kind");
        assert_eq!(
            err,
            HandleLoadError::UnregisteredKinds(vec!["Ghost".to_string()])
        );
    }

    // ── Registry GC ─────────────────────────────────────────────────────

    #[test]
    fn sweep_drops_unreachable_keeps_reachable() {
        let mut world = World::new();
        world.insert_resource(HandleKinds::<()>::default());
        world.insert_resource(HandleRetentionMetrics::<()>::default());
        world.insert_resource(HandleRegistry::<TimerKind>::new(TimerKind));
        world.resource_mut::<HandleKinds<()>>().kinds.insert(
            TimerKind::KIND,
            Box::new(RegistryOps::<TimerKind>::default()),
        );

        let (reachable_id, orphan_id) = {
            let mut reg = world.resource_mut::<HandleRegistry<TimerKind>>();
            (
                reg.mint(TimerState {
                    remaining_secs: 1.0,
                }),
                reg.mint(TimerState {
                    remaining_secs: 2.0,
                }),
            )
        };

        let mut reachable = BTreeMap::new();
        reachable.insert("Timer".to_string(), Set::from([reachable_id]));
        sweep_registries::<()>(&mut world, &reachable);

        let reg = world.resource::<HandleRegistry<TimerKind>>();
        assert!(reg.contains(reachable_id));
        assert!(!reg.contains(orphan_id));

        let metrics = world.resource::<HandleRetentionMetrics<()>>();
        let timer = &metrics.per_kind["Timer"];
        assert_eq!(timer.live, 1);
        assert_eq!(timer.last_gc_dropped, 1);
        assert_eq!(timer.sweeps, 1);
    }

    /// End-to-end: a real flow reaching `-> DONE` through
    /// [`crate::bindings::advance_flow`] fires [`BrinkTurnDone`], which
    /// [`BrinkPlugin`](crate::BrinkPlugin) has wired to [`gc_on_turn_done`] —
    /// proving the GC sweep is reachable from actual story playback, not
    /// just callable in isolation.
    #[test]
    fn gc_on_turn_done_is_wired_by_the_plugin() {
        let (program, tables, ctx) =
            compile_test_story("VAR target = 0\nVAR Timer = 0\nHi.\n-> DONE\n");
        let mut app = make_test_app();
        app.register_handle_kind::<(), TimerKind>(TimerKind);
        let story = add_story_assets(&mut app, program, tables, ctx);
        let entity = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();
        app.update(); // fulfill

        let (kind, target_idx) = {
            let world = app.world();
            let program = &world
                .resource::<Assets<ProgramAsset>>()
                .iter()
                .next()
                .expect("program asset")
                .1
                .program;
            (
                program.name_id("Timer").expect("interned"),
                program.global_index("target").expect("declared"),
            )
        };
        let (reachable_id, orphan_id) = {
            let mut reg = app.world_mut().resource_mut::<HandleRegistry<TimerKind>>();
            (
                reg.mint(TimerState {
                    remaining_secs: 1.0,
                }),
                reg.mint(TimerState {
                    remaining_secs: 2.0,
                }),
            )
        };

        // Make `target` (a World-scoped global) hold the reachable token,
        // so the GC sweep's reachability scan picks it up from
        // `BrinkGlobals`'s own state.
        app.world_mut()
            .resource_mut::<BrinkGlobals<()>>()
            .inner
            .set_global(target_idx, Value::handle(kind, reachable_id));

        {
            let world = app.world_mut();
            let _ = advance_flow::<()>(world, entity).expect("advances to -> DONE");
            world.flush();
        }
        app.update();

        let world = app.world();
        let reg = world.resource::<HandleRegistry<TimerKind>>();
        assert!(
            reg.contains(reachable_id),
            "reachable token must survive the sweep"
        );
        assert!(
            !reg.contains(orphan_id),
            "unreferenced token must be dropped by the -> DONE sweep"
        );
    }
}
