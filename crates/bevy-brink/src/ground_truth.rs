//! Host-side ground-truth check (issue #938, tracked from #897) — the
//! `brink_runtime::effect_trace` pattern applied to the bevy boundary.
//!
//! `brink_runtime::effect_trace` closes the compiler's #870 gap: a purely
//! structural row check can't catch a static effects row that under-reports
//! what the bytecode *actually* does, because both the caller's and the
//! callee's rows can silently agree on the wrong (too-small) answer. The
//! independent fix is to run the real bytecode and record what it actually
//! touches, then assert the static row covers it.
//!
//! The bevy host boundary has the exact same shape, one layer up: BH-1
//! (`crate::capability`) computes a *declared* per-story [`Access`] by
//! joining the compiler's static effect rows against a **host-authored**
//! capability manifest (`crate::capability::CapabilityManifest`). That join
//! is only as good as the manifest — if a `bind_brink_query` binding's real
//! Bevy system touches a component its manifest entry never lists in
//! `effects.reads`/`effects.writes`, BH-1's `Access` silently under-reports,
//! and BH-3's parallel Step phase's disjointness argument
//! (`crate::batch::parallel`, decision-log 2026-07-16) is built on exactly
//! that `Access`. A purely-manifest-side check can't catch this (both the
//! manifest and the join can silently agree on the wrong answer); this
//! module is the independent, run-the-real-binding-and-look check that
//! closes that gap, mirroring `brink_runtime::effect_trace`'s "ground truth"
//! role for the compiler side.
//!
//! Feature-gated exactly like `effect-trace` on `brink-runtime`
//! (`crates/brink-runtime/src/effect_trace.rs`): this module and every call
//! site are compiled out entirely unless `bevy-brink`'s own `effect-trace`
//! feature is enabled (off by default — not a released consumer's concern),
//! so an ordinary build pays exactly zero cost.
//!
//! ## What "actual component access" means here
//!
//! Bevy's own query/system access is *static*: a `Query<&Transform>`
//! declares exactly the same [`Access`] whether or not it ever iterates a
//! matching entity. So the ground truth doesn't need to be captured mid-run
//! by instrumenting opcodes (unlike the compiler's `effect_trace`, which
//! really does vary by which branch bytecode takes) — it can be captured
//! once, precisely, the moment [`crate::BrinkBindingsAppExt::bind_brink_query`]
//! registers the binding's system (`bindings.rs`'s `bind_brink_query`, via
//! [`System::initialize`](bevy_ecs::system::System::initialize)), which is
//! bevy's own ground truth for what that system can touch. "Instrumenting
//! the Step phase" then means: every time a query binding is **actually
//! dispatched** (`bindings.rs`'s `dispatch_one_external`, the one safe
//! access layer a real `bind_brink_query` invocation flows through — both
//! for the serial API and for a batch turn's parked-external resolution),
//! [`record`] logs that dispatch's (flow, binding, story, captured access)
//! tuple; [`check`] then asserts every logged access is a subset of BH-1's
//! declared row-join for that story.
//!
//! No `unsafe` — this wraps the existing safe `world.run_system_with` call
//! site; the sanctioned-unsafe module (`crate::batch::parallel`) is
//! untouched and does not grow.

use std::marker::PhantomData;

use bevy_asset::AssetId;
use bevy_ecs::component::Components;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::{Access, ComponentAccessKind};
use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;

use crate::asset::{BrinkProgram, ProgramAsset};
use crate::batch::aggregate_access;
use crate::capability::CapabilityTable;

/// One real `bind_brink_query` dispatch, observed at the exact point bevy
/// actually ran it. The runtime counterpart of
/// `brink_runtime::effect_trace::ObservedRow` — this module never
/// constructs an opaque/approximate access; every entry is the concrete
/// [`Access`] bevy's own `System::initialize` reported for the bound
/// system.
#[derive(Debug, Clone)]
pub struct ObservedAccess {
    /// The flow entity whose external call dispatched this binding.
    pub flow: Entity,
    /// The flow's story — [`CapabilityTable::access_for`] looks up its BH-1
    /// declared access under this key.
    pub story: AssetId<ProgramAsset>,
    /// The `bind_brink_query` binding name that was dispatched.
    pub binding: String,
    /// The binding's real, bevy-declared [`Access`] (captured once at
    /// registration time — see the module docs' "what actual component
    /// access means here").
    pub access: Access,
}

/// Log of every query-binding dispatch observed so far under marker `M`.
/// Populated by [`record`] (called from `bindings.rs`'s
/// `dispatch_one_external`); drained/inspected by [`check`]. A `Resource`
/// like `CapabilityTable<M>`, so a host/test/scenario-harness driving
/// several batch turns accumulates one log across all of them — call
/// [`GroundTruthLog::reset`] between comparisons that should be independent
/// (exactly like `brink_runtime::effect_trace::reset`).
#[derive(Resource)]
pub struct GroundTruthLog<M: Send + Sync + 'static = ()> {
    entries: Vec<ObservedAccess>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for GroundTruthLog<M> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            _marker: PhantomData,
        }
    }
}

impl<M: Send + Sync + 'static> GroundTruthLog<M> {
    /// Every dispatch recorded since the last [`reset`](Self::reset).
    #[must_use]
    pub fn entries(&self) -> &[ObservedAccess] {
        &self.entries
    }

    /// Clear the log. Call before a run whose observed accesses should be
    /// compared independently of any prior run in the same `World`.
    pub fn reset(&mut self) {
        self.entries.clear();
    }
}

/// Record one real query-binding dispatch. Called from
/// `bindings.rs`'s `dispatch_one_external` right after
/// `world.run_system_with` succeeds, with the binding's registration-time
/// [`Access`] (captured by `bind_brink_query`). A no-op if the flow entity
/// no longer carries a [`BrinkProgram<M>`] (it despawned between dispatch
/// decision and this call) — there is no story to attribute the access to,
/// and this instrumentation must never turn a benign race into a hard
/// error, mirroring `brink_runtime`'s own "a silent miss skips recording"
/// rule for its `note_effect_*` helpers.
pub(crate) fn record<M: Send + Sync + 'static>(
    world: &mut World,
    flow: Entity,
    binding: &str,
    access: Access,
) {
    let Some(story) = world.get::<BrinkProgram<M>>(flow).map(|p| p.handle.id()) else {
        return;
    };
    world
        .get_resource_or_insert_with(GroundTruthLog::<M>::default)
        .entries
        .push(ObservedAccess {
            flow,
            story,
            binding: binding.to_string(),
            access,
        });
}

/// Whether a violating access was a read or a write — named in
/// [`Violation`] so a report can say exactly which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessKind {
    Read,
    Write,
}

impl std::fmt::Display for AccessKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Read => "read",
            Self::Write => "write",
        })
    }
}

/// One under-report: a real `bind_brink_query` dispatch touched a component
/// its story's capability manifest never declares — the exact class this
/// issue guards (names the flow, the component, and the binding).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub flow: Entity,
    pub story: AssetId<ProgramAsset>,
    pub binding: String,
    /// Human-readable component name (`Components::get_name`), or a
    /// debug-formatted `ComponentId` if bevy has no name for it. `"<all
    /// components>"` for the rare case of an unbounded observed access
    /// (e.g. a binding taking `&World`/`EntityRef`), which can't be named
    /// component-by-component.
    pub component: String,
    pub kind: AccessKind,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "flow {:?} binding `{}` {}s component `{}`, which story {:?}'s capability manifest never declares",
            self.flow, self.binding, self.kind, self.component, self.story
        )
    }
}

/// The ground-truth check itself: for every dispatch [`record`]ed into
/// `log`, assert its real bevy [`Access`] is a subset of the story's BH-1
/// row-join access (`CapabilityTable::access_for`, aggregated across
/// containers via [`aggregate_access`] — the same aggregate BH-2/BH-3
/// already consume for their own bookkeeping, since v1 has no per-container
/// narrowing on the host side either, `docs/effects-spec.md` §7). A story
/// with no capability table loaded at all (no manifest/registry wired)
/// joins to an empty `Access` — any real component touch is then correctly
/// a violation, since nothing was declared.
///
/// Never panics — returns every violation found, named by
/// flow/component/binding, for the caller (a test or scenario harness) to
/// assert against (e.g. `assert!(violations.is_empty())`).
#[must_use]
pub fn check<M: Send + Sync + 'static>(
    log: &GroundTruthLog<M>,
    cap_table: &CapabilityTable<M>,
    components: &Components,
) -> Vec<Violation> {
    let mut violations = Vec::new();
    for entry in &log.entries {
        let declared = cap_table
            .access_for(entry.story)
            .map(aggregate_access)
            .unwrap_or_default();
        if entry.access.is_subset(&declared) {
            continue;
        }
        let Ok(iter) = entry.access.try_iter_access() else {
            violations.push(Violation {
                flow: entry.flow,
                story: entry.story,
                binding: entry.binding.clone(),
                component: "<all components>".to_string(),
                kind: AccessKind::Read,
            });
            continue;
        };
        for kind in iter {
            let (id, access_kind) = match kind {
                ComponentAccessKind::Exclusive(id) => (id, AccessKind::Write),
                ComponentAccessKind::Shared(id) => (id, AccessKind::Read),
                // A `Has<T>`-style archetypal check never touches the
                // component's value, so it can never be the ECS-value
                // under-report this check guards against.
                ComponentAccessKind::Archetypal(_) => continue,
            };
            let covered = match access_kind {
                AccessKind::Write => declared.has_write(id),
                AccessKind::Read => declared.has_read(id),
            };
            if covered {
                continue;
            }
            let component = components
                .get_name(id)
                .map_or_else(|| format!("{id:?}"), |n| n.to_string());
            violations.push(Violation {
                flow: entry.flow,
                story: entry.story,
                binding: entry.binding.clone(),
                component,
                kind: access_kind,
            });
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use bevy_ecs::component::Component;
    use bevy_ecs::world::World;

    use super::*;

    #[derive(Component)]
    struct Transform;

    #[derive(Component)]
    struct AudioSink;

    fn story_id() -> AssetId<ProgramAsset> {
        AssetId::<ProgramAsset>::invalid()
    }

    #[test]
    fn subset_access_produces_no_violations() {
        let mut world = World::new();
        let transform_id = world.register_component::<Transform>();
        let flow = world.spawn_empty().id();

        let mut log = GroundTruthLog::<()>::default();
        let mut observed = Access::default();
        observed.add_read(transform_id);
        log.entries.push(ObservedAccess {
            flow,
            story: story_id(),
            binding: "get_position".to_string(),
            access: observed,
        });

        let mut declared = Access::default();
        declared.add_read(transform_id);
        let mut table = crate::capability::ContainerAccessTable::default();
        table.insert(
            brink_format::DefinitionId::new(brink_format::DefinitionTag::Address, 0),
            crate::capability::ContainerAccess {
                access: declared,
                ..Default::default()
            },
        );
        let mut cap_table = CapabilityTable::<()>::default();
        cap_table.insert_for_test(story_id(), Ok(table));

        let violations = check(&log, &cap_table, world.components());
        assert!(violations.is_empty(), "{violations:?}");
    }

    #[test]
    fn write_beyond_declared_read_is_a_named_violation() {
        let mut world = World::new();
        let transform_id = world.register_component::<Transform>();
        let audio_id = world.register_component::<AudioSink>();
        let flow = world.spawn_empty().id();

        let mut log = GroundTruthLog::<()>::default();
        let mut observed = Access::default();
        observed.add_read(transform_id);
        observed.add_write(audio_id);
        log.entries.push(ObservedAccess {
            flow,
            story: story_id(),
            binding: "play_and_reposition".to_string(),
            access: observed,
        });

        // Manifest declares only the Transform read — AudioSink write is an
        // under-report.
        let mut declared = Access::default();
        declared.add_read(transform_id);
        let mut table = crate::capability::ContainerAccessTable::default();
        table.insert(
            brink_format::DefinitionId::new(brink_format::DefinitionTag::Address, 0),
            crate::capability::ContainerAccess {
                access: declared,
                ..Default::default()
            },
        );
        let mut cap_table = CapabilityTable::<()>::default();
        cap_table.insert_for_test(story_id(), Ok(table));

        let violations = check(&log, &cap_table, world.components());
        assert_eq!(violations.len(), 1, "{violations:?}");
        let v = &violations[0];
        assert_eq!(v.flow, flow);
        assert_eq!(v.binding, "play_and_reposition");
        assert_eq!(v.kind, AccessKind::Write);
        assert!(
            v.component.contains("AudioSink"),
            "violation should name the offending component: {v:?}"
        );
    }

    #[test]
    fn no_capability_table_at_all_flags_any_real_access() {
        let mut world = World::new();
        let transform_id = world.register_component::<Transform>();
        let flow = world.spawn_empty().id();

        let mut log = GroundTruthLog::<()>::default();
        let mut observed = Access::default();
        observed.add_read(transform_id);
        log.entries.push(ObservedAccess {
            flow,
            story: story_id(),
            binding: "get_position".to_string(),
            access: observed,
        });

        let cap_table = CapabilityTable::<()>::default();
        let violations = check(&log, &cap_table, world.components());
        assert_eq!(
            violations.len(),
            1,
            "no manifest wired at all means nothing is declared — any real access is a violation: {violations:?}"
        );
    }
}

/// End-to-end scenario coverage (this issue's own gate): drives real
/// `bind_brink_query` bindings through the actual dispatch call site
/// (`bindings.rs`'s `dispatch_one_external`, reached here via the plugin's
/// `resolve_pending_externals` servicing a batch turn's parked query — the
/// same path a batch-mode host's flows go through today), across several
/// flow counts, and asserts [`check`] behaves correctly against both a
/// correctly-declared and a deliberately under-declared manifest. This is
/// the "wire into the scenario harness so randomized workloads exercise the
/// assertion" deliverable: a small, real (not mocked) workload axis
/// (flow count) exercising the whole registration → dispatch → check
/// pipeline, rather than a hand-constructed `ObservedAccess`/`Access` like
/// the unit tests above. Full integration into `benches/scenario/model.rs`'s
/// BH-B axes matrix (a dedicated access-disjointness axis) is future BH-B
/// work per the epic's own scope note (#897) — flagged, not attempted here.
#[cfg(test)]
mod scenario {
    use bevy_app::Update;
    use bevy_asset::Assets;
    use bevy_ecs::component::Component;
    use bevy_ecs::entity::Entity;
    use bevy_ecs::system::{In, Query};
    use brink_format::Value;
    use std::collections::BTreeMap;

    use super::*;
    use crate::asset::{BrinkStoryAsset, LineTablesAsset};
    use crate::capability::{CapabilityEffects, CapabilityManifest, CapabilityManifestExternal};
    use crate::{
        BrinkBindingsAppExt, BrinkCapabilityAppExt, BrinkFlowRequest, BrinkQueryInput,
        advance_batch,
    };

    #[derive(Component)]
    struct Enemy;

    fn enemy_count(In((_entity, _args)): In<BrinkQueryInput>, q: Query<&Enemy>) -> Value {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_possible_wrap,
            reason = "test story, tiny enemy count"
        )]
        Value::Int(q.iter().count() as i32)
    }

    const STORY_SOURCE: &str =
        "EXTERNAL enemy_count()\n-> start\n=== start ===\nEnemies near: {enemy_count()}.\n-> END\n";

    /// Compile `STORY_SOURCE` for real (through the full `.inkb` round trip,
    /// not the `add_story_assets` test helper, which zeroes `effect_rows` —
    /// BH-1's join needs real ones) and spawn `flow_count` flows against one
    /// shared story, driving `advance_batch` until every flow reaches its
    /// terminal `Done` line. Returns the app so the caller can inspect its
    /// `GroundTruthLog`/`CapabilityTable`.
    fn drive_scenario(flow_count: usize, manifest: CapabilityManifest) -> bevy_app::App {
        let mut app = crate::test_support::make_test_app();
        app.bind_brink_query::<(), _, _>("enemy_count", enemy_count);
        app.register_capability::<(), Enemy>("Enemy");
        app.insert_resource(manifest);
        app.add_systems(Update, advance_batch::<()>);
        app.world_mut().spawn(Enemy);

        let out = brink_compiler::compile("t.ink", move |p| {
            if p == "t.ink" {
                Ok(STORY_SOURCE.to_string())
            } else {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "x"))
            }
        })
        .expect("scenario story should compile");
        let mut inkb = Vec::new();
        brink_format::write_inkb(&out.data, &mut inkb);
        let loaded = brink_format::read_inkb(&inkb).expect("read_inkb");
        let (program, tables) = brink_runtime::link(&loaded).expect("link");
        let (_, initial_context) = brink_runtime::FlowInstance::new_at_root(&program);

        let world = app.world_mut();
        let program_handle = world
            .resource_mut::<Assets<ProgramAsset>>()
            .add(ProgramAsset {
                program,
                initial_context,
                effect_rows: loaded.effect_rows,
            });
        let tables_handle = world
            .resource_mut::<Assets<LineTablesAsset>>()
            .add(LineTablesAsset { tables });
        let story_handle = world
            .resource_mut::<Assets<BrinkStoryAsset>>()
            .add(BrinkStoryAsset {
                program: program_handle,
                line_tables: tables_handle,
            });

        let flows: Vec<Entity> = (0..flow_count)
            .map(|_| {
                app.world_mut()
                    .spawn(
                        BrinkFlowRequest::<()>::builder()
                            .story(story_handle.clone())
                            .build(),
                    )
                    .id()
            })
            .collect();

        // Generous, fixed tick budget: fulfillment + the capability join's
        // one-tick-late asset-event flush + several batch turns' worth of
        // Collect/park/resolve/continue round trips. Every flow's story is
        // two turns deep (one call, one terminal line), so this comfortably
        // converges regardless of how bevy orders `advance_batch` relative
        // to `resolve_pending_externals` within a tick.
        for _ in 0..12 {
            app.update();
        }

        for flow in flows {
            // "Reached its terminal line" ⟺ no longer parked on a pending
            // external — the story is exactly two turns deep (one call, one
            // `-> END`), so once unparked it has nothing left to await.
            let unparked = app
                .world()
                .get::<crate::BrinkFlow<()>>(flow)
                .is_some_and(|f| !f.inner.has_pending_external());
            assert!(
                unparked,
                "flow {flow:?} should have resolved its pending external within the tick budget"
            );
        }

        app
    }

    fn declaring_manifest() -> CapabilityManifest {
        CapabilityManifest {
            externals: vec![CapabilityManifestExternal {
                name: "enemy_count".to_string(),
                effects: CapabilityEffects {
                    reads: vec!["Enemy".to_string()],
                    writes: vec![],
                    detect: BTreeMap::new(),
                },
            }],
        }
    }

    fn under_declaring_manifest() -> CapabilityManifest {
        // An entry for `enemy_count` exists (so the row join doesn't treat it
        // as "no manifest entry at all, contributes nothing" for an unrelated
        // reason) but its `effects` are empty — the exact under-report class
        // this issue guards: the binding really reads `Enemy`, but nothing
        // declares it.
        CapabilityManifest {
            externals: vec![CapabilityManifestExternal {
                name: "enemy_count".to_string(),
                effects: CapabilityEffects::default(),
            }],
        }
    }

    /// Randomized-workload axis (flow count, 1/3/7): with a manifest that
    /// correctly declares `enemy_count`'s `Enemy` read, every real dispatch
    /// recorded across every flow count checks clean.
    #[test]
    fn correctly_declared_manifest_checks_clean_across_flow_counts() {
        for flow_count in [1usize, 3, 7] {
            let app = drive_scenario(flow_count, declaring_manifest());
            let log = app.world().resource::<GroundTruthLog<()>>();
            assert_eq!(
                log.entries().len(),
                flow_count,
                "expected one recorded dispatch per flow at flow_count={flow_count}"
            );
            let cap_table = app.world().resource::<CapabilityTable<()>>();
            let violations = check(log, cap_table, app.world().components());
            assert!(
                violations.is_empty(),
                "flow_count={flow_count}: {violations:?}"
            );
        }
    }

    /// The under-report case: the manifest never declares `enemy_count`'s
    /// real `Enemy` read, so every recorded dispatch is a violation, each
    /// naming its flow, the `Enemy` component, and the `enemy_count` binding.
    #[test]
    fn under_declared_manifest_flags_every_dispatch_by_flow_component_and_binding() {
        let flow_count = 3;
        let app = drive_scenario(flow_count, under_declaring_manifest());
        let log = app.world().resource::<GroundTruthLog<()>>();
        let cap_table = app.world().resource::<CapabilityTable<()>>();
        let violations = check(log, cap_table, app.world().components());
        assert_eq!(violations.len(), flow_count, "{violations:?}");
        for v in &violations {
            assert_eq!(v.binding, "enemy_count");
            assert_eq!(v.kind, AccessKind::Read);
            assert!(
                v.component.contains("Enemy"),
                "violation should name the Enemy component: {v:?}"
            );
        }
        // Every one of the flows spawned above is named by some violation —
        // "a violation must name the flow" (this issue's own wording).
        let mut named_flows: Vec<Entity> = violations.iter().map(|v| v.flow).collect();
        named_flows.sort_unstable();
        named_flows.dedup();
        assert_eq!(named_flows.len(), flow_count);
    }
}
