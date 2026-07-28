//! BH-1: the bevy host capability join (`docs/effects-spec.md` §9, §12–§13;
//! tracking #897, this slice #899).
//!
//! Pure data plumbing — no scheduling, no `unsafe`. This module owns the
//! host half of the "ECS join" §9 describes: brink-side rows speak cells +
//! call kinds (all the compiler can see, shipped in `.inkb`'s `EffectRows`
//! section, T2-3/PR #878); the host manifest declares each binding's
//! capability signature in engine vocabulary (§13.2); at story load, the two
//! join into a `bevy_ecs::query::Access` per container — the same currency
//! bevy's own executor uses, so a later scheduler (BH-3) can test
//! disjointness with `Access::is_compatible` directly.
//!
//! - [`CapabilityManifest`]/[`CapabilityManifestExternal`]/[`CapabilityEffects`]
//!   — §13.2 grammar: `{"name": ..., "effects": {"reads": [...], "writes":
//!   [...], "detect": {...}}}`. Capability names are engine-vocabulary
//!   strings, **compiler-opaque** — deserialized here as plain `String`s;
//!   bevy-brink is the only thing that gives them meaning.
//! - [`CapabilityRegistry`]/[`BrinkCapabilityAppExt::register_capability`] —
//!   the app-level name → `ComponentId` map, mirroring the
//!   [`HandleKind`](crate::HandleKind) registration pattern
//!   (`crates/bevy-brink/src/handle.rs`, T1d-3/PR #780): an app-builder
//!   extension trait, a type-keyed `Resource` (here keyed by string name
//!   directly rather than by a per-kind trait, since a capability is just a
//!   `ComponentId` — no save/resolve halves to erase).
//! - [`compute_container_access`] — the row join: for every
//!   [`EffectRowEntry`] a loaded story ships, resolve its call atoms'
//!   `NameId`s back to external names, look each up in the manifest, resolve
//!   the declared capability names against the registry (an unregistered
//!   name is a load-time [`CapabilityError::UnknownCapability`], never a
//!   silent drop), and fold the result into one [`Access`] per container —
//!   also walking every dispatch's static fallback row (`docs/effects-spec.md`
//!   §7: v1 does no runtime narrowing, so the conservative fallback always
//!   applies; skipping it would under-report access, the one soundness
//!   direction §3 forbids).
//! - [`CapabilityTable`]/[`rebuild_capability_table`] — the load/unload
//!   boundary: a `Resource` keyed by `AssetId<ProgramAsset>`, rebuilt
//!   whenever a story (re)loads and torn down when it unloads (§12.5's
//!   "story load/unload is when the params rebuild" invariant, applied here
//!   to the per-container `Access` table rather than a `SystemParamBuilder`).
//! - [`dump_container_access`] — the dev-visible debug fn (container → access
//!   set), for BH-B's scenario harness and interactive debugging.
//! - [`missing_capabilities`]/[`MissingCapability`]/[`CapabilityError::LoadRejected`]/
//!   [`check_load_capability_gate`] — issue #912's load-boundary admission
//!   check: the manifest stays app-global while [`CapabilityRegistry`] is
//!   per-marker `M`, so a story loaded under a marker whose registry lacks a
//!   manifest-required capability must fail to load *at all* under that
//!   marker, loudly, rather than joining to a silently-incomplete
//!   `UnknownCapability` err-table at call time.
//!   [`check_load_capability_gate`] is the one shared helper every
//!   load-shaped entry point must call before constructing a `FlowInstance`
//!   from a `ProgramAsset` (#997): `bevy-brink`'s `fulfill_flow_requests::<M>`
//!   (`crates/bevy-brink/src/request.rs`, the initial load) and
//!   `replay_on_reload::<M>` (`crates/bevy-brink/src/replay.rs`, the dev-only
//!   hot-reload reconstruction) both call it at their respective
//!   story-construction boundary and refuse to proceed when it errs.

use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;

use bevy_app::{App, Update};
use bevy_asset::{AssetEvent, AssetId, Assets};
use bevy_ecs::component::{Component, ComponentId};
use bevy_ecs::message::MessageReader;
use bevy_ecs::query::{Access, Changed};
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs as _;
use bevy_ecs::schedule::common_conditions::any_with_component;
use bevy_ecs::system::{Query, Res, ResMut};
use bevy_log::error;
use brink_format::{CallAtom, DefinitionId, DirectEffects, EffectRowEntry};
use brink_runtime::Program;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::asset::ProgramAsset;

// ── Manifest grammar (§13.2) ─────────────────────────────────────────────

/// The `effects` object on a manifest external (`docs/effects-spec.md`
/// §13.2): `{"reads": [...], "writes": [...], "detect": {...}}`. Every field
/// is optional (defaults empty) — an external with no `effects` key at all
/// touches no ECS capability.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityEffects {
    /// Capability names this external reads. Compiler-opaque engine
    /// vocabulary (e.g. `"Transform"`) — meaningless until resolved through a
    /// [`CapabilityRegistry`].
    #[serde(default)]
    pub reads: Vec<String>,
    /// Capability names this external writes.
    #[serde(default)]
    pub writes: Vec<String>,
    /// Capability name → change-detection-backed bit: `true` means bevy's
    /// own change ticks can back a wake/reactive-sleep dependency on this
    /// capability; `false` (or absent) means it must be polled. Consumed by
    /// BH-4's Detect phase (`crate::sleep`) after the per-container
    /// AND/conservative merge (`#913`).
    #[serde(default)]
    pub detect: BTreeMap<String, bool>,
}

/// One external's manifest entry, restricted to the fields BH-1 needs.
/// Deserialized from the **same** JSON manifest file
/// `docs/host-capability-manifest.md`/`brink_ir::host_manifest` describes for
/// the compiler/IDE side (`name`, `params`, `kind`, `doc`, `widgets`, `path`,
/// …) — this type only names `name` and `effects`; every other key present in
/// a real manifest file is ignored by `serde`'s default "unknown fields are
/// fine" behavior, so the same file serves both consumers.
///
/// **Not converged onto `brink_ir::host_manifest::ManifestExternal`** (issue
/// #911, BH follow-up deliverable 1) — see that type's module doc for the
/// full rationale (opposite-direction crate dependency the two sides must
/// never take on each other). The two shared keys (`externals`, `name`) are
/// pinned by `brink_format::manifest_field_names`; `tests/manifest_field_convergence.rs`
/// cross-validates one manifest literal against both types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityManifestExternal {
    pub name: String,
    #[serde(default)]
    pub effects: CapabilityEffects,
}

/// The top-level manifest shape: `{"externals": [...]}`. Register one as a
/// `Resource` (`app.insert_resource(CapabilityManifest::from_json(json)?)`)
/// before or after adding [`crate::BrinkPlugin`] — order doesn't matter,
/// [`BrinkPlugin`](crate::BrinkPlugin) only `init_resource`s an empty default
/// if none is present yet.
#[derive(Debug, Clone, Default, PartialEq, Eq, Resource, Serialize, Deserialize)]
pub struct CapabilityManifest {
    #[serde(default)]
    pub externals: Vec<CapabilityManifestExternal>,
}

impl CapabilityManifest {
    /// Parse a manifest from its JSON text (§13.2 grammar).
    pub fn from_json(json: &str) -> Result<Self, CapabilityError> {
        Ok(serde_json::from_str(json)?)
    }

    /// Look up an external's manifest entry by name. First match on
    /// duplicate names (not a documented case; manifests are host-authored).
    #[must_use]
    pub fn external(&self, name: &str) -> Option<&CapabilityManifestExternal> {
        self.externals.iter().find(|e| e.name == name)
    }
}

// ── Errors ────────────────────────────────────────────────────────────────

/// Errors from manifest parsing or the row join.
#[derive(Debug, Error)]
pub enum CapabilityError {
    #[error("capability manifest JSON is malformed: {0}")]
    ManifestJson(#[from] serde_json::Error),
    /// The tier-1 admission rule: a manifest-declared capability name that no
    /// `register_capability` call ever registered. Always a load-time error
    /// — never silently dropped (an unregistered name means the join cannot
    /// prove which `ComponentId` the row may touch, which would under-report
    /// access, the one direction `docs/effects-spec.md` §3 forbids).
    #[error(
        "external `{external}` declares capability `{capability}` in its effects manifest, \
         but no `register_capability::<_, _>(\"{capability}\")` call has registered that name \
         — capability join cannot proceed for this story"
    )]
    UnknownCapability {
        external: String,
        capability: String,
    },
    /// Load-boundary hard error (issue #912, RULED 2026-07-18 option (b)):
    /// a story failed to load under a marker because that marker's
    /// [`CapabilityRegistry`] is missing one or more manifest-required
    /// capabilities. Raised by `bevy-brink`'s `fulfill_flow_requests` at
    /// the story-load boundary itself — the tier-1 admission posture
    /// applied per-marker. Before this variant existed, an unregistered
    /// name only ever surfaced as a per-story [`UnknownCapability`] logged
    /// into [`CapabilityTable`] at call time (a silent err-table); this is
    /// the load refusing to happen at all, loudly, naming the marker, the
    /// story, and every missing capability at once (not just the first).
    #[error(
        "story `{story}` failed to load under marker `{marker}`: this marker's \
         CapabilityRegistry is missing {} manifest-required capability name(s): {}",
        missing.len(),
        missing
            .iter()
            .map(|m| format!("`{}` (required by external `{}`)", m.capability, m.external))
            .collect::<Vec<_>>()
            .join(", ")
    )]
    LoadRejected {
        /// The marker type name (`std::any::type_name::<M>()`) the story
        /// was loading under.
        marker: &'static str,
        /// A human-readable identifier for the story — its asset path if
        /// the handle carries one, otherwise its `AssetId` debug form.
        story: String,
        /// Every manifest-required capability name this marker's registry
        /// doesn't recognize, deduplicated and sorted (`external`, then
        /// `capability`).
        missing: Vec<MissingCapability>,
    },
}

/// One manifest-declared capability name a marker's [`CapabilityRegistry`]
/// doesn't recognize — the unit [`missing_capabilities`] collects and
/// [`CapabilityError::LoadRejected`] reports in full (issue #912).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MissingCapability {
    /// The external whose manifest entry declares the missing capability.
    pub external: String,
    /// The capability name itself (engine vocabulary, e.g. `"Transform"`).
    pub capability: String,
}

// ── Registry: name → ComponentId (mirrors HandleKind's registration) ────

/// App-level registry mapping capability names to `ComponentId`s, keyed by
/// marker `M` (mirrors [`HandleKinds<M>`](crate::HandleKinds)). Populated via
/// [`BrinkCapabilityAppExt::register_capability`]. `BTreeMap` for
/// deterministic iteration (CLAUDE.md determinism rule).
#[derive(Resource)]
pub struct CapabilityRegistry<M: Send + Sync + 'static = ()> {
    names: BTreeMap<&'static str, ComponentId>,
    /// Parallel name → concrete-component `TypeId` map. Keyed the same way as
    /// `names`, but valued by `TypeId` rather than `ComponentId` because the
    /// §12.5 change-tick tracker ([`CapabilityChanges`]) keys its per-frame
    /// verdict by `TypeId` — the one identity a
    /// [`detect_capability_changes`] system (generic over the concrete `C`)
    /// can compute at runtime with no `&World`/`Components` access.
    type_ids: BTreeMap<&'static str, TypeId>,
    /// Component `TypeId`s that already have a [`detect_capability_changes`]
    /// system wired into `Update` — the idempotency guard so registering the
    /// same component (or two names for one component) never double-adds its
    /// change-tracker system.
    detect_wired: BTreeSet<TypeId>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for CapabilityRegistry<M> {
    fn default() -> Self {
        Self {
            names: BTreeMap::new(),
            type_ids: BTreeMap::new(),
            detect_wired: BTreeSet::new(),
            _marker: PhantomData,
        }
    }
}

impl<M: Send + Sync + 'static> CapabilityRegistry<M> {
    /// Resolve a registered capability name to its `ComponentId`. `None`
    /// means no `register_capability` call has claimed this name yet.
    #[must_use]
    pub fn component_id(&self, name: &str) -> Option<ComponentId> {
        self.names.get(name).copied()
    }

    /// Resolve a registered capability name to its concrete component's
    /// `TypeId` — the key BH-4's Detect phase (`crate::sleep`) uses to look
    /// the capability's per-frame change verdict up in [`CapabilityChanges`]
    /// (§12.5). `None` means no `register_capability` call has claimed this
    /// name, so the capability is **untracked**: the wake layer must
    /// conservatively must-poll it (it cannot prove the component is
    /// unchanged, and a missed wake is the engine-race bug class).
    #[must_use]
    pub fn type_id(&self, name: &str) -> Option<TypeId> {
        self.type_ids.get(name).copied()
    }

    /// The capability names registered so far, in deterministic order.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.names.keys().copied()
    }
}

/// App-builder extension for registering capability names.
pub trait BrinkCapabilityAppExt {
    /// Register component `C` under the engine-vocabulary `name` a manifest
    /// `effects.reads`/`effects.writes` entry may reference, for marker `M`.
    /// Mirrors [`BrinkHandleAppExt::register_handle_kind`](crate::BrinkHandleAppExt::register_handle_kind):
    /// resolves (registering if needed) `C`'s `ComponentId` and indexes it in
    /// [`CapabilityRegistry<M>`] by name.
    fn register_capability<M: Send + Sync + 'static, C: Component>(
        &mut self,
        name: &'static str,
    ) -> &mut Self;
}

impl BrinkCapabilityAppExt for App {
    fn register_capability<M: Send + Sync + 'static, C: Component>(
        &mut self,
        name: &'static str,
    ) -> &mut Self {
        let id = self.world_mut().register_component::<C>();
        self.world_mut()
            .get_resource_or_insert_with(CapabilityRegistry::<M>::default);
        // Index the name (→ ComponentId for the row join, → TypeId for the
        // §12.5 change tracker) and learn whether `C` still needs a
        // change-tracker system wired. `BTreeSet::insert` returns `true` only
        // the first time `C`'s `TypeId` is seen — the idempotency guard.
        let needs_detect_system = {
            let mut registry = self.world_mut().resource_mut::<CapabilityRegistry<M>>();
            registry.names.insert(name, id);
            registry.type_ids.insert(name, TypeId::of::<C>());
            registry.detect_wired.insert(TypeId::of::<C>())
        };
        // The per-frame change-verdict sink the tracker writes and
        // `mark_wake_dirty` reads (§12.5). Idempotent: only the first
        // capability under marker `M` creates it.
        self.init_resource::<CapabilityChanges<M>>();
        if needs_detect_system {
            // BH detect path (#996, `docs/effects-spec.md` §12.5): wire a
            // typed change-tracker for `C` so a component-backed,
            // detect-capable wake condition re-evaluates only when `C`
            // actually changed — not every frame. Ordered before
            // `mark_wake_dirty` (its reader) so a same-frame component change
            // is seen this pass; gated on `any_with_component::<FlowSleep<M>>`
            // exactly like the wake systems, so it costs nothing until a flow
            // sleeps.
            self.add_systems(
                Update,
                detect_capability_changes::<M, C>
                    .before(crate::sleep::mark_wake_dirty::<M>)
                    .run_if(any_with_component::<crate::sleep::FlowSleep<M>>),
            );
        }
        self
    }
}

// ── §12.5: per-capability component-tick tracking (#996) ────────────────────

/// Per-frame, per-capability change verdict — the §12.5 hook BH-4's Detect
/// phase (`crate::sleep::mark_wake_dirty`) consumes so a component-backed
/// **detect-capable** wake condition gets the cheap re-evaluate-on-change
/// path without the missed-wake class.
///
/// Keyed by the concrete component's `TypeId` (see
/// [`CapabilityRegistry::type_id`] for why `TypeId` rather than `ComponentId`).
/// A [`detect_capability_changes`] system — one per registered component,
/// wired by [`BrinkCapabilityAppExt::register_capability`] — overwrites its
/// component's entry every frame with whether any entity carrying that
/// component changed since the tracker last ran (bevy's own `Changed<C>`
/// window). An **absent** entry means no tracker has recorded a verdict for
/// that component yet this run — the wake layer treats that, like an
/// unregistered capability, as a conservative must-poll (never a missed wake).
#[derive(Resource)]
pub struct CapabilityChanges<M: Send + Sync + 'static = ()> {
    /// component `TypeId` → did any entity carrying it change since the
    /// tracker's last run this frame. `BTreeMap` for the determinism rule,
    /// though this map is only ever point-looked-up, never iterated for
    /// output.
    changed: BTreeMap<TypeId, bool>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for CapabilityChanges<M> {
    fn default() -> Self {
        Self {
            changed: BTreeMap::new(),
            _marker: PhantomData,
        }
    }
}

impl<M: Send + Sync + 'static> CapabilityChanges<M> {
    /// This frame's change verdict for a component `TypeId`: `Some(true)` if
    /// an entity carrying it changed since the tracker last ran, `Some(false)`
    /// if it is tracked but unchanged, and `None` if no tracker has recorded a
    /// verdict for it yet (untracked — the wake layer must-polls it).
    #[must_use]
    pub fn changed(&self, ty: TypeId) -> Option<bool> {
        self.changed.get(&ty).copied()
    }
}

/// Typed change-tracker for one registered capability component `C` (§12.5,
/// #996). Wired into `Update` by
/// [`BrinkCapabilityAppExt::register_capability`], one per distinct component,
/// ordered before [`mark_wake_dirty`](crate::sleep::mark_wake_dirty).
///
/// `Query<(), Changed<C>>` rides bevy's own per-table change ticks: on a quiet
/// frame it matches nothing (tables whose change tick didn't advance are
/// skipped wholesale), so `is_empty()` is cheap — far cheaper than the
/// alternative it replaces (re-evaluating every parked flow's wake condition,
/// a full `bind_brink_query` round trip, every single frame). The verdict is
/// written by the component's `TypeId` so `mark_wake_dirty` — which only knows
/// capability *names*, resolved to `TypeId`s through [`CapabilityRegistry`] —
/// can read it back without ever needing the concrete `C`.
pub fn detect_capability_changes<M: Send + Sync + 'static, C: Component>(
    changed: Query<(), Changed<C>>,
    mut sink: ResMut<CapabilityChanges<M>>,
) {
    let any_changed = !changed.is_empty();
    sink.changed.insert(TypeId::of::<C>(), any_changed);
}

// ── The row join ─────────────────────────────────────────────────────────

/// A story's full joined access table: every container's (knot/stitch's)
/// [`DefinitionId`] mapped to its [`ContainerAccess`].
pub type ContainerAccessTable = BTreeMap<DefinitionId, ContainerAccess>;

/// One container's (knot/stitch's) joined ECS access — the output of folding
/// an [`EffectRowEntry`] through the [`CapabilityManifest`] and
/// [`CapabilityRegistry`] (`docs/effects-spec.md` §9).
#[derive(Debug, Clone, Default)]
pub struct ContainerAccess {
    /// The joined access, in bevy's own currency (§12.2: "the row-join output
    /// is the same currency as bevy's `FilteredAccessSet`"). BH-3's parallel
    /// step phase tests flow disjointness with `Access::is_compatible` on
    /// this directly.
    pub access: Access,
    /// Capability names this container reads, sorted — the human-readable
    /// projection of `access`'s read set, for [`dump_container_access`].
    pub reads: Vec<String>,
    /// Capability names this container writes, sorted.
    pub writes: Vec<String>,
    /// Capability name → change-detection-backed bit, **AND-merged** across
    /// every call this container's row (and its dispatch fallbacks) may
    /// perform (`#913`, ruled 2026-07-18): the capability is detect-capable
    /// for this container only if EVERY read of it is detect-capable, so a
    /// single non-detectable read folds the bit to the conservative `false`
    /// (must-poll). BH-4's Detect phase (`crate::sleep`) consumes this to
    /// decide a sleeping flow's re-evaluation cadence.
    pub detect: BTreeMap<String, bool>,
    /// Whether any part of this row hit the pessimal top element
    /// (`docs/effects-spec.md` §3: a call whose effects inference couldn't
    /// summarize). When set, `access` is `read_all`+`write_all` rather than
    /// the joined capability set — conservative-total, never under-report.
    pub opaque: bool,
}

/// Mutable fold state for one container's row join — bundled into a struct
/// (rather than five separate `&mut` parameters) so [`join_direct`] and
/// [`resolve_call_atom`] stay small.
#[derive(Default)]
struct JoinAccumulator {
    access: Access,
    reads: BTreeSet<String>,
    writes: BTreeSet<String>,
    detect: BTreeMap<String, bool>,
    opaque: bool,
}

impl JoinAccumulator {
    fn into_container_access(self) -> ContainerAccess {
        ContainerAccess {
            access: self.access,
            reads: self.reads.into_iter().collect(),
            writes: self.writes.into_iter().collect(),
            detect: self.detect,
            opaque: self.opaque,
        }
    }
}

/// Fold one [`DirectEffects`] row (the entry's direct part, or a dispatch's
/// static fallback) into `acc`. Shared by both call sites in
/// [`compute_container_access`] so dispatch fallbacks join identically to
/// the direct part (`docs/effects-spec.md` §7: v1 does no runtime narrowing,
/// so the fallback always applies — omitting it would under-report access).
fn join_direct<M: Send + Sync + 'static>(
    direct: &DirectEffects,
    program: &Program,
    manifest: &CapabilityManifest,
    registry: &CapabilityRegistry<M>,
    acc: &mut JoinAccumulator,
) -> Result<(), CapabilityError> {
    if direct.opaque {
        acc.opaque = true;
        acc.access.read_all();
        acc.access.write_all();
    }

    for call in &direct.calls {
        resolve_call_atom(call, program, manifest, registry, acc)?;
    }
    Ok(())
}

/// Resolve one call atom's manifest-declared capabilities, if any, folding
/// them into `acc`. A call whose `NameId` doesn't resolve, or that has no
/// manifest entry at all, contributes no access — silently, since not every
/// `EXTERNAL` touches ECS state (§13.2's `effects` key is opt-in).
fn resolve_call_atom<M: Send + Sync + 'static>(
    call: &CallAtom,
    program: &Program,
    manifest: &CapabilityManifest,
    registry: &CapabilityRegistry<M>,
    acc: &mut JoinAccumulator,
) -> Result<(), CapabilityError> {
    let Some(external_name) = program.name_checked(call.name) else {
        return Ok(());
    };
    let Some(external) = manifest.external(external_name) else {
        return Ok(());
    };
    for name in &external.effects.reads {
        let id = resolve_capability(registry, external_name, name)?;
        acc.access.add_read(id);
        acc.reads.insert(name.clone());
    }
    for name in &external.effects.writes {
        let id = resolve_capability(registry, external_name, name)?;
        acc.access.add_write(id);
        acc.writes.insert(name.clone());
    }
    for (name, bit) in &external.effects.detect {
        // #913 (ruled 2026-07-18, decision-log): AND/conservative merge, NOT
        // last-write-wins. A capability is change-detection-backed for this
        // container only if EVERY read of it is detect-capable; two externals
        // touching the same capability with conflicting `detect` bits fold to
        // the conservative `false` (must-poll). A missed wake is the
        // engine-race class; an extra poll is a wasted microsecond (§3
        // soundness direction: over-report, never under-report). BH-4's Detect
        // phase (`crate::sleep`) consumes exactly this merged bit.
        acc.detect
            .entry(name.clone())
            .and_modify(|merged| *merged = *merged && *bit)
            .or_insert(*bit);
    }
    Ok(())
}

fn resolve_capability<M: Send + Sync + 'static>(
    registry: &CapabilityRegistry<M>,
    external_name: &str,
    capability: &str,
) -> Result<ComponentId, CapabilityError> {
    registry
        .component_id(capability)
        .ok_or_else(|| CapabilityError::UnknownCapability {
            external: external_name.to_string(),
            capability: capability.to_string(),
        })
}

/// The row join (`docs/effects-spec.md` §9): compute every container's
/// [`ContainerAccess`] from a story's decoded `EffectRows` table (T2-3/PR
/// #878), joined against `manifest` and `registry`.
///
/// Errors on the first manifest-declared capability name the registry
/// doesn't recognize (the tier-1 admission rule — a clear, load-time
/// failure rather than a silently-incomplete access set).
pub fn compute_container_access<M: Send + Sync + 'static>(
    program: &Program,
    effect_rows: &[EffectRowEntry],
    manifest: &CapabilityManifest,
    registry: &CapabilityRegistry<M>,
) -> Result<ContainerAccessTable, CapabilityError> {
    let mut out = BTreeMap::new();
    for row in effect_rows {
        let mut acc = JoinAccumulator::default();
        join_direct(&row.direct, program, manifest, registry, &mut acc)?;
        // §7: v1 has no narrowing logic on the host side yet, so every
        // dispatch's conservative static fallback always folds in — never
        // conditionally, regardless of its `narrowable` bit.
        for dispatch in &row.dispatches {
            join_direct(&dispatch.fallback, program, manifest, registry, &mut acc)?;
        }
        out.insert(row.def, acc.into_container_access());
    }
    Ok(out)
}

/// Fold one [`DirectEffects`] row's calls into `missing` — the
/// [`missing_capabilities`] counterpart to [`join_direct`], collecting
/// every unregistered name instead of erroring on the first one.
fn collect_missing_from_direct<M: Send + Sync + 'static>(
    direct: &DirectEffects,
    program: &Program,
    manifest: &CapabilityManifest,
    registry: &CapabilityRegistry<M>,
    missing: &mut BTreeSet<(String, String)>,
) {
    for call in &direct.calls {
        let Some(external_name) = program.name_checked(call.name) else {
            continue;
        };
        let Some(external) = manifest.external(external_name) else {
            continue;
        };
        for name in external
            .effects
            .reads
            .iter()
            .chain(external.effects.writes.iter())
        {
            if registry.component_id(name).is_none() {
                missing.insert((external_name.to_string(), name.clone()));
            }
        }
    }
}

/// The load-boundary admission check (issue #912, RULED option (b)): every
/// manifest-declared capability name — from externals this story's effect
/// rows actually call, direct part and every dispatch's static fallback,
/// the same walk [`compute_container_access`] does — that `registry`
/// doesn't recognize. Empty means this story's capabilities all resolve
/// under this marker's registry and the load may proceed.
///
/// Unlike [`compute_container_access`] (which errors on the first miss,
/// suited to the call-time join), this collects every miss so a load
/// rejection ([`CapabilityError::LoadRejected`]) can name the full gap in
/// one shot instead of a fix/reload/fix cycle. Deduplicated and sorted
/// (`BTreeSet`) — CLAUDE.md's determinism rule.
#[must_use]
pub fn missing_capabilities<M: Send + Sync + 'static>(
    program: &Program,
    effect_rows: &[EffectRowEntry],
    manifest: &CapabilityManifest,
    registry: &CapabilityRegistry<M>,
) -> Vec<MissingCapability> {
    let mut missing = BTreeSet::new();
    for row in effect_rows {
        collect_missing_from_direct(&row.direct, program, manifest, registry, &mut missing);
        for dispatch in &row.dispatches {
            collect_missing_from_direct(
                &dispatch.fallback,
                program,
                manifest,
                registry,
                &mut missing,
            );
        }
    }
    missing
        .into_iter()
        .map(|(external, capability)| MissingCapability {
            external,
            capability,
        })
        .collect()
}

/// The shared load-boundary gate (issue #912's admission rule, extended by
/// #997 to every load-shaped entry point): runs [`missing_capabilities`] and,
/// if it finds any gap, builds the [`CapabilityError::LoadRejected`] error
/// naming `marker`, `story`, and every missing capability. `Ok(())` means the
/// story's capabilities all resolve under this marker's registry and the
/// construction may proceed.
///
/// **Every** story-construction path that builds a `FlowInstance` from a
/// `ProgramAsset` must call this before doing so — not just the initial
/// `fulfill_flow_requests` load. PR #989 (closing #912) only wired this into
/// that one path; #997 found the dev-only hot-reload reconstruction in
/// `crate::replay::replay_on_reload` builds a fresh `FlowInstance` against
/// the (possibly changed) reloaded program without re-running the check,
/// letting a story that lost a manifest-required capability across a reload
/// slip past the hard-error boundary. Both call sites now share this one
/// function so a third load-shaped path can't repeat the gap.
pub fn check_load_capability_gate<M: Send + Sync + 'static>(
    program: &Program,
    effect_rows: &[EffectRowEntry],
    manifest: &CapabilityManifest,
    registry: &CapabilityRegistry<M>,
    story: String,
) -> Result<(), CapabilityError> {
    let missing = missing_capabilities(program, effect_rows, manifest, registry);
    if missing.is_empty() {
        Ok(())
    } else {
        Err(CapabilityError::LoadRejected {
            marker: std::any::type_name::<M>(),
            story,
            missing,
        })
    }
}

// ── Load/unload boundary ──────────────────────────────────────────────────

/// Per-story table of joined [`ContainerAccess`], keyed by the loaded
/// [`ProgramAsset`]'s `AssetId` (a `bevy-brink` app may have several stories
/// loaded — under one marker or several — at once). Rebuilt by
/// [`rebuild_capability_table`] at the story load/unload boundary (§12.5's
/// ruled invariant — "story load/unload is when the params rebuild").
#[derive(Resource)]
pub struct CapabilityTable<M: Send + Sync + 'static = ()> {
    per_story: BTreeMap<AssetId<ProgramAsset>, Result<ContainerAccessTable, CapabilityError>>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for CapabilityTable<M> {
    fn default() -> Self {
        Self {
            per_story: BTreeMap::new(),
            _marker: PhantomData,
        }
    }
}

impl<M: Send + Sync + 'static> CapabilityTable<M> {
    /// The join result for a loaded story, if any story with this asset id
    /// has been processed yet. `Some(Err(_))` means the join failed (an
    /// unregistered capability) — the story loaded, but has no usable access
    /// table.
    #[must_use]
    pub fn get(
        &self,
        id: AssetId<ProgramAsset>,
    ) -> Option<&Result<ContainerAccessTable, CapabilityError>> {
        self.per_story.get(&id)
    }

    /// The joined access table for a loaded story, if the join succeeded.
    #[must_use]
    pub fn access_for(&self, id: AssetId<ProgramAsset>) -> Option<&ContainerAccessTable> {
        self.per_story.get(&id)?.as_ref().ok()
    }

    /// Test-only constructor bypassing the load/unload boundary system
    /// (`rebuild_capability_table`) — lets a unit test (`crate::ground_truth`'s,
    /// the only caller) exercise [`CapabilityTable::access_for`] against a
    /// hand-built join result without spinning up a full `App`/`ProgramAsset`
    /// load cycle. Gated on `effect-trace` too (not just `test`) since that's
    /// the only feature combination that compiles a caller.
    #[cfg(all(test, feature = "effect-trace"))]
    pub(crate) fn insert_for_test(
        &mut self,
        id: AssetId<ProgramAsset>,
        result: Result<ContainerAccessTable, CapabilityError>,
    ) {
        self.per_story.insert(id, result);
    }
}

/// Plugin-managed system: rebuild a loaded story's [`ContainerAccess`] table
/// whenever its [`ProgramAsset`] (re)loads, and drop it when the asset
/// unloads — the load/unload boundary §12.5 rules access sets rebuild at.
///
/// A failed join (an unregistered capability name) is logged loudly and
/// recorded as `Err` in the table rather than left stale or silently
/// dropped — the caller can inspect [`CapabilityTable::get`] directly
/// instead of scraping logs (this is what BH-1's headless tests do).
#[expect(
    clippy::needless_pass_by_value,
    reason = "bevy systems take Res/ResMut/MessageReader by value"
)]
pub fn rebuild_capability_table<M: Send + Sync + 'static>(
    mut events: MessageReader<AssetEvent<ProgramAsset>>,
    programs: Res<Assets<ProgramAsset>>,
    manifest: Res<CapabilityManifest>,
    registry: Res<CapabilityRegistry<M>>,
    mut table: ResMut<CapabilityTable<M>>,
) {
    for event in events.read() {
        match event {
            AssetEvent::Added { id }
            | AssetEvent::Modified { id }
            | AssetEvent::LoadedWithDependencies { id } => {
                let Some(asset) = programs.get(*id) else {
                    continue;
                };
                let result = compute_container_access(
                    &asset.program,
                    &asset.effect_rows,
                    &manifest,
                    &registry,
                );
                if let Err(err) = &result {
                    error!("brink capability join failed for a loaded story: {err}");
                }
                table.per_story.insert(*id, result);
            }
            AssetEvent::Removed { id } | AssetEvent::Unused { id } => {
                table.per_story.remove(id);
            }
        }
    }
}

/// Render a human-readable `container -> access set` table (BH-B's scenario
/// harness + interactive debugging, per this issue's "dev-visible dump"
/// deliverable). Deterministic: the input is keyed by `DefinitionId`
/// (`BTreeMap` order) and each container's name lists are pre-sorted.
#[must_use]
pub fn dump_container_access(program: &Program, table: &ContainerAccessTable) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    for (def, access) in table {
        let label = program
            .divert_target_path(*def)
            .unwrap_or_else(|| format!("<{def}>"));
        let opaque_tag = if access.opaque {
            " OPAQUE(read_all+write_all)"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "{label}: reads=[{}] writes=[{}]{opaque_tag}",
            access.reads.join(", "),
            access.writes.join(", "),
        );
        for (name, detect_bit) in &access.detect {
            let _ = writeln!(out, "    detect[{name}] = {detect_bit}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use bevy_ecs::component::Component;
    use brink_format::{
        CallAtom, CapabilityParam, DefinitionId, DefinitionTag, DirectEffects, DispatchEntry,
        EffectRowEntry,
    };

    use super::*;
    use crate::test_support::compile_test_story;

    #[derive(Component)]
    struct Transform;

    #[derive(Component)]
    struct AudioSink;

    fn atom(name: brink_format::NameId) -> CallAtom {
        CallAtom {
            name,
            capability: CapabilityParam::Any,
            handle_param: None,
        }
    }

    #[test]
    fn manifest_round_trips_the_13_2_grammar() {
        let json = r#"
        {
            "externals": [
                {
                    "name": "get_position",
                    "params": [{"name": "npc", "ty": "Handle<Npc>"}],
                    "effects": {
                        "reads": ["Transform"],
                        "detect": {"Transform": true}
                    }
                }
            ]
        }
        "#;
        let manifest = CapabilityManifest::from_json(json).expect("valid manifest json");
        assert_eq!(manifest.externals.len(), 1);
        let ext = &manifest.externals[0];
        assert_eq!(ext.name, "get_position");
        assert_eq!(ext.effects.reads, vec!["Transform".to_string()]);
        assert!(ext.effects.writes.is_empty());
        assert_eq!(ext.effects.detect.get("Transform"), Some(&true));

        let serialized = serde_json::to_string(&manifest).expect("serialize back to json");
        let round_tripped =
            CapabilityManifest::from_json(&serialized).expect("re-parse the serialized manifest");
        assert_eq!(manifest, round_tripped);
    }

    #[test]
    fn manifest_json_ignores_unknown_fields() {
        // The same manifest file also carries `params`/`kind`/`doc`/`widgets`/
        // `path` for the compiler/IDE side (brink_ir::host_manifest) — this
        // parse must not choke on any of it.
        let json = r#"
        {
            "externals": [
                {"name": "play_sfx", "kind": "effect", "doc": "plays a sound", "path": ["Audio"]}
            ]
        }
        "#;
        let manifest = CapabilityManifest::from_json(json).expect("unknown fields are ignored");
        assert_eq!(manifest.externals[0].name, "play_sfx");
        assert_eq!(manifest.externals[0].effects, CapabilityEffects::default());
    }

    #[test]
    fn malformed_manifest_json_is_an_error() {
        let err = CapabilityManifest::from_json("not json").unwrap_err();
        assert!(matches!(err, CapabilityError::ManifestJson(_)));
    }

    #[test]
    fn register_capability_indexes_component_id_by_name() {
        let mut app = App::new();
        app.register_capability::<(), Transform>("Transform");
        app.register_capability::<(), AudioSink>("AudioSink");

        let registry = app.world().resource::<CapabilityRegistry<()>>();
        assert!(registry.component_id("Transform").is_some());
        assert!(registry.component_id("AudioSink").is_some());
        assert_eq!(
            registry.component_id("Transform"),
            registry.component_id("Transform")
        );
        assert_ne!(
            registry.component_id("Transform"),
            registry.component_id("AudioSink")
        );
        assert_eq!(
            registry.names().collect::<Vec<_>>(),
            vec!["AudioSink", "Transform"]
        );
    }

    #[test]
    fn unknown_capability_name_is_a_load_time_error() {
        let mut app = App::new();
        app.register_capability::<(), Transform>("Transform");
        let registry = app.world().resource::<CapabilityRegistry<()>>();

        let mut manifest = CapabilityManifest::default();
        manifest.externals.push(CapabilityManifestExternal {
            name: "get_position".to_string(),
            effects: CapabilityEffects {
                reads: vec!["Nonexistent".to_string()],
                writes: vec![],
                detect: BTreeMap::new(),
            },
        });

        let source = "EXTERNAL get_position(id)\n=== start ===\n~ temp x = get_position(0)\nHello.\n-> END\n";
        let (program, _tables, _ctx) = compile_test_story(source);
        let name_id = program
            .name_id("get_position")
            .expect("interned as a call kind");

        let row = EffectRowEntry {
            def: DefinitionId::new(DefinitionTag::Address, 0),
            is_entry: true,
            direct: DirectEffects {
                reads: vec![],
                writes: vec![],
                calls: vec![atom(name_id)],
                opaque: false,
                emits: false,
                tags: false,
                faults: false,
            },
            dispatches: vec![],
        };

        let err = compute_container_access(&program, &[row], &manifest, registry).unwrap_err();
        assert!(matches!(
            &err,
            CapabilityError::UnknownCapability { external, capability }
                if external == "get_position" && capability == "Nonexistent"
        ));
    }

    #[test]
    fn known_capability_joins_into_component_access_and_names() {
        let mut app = App::new();
        app.register_capability::<(), Transform>("Transform");
        app.register_capability::<(), AudioSink>("AudioSink");
        let registry = app.world().resource::<CapabilityRegistry<()>>();
        let transform_id = registry.component_id("Transform").expect("registered");
        let audio_id = registry.component_id("AudioSink").expect("registered");

        let mut manifest = CapabilityManifest::default();
        manifest.externals.push(CapabilityManifestExternal {
            name: "get_position".to_string(),
            effects: CapabilityEffects {
                reads: vec!["Transform".to_string()],
                writes: vec![],
                detect: [("Transform".to_string(), true)].into_iter().collect(),
            },
        });
        manifest.externals.push(CapabilityManifestExternal {
            name: "play_sfx".to_string(),
            effects: CapabilityEffects {
                reads: vec![],
                writes: vec!["AudioSink".to_string()],
                detect: BTreeMap::new(),
            },
        });

        let source = "EXTERNAL get_position(id)\nEXTERNAL play_sfx(id)\n=== start ===\n~ temp x = get_position(0)\n~ play_sfx(0)\nHello.\n-> END\n";
        let (program, _tables, _ctx) = compile_test_story(source);
        let get_position = program.name_id("get_position").expect("interned");
        let play_sfx = program.name_id("play_sfx").expect("interned");

        let row = EffectRowEntry {
            def: DefinitionId::new(DefinitionTag::Address, 0),
            is_entry: true,
            direct: DirectEffects {
                reads: vec![],
                writes: vec![],
                calls: vec![atom(get_position), atom(play_sfx)],
                opaque: false,
                emits: false,
                tags: false,
                faults: false,
            },
            dispatches: vec![],
        };

        let table =
            compute_container_access(&program, std::slice::from_ref(&row), &manifest, registry)
                .expect("join succeeds");
        let access = table.get(&row.def).expect("row's container present");
        assert!(access.access.has_read(transform_id));
        assert!(!access.access.has_write(transform_id));
        assert!(access.access.has_write(audio_id));
        assert_eq!(access.reads, vec!["Transform".to_string()]);
        assert_eq!(access.writes, vec!["AudioSink".to_string()]);
        assert_eq!(access.detect.get("Transform"), Some(&true));
        assert!(!access.opaque);
    }

    /// #913 (ruled 2026-07-18): when one container calls two externals that
    /// touch the **same** capability with **conflicting** `detect` bits, the
    /// folded bit is the AND (conservative `false` / must-poll) — never the
    /// accidental last-write-wins that `BTreeMap::insert` used to give. Order
    /// must not matter: `true`-then-`false` and `false`-then-`true` both fold
    /// to `false`.
    #[test]
    fn conflicting_detect_bits_fold_conservative_and_not_last_write_wins() {
        let mut app = App::new();
        app.register_capability::<(), Transform>("Transform");
        let registry = app.world().resource::<CapabilityRegistry<()>>();

        // Two externals, both reading `Transform`, one detect-capable (true),
        // one opaque (false). A container that calls both must classify
        // `Transform` as must-poll (false) regardless of manifest order.
        let mut manifest = CapabilityManifest::default();
        manifest.externals.push(CapabilityManifestExternal {
            name: "watch_pos".to_string(),
            effects: CapabilityEffects {
                reads: vec!["Transform".to_string()],
                writes: vec![],
                detect: [("Transform".to_string(), true)].into_iter().collect(),
            },
        });
        manifest.externals.push(CapabilityManifestExternal {
            name: "poke_pos".to_string(),
            effects: CapabilityEffects {
                reads: vec!["Transform".to_string()],
                writes: vec![],
                detect: [("Transform".to_string(), false)].into_iter().collect(),
            },
        });

        let source = "EXTERNAL watch_pos(id)\nEXTERNAL poke_pos(id)\n=== start ===\n~ temp x = watch_pos(0)\n~ temp y = poke_pos(0)\nHi.\n-> END\n";
        let (program, _tables, _ctx) = compile_test_story(source);
        let watch = program.name_id("watch_pos").expect("interned");
        let poke = program.name_id("poke_pos").expect("interned");

        // Order A: watch (true) then poke (false).
        let row_a = EffectRowEntry {
            def: DefinitionId::new(DefinitionTag::Address, 0),
            is_entry: true,
            direct: DirectEffects {
                reads: vec![],
                writes: vec![],
                calls: vec![atom(watch), atom(poke)],
                opaque: false,
                emits: false,
                tags: false,
                faults: false,
            },
            dispatches: vec![],
        };
        let table_a =
            compute_container_access(&program, std::slice::from_ref(&row_a), &manifest, registry)
                .expect("join succeeds");
        assert_eq!(
            table_a[&row_a.def].detect.get("Transform"),
            Some(&false),
            "true-then-false must AND to false (must-poll), not last-write-wins to false-by-luck"
        );

        // Order B: poke (false) then watch (true) — last write is `true`; a
        // last-write-wins fold would wrongly yield `true`. AND still gives false.
        let row_b = EffectRowEntry {
            def: DefinitionId::new(DefinitionTag::Address, 0),
            is_entry: true,
            direct: DirectEffects {
                reads: vec![],
                writes: vec![],
                calls: vec![atom(poke), atom(watch)],
                opaque: false,
                emits: false,
                tags: false,
                faults: false,
            },
            dispatches: vec![],
        };
        let table_b =
            compute_container_access(&program, std::slice::from_ref(&row_b), &manifest, registry)
                .expect("join succeeds");
        assert_eq!(
            table_b[&row_b.def].detect.get("Transform"),
            Some(&false),
            "false-then-true must AND to false — the regression #913 fixes: \
             last-write-wins would have left it `true` and risked a missed wake"
        );
    }

    /// Two detect-capable reads of the same capability keep the bit `true`
    /// (AND of `true`s) — the merge is conservative, not blindly pessimistic.
    #[test]
    fn all_detect_capable_reads_keep_the_bit_true() {
        let mut app = App::new();
        app.register_capability::<(), Transform>("Transform");
        let registry = app.world().resource::<CapabilityRegistry<()>>();

        let mut manifest = CapabilityManifest::default();
        for ext in ["watch_a", "watch_b"] {
            manifest.externals.push(CapabilityManifestExternal {
                name: ext.to_string(),
                effects: CapabilityEffects {
                    reads: vec!["Transform".to_string()],
                    writes: vec![],
                    detect: [("Transform".to_string(), true)].into_iter().collect(),
                },
            });
        }
        let source = "EXTERNAL watch_a(id)\nEXTERNAL watch_b(id)\n=== start ===\n~ temp x = watch_a(0)\n~ temp y = watch_b(0)\nHi.\n-> END\n";
        let (program, _tables, _ctx) = compile_test_story(source);
        let a = program.name_id("watch_a").expect("interned");
        let b = program.name_id("watch_b").expect("interned");
        let row = EffectRowEntry {
            def: DefinitionId::new(DefinitionTag::Address, 0),
            is_entry: true,
            direct: DirectEffects {
                reads: vec![],
                writes: vec![],
                calls: vec![atom(a), atom(b)],
                opaque: false,
                emits: false,
                tags: false,
                faults: false,
            },
            dispatches: vec![],
        };
        let table =
            compute_container_access(&program, std::slice::from_ref(&row), &manifest, registry)
                .expect("join succeeds");
        assert_eq!(table[&row.def].detect.get("Transform"), Some(&true));
    }

    #[test]
    fn opaque_row_reads_and_writes_everything() {
        let registry = CapabilityRegistry::<()>::default();
        let manifest = CapabilityManifest::default();
        let program_source = "=== start ===\nHello.\n-> END\n";
        let (program, _tables, _ctx) = compile_test_story(program_source);

        let row = EffectRowEntry {
            def: DefinitionId::new(DefinitionTag::Address, 0),
            is_entry: true,
            direct: DirectEffects {
                reads: vec![],
                writes: vec![],
                calls: vec![],
                opaque: true,
                emits: false,
                tags: false,
                faults: false,
            },
            dispatches: vec![],
        };

        let table =
            compute_container_access(&program, std::slice::from_ref(&row), &manifest, &registry)
                .expect("join succeeds");
        let access = table.get(&row.def).expect("row's container present");
        assert!(access.opaque);
        assert!(access.access.has_read_all());
        assert!(access.access.has_write_all());
    }

    #[test]
    fn dispatch_fallback_rows_always_fold_in() {
        // §7: v1 has no narrowing, so a populated dispatch's fallback must
        // join exactly like the direct part — even though it's marked
        // `narrowable`, since no host-side narrowing exists yet to act on
        // that bit. Omitting it would under-report access.
        let mut app = App::new();
        app.register_capability::<(), Transform>("Transform");
        let registry = app.world().resource::<CapabilityRegistry<()>>();
        let transform_id = registry.component_id("Transform").expect("registered");

        let mut manifest = CapabilityManifest::default();
        manifest.externals.push(CapabilityManifestExternal {
            name: "get_position".to_string(),
            effects: CapabilityEffects {
                reads: vec!["Transform".to_string()],
                writes: vec![],
                detect: BTreeMap::new(),
            },
        });

        let source = "EXTERNAL get_position(id)\n=== start ===\n~ temp x = get_position(0)\nHello.\n-> END\n";
        let (program, _tables, _ctx) = compile_test_story(source);
        let get_position = program.name_id("get_position").expect("interned");

        let row = EffectRowEntry {
            def: DefinitionId::new(DefinitionTag::Address, 0),
            is_entry: true,
            direct: DirectEffects::default(),
            dispatches: vec![DispatchEntry {
                cell: DefinitionId::new(DefinitionTag::Address, 1),
                narrowable: true,
                fallback: DirectEffects {
                    reads: vec![],
                    writes: vec![],
                    calls: vec![atom(get_position)],
                    opaque: false,
                    emits: false,
                    tags: false,
                    faults: false,
                },
            }],
        };

        let table =
            compute_container_access(&program, std::slice::from_ref(&row), &manifest, registry)
                .expect("join succeeds");
        let access = table.get(&row.def).expect("row's container present");
        assert!(access.access.has_read(transform_id));
        assert_eq!(access.reads, vec!["Transform".to_string()]);
    }

    #[test]
    fn dump_renders_names_and_detect_bits_deterministically() {
        let mut table: ContainerAccessTable = BTreeMap::new();
        let access = ContainerAccess {
            reads: vec!["Transform".to_string()],
            writes: vec!["AudioSink".to_string()],
            detect: [("Transform".to_string(), true)].into_iter().collect(),
            ..ContainerAccess::default()
        };
        table.insert(DefinitionId::new(DefinitionTag::Address, 0), access);

        let source = "=== start ===\nHello.\n-> END\n";
        let (program, _tables, _ctx) = compile_test_story(source);
        let rendered = dump_container_access(&program, &table);
        assert!(rendered.contains("reads=[Transform]"));
        assert!(rendered.contains("writes=[AudioSink]"));
        assert!(rendered.contains("detect[Transform] = true"));
    }

    /// Reachability proof (this issue's own gate): a real app that only
    /// calls the public surface — `add_plugins(BrinkPlugin::default())`,
    /// `register_capability`, `insert_resource(CapabilityManifest)` — gets
    /// its loaded story's `CapabilityTable` populated automatically, with no
    /// manual call to `compute_container_access` anywhere. This is the exact
    /// path a host app takes: the join runs off `BrinkPlugin`'s own
    /// `rebuild_capability_table` system reacting to the `ProgramAsset`'s
    /// `AssetEvent::Added`, not a test-only hook.
    #[test]
    fn wired_via_brink_plugin_rebuilds_capability_table_on_story_load() {
        let mut app = crate::test_support::make_test_app();
        app.register_capability::<(), Transform>("Transform");

        let mut manifest = CapabilityManifest::default();
        manifest.externals.push(CapabilityManifestExternal {
            name: "get_position".to_string(),
            effects: CapabilityEffects {
                reads: vec!["Transform".to_string()],
                writes: vec![],
                detect: BTreeMap::new(),
            },
        });
        app.insert_resource(manifest);

        let source = "EXTERNAL get_position(id)\n=== start ===\n~ temp x = get_position(0)\nHello.\n-> END\n";
        let out = brink_compiler::compile("t.ink", move |p| {
            if p == "t.ink" {
                Ok(source.to_string())
            } else {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "x"))
            }
        })
        .expect("compile");
        let mut inkb = Vec::new();
        brink_format::write_inkb(&out.data, &mut inkb);
        let loaded = brink_format::read_inkb(&inkb).expect("read_inkb");
        let (program, _tables) = brink_runtime::link(&loaded).expect("link");
        let (_, initial_context) = brink_runtime::FlowInstance::new_at_root(&program);

        // Kept alive for the whole test (a dropped strong Handle queues its
        // own AssetEvent::Removed via `Assets::track_assets`, which would
        // race the Added event this test actually wants to observe).
        let program_handle =
            app.world_mut()
                .resource_mut::<Assets<ProgramAsset>>()
                .add(ProgramAsset {
                    program,
                    initial_context,
                    effect_rows: loaded.effect_rows,
                });
        let program_id = program_handle.id();

        // BrinkPlugin<()>'s rebuild_capability_table system reacts to the
        // AssetEvent::Added the `add` above queued. Bevy's asset events flush
        // `queued_events` into the readable `Messages<AssetEvent<_>>` buffer
        // in `PostUpdate` — after our system's own `Update` stage — so the
        // event isn't visible to a `MessageReader` until the *following*
        // tick; two updates are the correct wait, not a workaround.
        app.update();
        app.update();

        let table = app.world().resource::<CapabilityTable<()>>();
        let access_table = table
            .access_for(program_id)
            .expect("capability join ran for the loaded story off the plugin's own system");
        // Harden past non-emptiness (issue #911, BH follow-up deliverable
        // 2): this story has exactly one container (`start`), which calls
        // `get_position` — assert the *actual* joined set the plugin's own
        // system produced (reads == [Transform], no writes, not opaque),
        // not merely that the table has *some* entry in it.
        assert_eq!(
            access_table.len(),
            1,
            "expected exactly one container row (the story's single `start` knot): {access_table:?}"
        );
        let access = access_table
            .values()
            .next()
            .expect("checked len() == 1 above");
        let transform_id = app
            .world()
            .resource::<CapabilityRegistry<()>>()
            .component_id("Transform")
            .expect("Transform was registered above");
        assert_eq!(
            access.reads,
            vec!["Transform".to_string()],
            "joined reads should be exactly what get_position's manifest entry declares"
        );
        assert!(
            access.writes.is_empty(),
            "get_position's manifest entry declares no writes"
        );
        assert!(
            access.access.has_read(transform_id),
            "the joined bevy Access should carry a read on Transform's ComponentId"
        );
        assert!(!access.access.has_write(transform_id));
        assert!(
            !access.opaque,
            "no call in this story hits the opaque fallback"
        );
        drop(program_handle);
    }

    /// The unload half of the load/unload boundary invariant (§12.5): once
    /// the `ProgramAsset` is dropped from `Assets`, the next tick's
    /// `AssetEvent::Removed` clears the story's entry out of the table.
    #[test]
    fn unloading_a_story_drops_its_capability_table_entry() {
        let mut app = crate::test_support::make_test_app();
        app.insert_resource(CapabilityManifest::default());

        let source = "=== start ===\nHello.\n-> END\n";
        let (program, _tables, initial_context) = compile_test_story(source);
        // Kept alive until the deliberate `remove` below — see the note on
        // the sibling reachability test about handle-drop races.
        let program_handle =
            app.world_mut()
                .resource_mut::<Assets<ProgramAsset>>()
                .add(ProgramAsset {
                    program,
                    initial_context,
                    effect_rows: vec![],
                });
        let program_id = program_handle.id();
        app.update();
        app.update(); // see the sibling test's note: events flush one tick late
        assert!(
            app.world()
                .resource::<CapabilityTable<()>>()
                .get(program_id)
                .is_some()
        );

        app.world_mut()
            .resource_mut::<Assets<ProgramAsset>>()
            .remove(program_id);
        app.update();
        app.update();
        assert!(
            app.world()
                .resource::<CapabilityTable<()>>()
                .get(program_id)
                .is_none()
        );
        drop(program_handle);
    }

    // ── Issue #912: load-boundary admission check ──────────────────────

    /// [`missing_capabilities`] must collect **every** manifest-declared
    /// capability the registry doesn't recognize — not just the first,
    /// unlike [`compute_container_access`]'s call-time short-circuit —
    /// since the whole point of a load-boundary error is to show the host
    /// the full gap in one shot.
    #[test]
    fn missing_capabilities_collects_every_gap_not_just_the_first() {
        let registry = CapabilityRegistry::<()>::default(); // nothing registered at all

        let mut manifest = CapabilityManifest::default();
        manifest.externals.push(CapabilityManifestExternal {
            name: "get_position".to_string(),
            effects: CapabilityEffects {
                reads: vec!["Transform".to_string()],
                writes: vec![],
                detect: BTreeMap::new(),
            },
        });
        manifest.externals.push(CapabilityManifestExternal {
            name: "play_sfx".to_string(),
            effects: CapabilityEffects {
                reads: vec![],
                writes: vec!["AudioSink".to_string()],
                detect: BTreeMap::new(),
            },
        });

        let source = "EXTERNAL get_position(id)\nEXTERNAL play_sfx(id)\n\
                       === start ===\n~ temp x = get_position(0)\n~ play_sfx(0)\nHello.\n-> END\n";
        let (program, _tables, _ctx) = compile_test_story(source);
        let get_position = program.name_id("get_position").expect("interned");
        let play_sfx = program.name_id("play_sfx").expect("interned");

        let row = EffectRowEntry {
            def: DefinitionId::new(DefinitionTag::Address, 0),
            is_entry: true,
            direct: DirectEffects {
                reads: vec![],
                writes: vec![],
                calls: vec![atom(get_position), atom(play_sfx)],
                opaque: false,
                emits: false,
                tags: false,
                faults: false,
            },
            dispatches: vec![],
        };

        let missing = missing_capabilities(&program, &[row], &manifest, &registry);
        assert_eq!(
            missing.len(),
            2,
            "both externals' missing capabilities should be reported: {missing:?}"
        );
        assert!(missing.contains(&MissingCapability {
            external: "get_position".to_string(),
            capability: "Transform".to_string(),
        }));
        assert!(missing.contains(&MissingCapability {
            external: "play_sfx".to_string(),
            capability: "AudioSink".to_string(),
        }));
    }

    /// A capability the registry *does* recognize contributes nothing to
    /// `missing_capabilities` — the happy path a single-satisfied marker
    /// takes at load time.
    #[test]
    fn missing_capabilities_is_empty_when_registry_covers_every_declared_name() {
        let mut app = App::new();
        app.register_capability::<(), Transform>("Transform");
        let registry = app.world().resource::<CapabilityRegistry<()>>();

        let mut manifest = CapabilityManifest::default();
        manifest.externals.push(CapabilityManifestExternal {
            name: "get_position".to_string(),
            effects: CapabilityEffects {
                reads: vec!["Transform".to_string()],
                writes: vec![],
                detect: BTreeMap::new(),
            },
        });

        let source = "EXTERNAL get_position(id)\n=== start ===\n~ temp x = get_position(0)\nHello.\n-> END\n";
        let (program, _tables, _ctx) = compile_test_story(source);
        let get_position = program.name_id("get_position").expect("interned");

        let row = EffectRowEntry {
            def: DefinitionId::new(DefinitionTag::Address, 0),
            is_entry: true,
            direct: DirectEffects {
                reads: vec![],
                writes: vec![],
                calls: vec![atom(get_position)],
                opaque: false,
                emits: false,
                tags: false,
                faults: false,
            },
            dispatches: vec![],
        };

        let missing = missing_capabilities(&program, &[row], &manifest, registry);
        assert!(missing.is_empty(), "got {missing:?}");
    }

    /// The load-rejection error (this issue's user-facing surface) must
    /// name the marker, the story, and every missing capability — not a
    /// generic "something's missing" message the host has to go dig for.
    #[test]
    fn load_rejected_error_names_marker_story_and_every_missing_capability() {
        let err = CapabilityError::LoadRejected {
            marker: "my_game::DreamSequence",
            story: "dialogue.ink".to_string(),
            missing: vec![
                MissingCapability {
                    external: "get_position".to_string(),
                    capability: "Transform".to_string(),
                },
                MissingCapability {
                    external: "play_sfx".to_string(),
                    capability: "AudioSink".to_string(),
                },
            ],
        };
        let message = err.to_string();
        assert!(
            message.contains("my_game::DreamSequence"),
            "should name the marker: {message}"
        );
        assert!(
            message.contains("dialogue.ink"),
            "should name the story: {message}"
        );
        assert!(
            message.contains("Transform") && message.contains("get_position"),
            "should name the first missing capability and its external: {message}"
        );
        assert!(
            message.contains("AudioSink") && message.contains("play_sfx"),
            "should name the second missing capability and its external too: {message}"
        );
    }
}
