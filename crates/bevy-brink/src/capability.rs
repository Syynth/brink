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

use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;

use bevy_app::App;
use bevy_asset::{AssetEvent, AssetId, Assets};
use bevy_ecs::component::{Component, ComponentId};
use bevy_ecs::message::MessageReader;
use bevy_ecs::query::Access;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Res, ResMut};
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
    /// capability; `false` (or absent) means it must be polled. Captured
    /// here but not consumed until BH-4.
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
}

// ── Registry: name → ComponentId (mirrors HandleKind's registration) ────

/// App-level registry mapping capability names to `ComponentId`s, keyed by
/// marker `M` (mirrors [`HandleKinds<M>`](crate::HandleKinds)). Populated via
/// [`BrinkCapabilityAppExt::register_capability`]. `BTreeMap` for
/// deterministic iteration (CLAUDE.md determinism rule).
#[derive(Resource)]
pub struct CapabilityRegistry<M: Send + Sync + 'static = ()> {
    names: BTreeMap<&'static str, ComponentId>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for CapabilityRegistry<M> {
    fn default() -> Self {
        Self {
            names: BTreeMap::new(),
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
        self.world_mut()
            .resource_mut::<CapabilityRegistry<M>>()
            .names
            .insert(name, id);
        self
    }
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
    /// Capability name → change-detection-backed bit, unioned from every
    /// call this container's row (and its dispatch fallbacks) may perform.
    /// Diagnostic surface for BH-4; BH-1 only captures it.
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
        acc.detect.insert(name.clone(), *bit);
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
                    "params": [{"type": "handle<Npc>"}],
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
}
