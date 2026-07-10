//! Asynchronous ink → engine external bindings: resolve **across frames**.
//!
//! A synchronous `bind_brink_query` resolves in one resolver pass. Some
//! externals can't: a `pick_target()` that opens a targeting UI and waits for
//! a click, or an `expensive_roll()` that runs off-thread. These *park* the
//! flow on a pending external (the runtime's `AwaitingExternal`/
//! `resolve_external` pause/resume) and resolve it whenever the work finishes.
//!
//! Two registration verbs (on [`BrinkBindingsAppExt`](crate::BrinkBindingsAppExt)):
//!
//! - **`bind_brink_async`** — the primitive. When ink calls the external the
//!   flow parks and [`BrinkExternalAwaited`] fires (once) at the flow entity.
//!   An observer does whatever multi-frame work it needs (UI, input, world
//!   state) and eventually calls
//!   [`resolve_brink_external`](BrinkResolveExternalExt::resolve_brink_external).
//! - **`bind_brink_task`** — sugar over [`bevy_tasks::AsyncComputeTaskPool`]:
//!   bevy-brink spawns the future, parks a [`BrinkPendingTask`] on the flow,
//!   and [`poll_brink_tasks`] resolves it when the task completes. The future
//!   is `Send + 'static` and computes from the ink args only (no World access);
//!   use the event primitive for World-dependent async.
//!
//! **Correlation is the flow entity.** A flow parks on exactly one external and
//! is frozen until resolved, so the entity is the unambiguous key — no
//! per-call entity or correlation id is needed (unlike `commands.brink_call`).

use std::marker::PhantomData;

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::event::EntityEvent;
use bevy_ecs::system::{Commands, Query};
use bevy_ecs::world::World;
use bevy_log::warn;
use bevy_tasks::{Task, block_on, poll_once};
use brink_format::Value;

use crate::flow::BrinkFlow;

/// Fired (once, targeted at the flow entity) when a flow parks on a
/// [`bind_brink_async`](crate::BrinkBindingsAppExt::bind_brink_async) external.
///
/// React with a global observer or `entity.observe(...)`, kick off whatever
/// multi-frame work the external represents, and resolve with
/// [`resolve_brink_external`](BrinkResolveExternalExt::resolve_brink_external):
///
/// ```ignore
/// app.add_observer(|on: On<BrinkExternalAwaited>, mut commands: Commands| {
///     if on.event().name == "pick_target" {
///         // … open UI; later, when the player picks target 7:
///         commands.resolve_brink_external::<()>(on.event().entity, Value::Int(7));
///     }
/// });
/// ```
#[derive(EntityEvent)]
pub struct BrinkExternalAwaited<M: Send + Sync + 'static = ()> {
    /// The flow entity awaiting resolution (the observer target).
    pub entity: Entity,
    /// The external function name ink called.
    pub name: String,
    /// The ink call arguments, in declaration order.
    pub args: Vec<Value>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkExternalAwaited<M> {
    pub(crate) fn new(entity: Entity, name: String, args: Vec<Value>) -> Self {
        Self {
            entity,
            name,
            args,
            _marker: PhantomData,
        }
    }
}

/// Marker inserted on a flow while it awaits a `bind_brink_async` external.
///
/// Its presence makes the dispatcher fire [`BrinkExternalAwaited`] exactly
/// once; [`resolve_brink_external`](BrinkResolveExternalExt::resolve_brink_external)
/// removes it on resolution.
#[derive(Component)]
pub struct BrinkAwaiting<M: Send + Sync + 'static = ()> {
    /// The external name being awaited.
    pub name: String,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkAwaiting<M> {
    pub(crate) fn new(name: String) -> Self {
        Self {
            name,
            _marker: PhantomData,
        }
    }
}

/// A detached [`Task`] computing a [`bind_brink_task`](crate::BrinkBindingsAppExt::bind_brink_task)
/// external's value, parked on the flow entity.
///
/// [`poll_brink_tasks`] polls it each frame; when it finishes, the flow's
/// pending external is resolved with the value and this component is removed.
#[derive(Component)]
pub struct BrinkPendingTask<M: Send + Sync + 'static = ()> {
    pub(crate) task: Task<Value>,
    /// External name + args, kept in dev builds so [`poll_brink_tasks`] can
    /// record the task's result into the flow's replay log on completion (the
    /// value isn't available until the future finishes).
    #[cfg(feature = "dev")]
    name: String,
    #[cfg(feature = "dev")]
    args: Vec<Value>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkPendingTask<M> {
    pub(crate) fn new(
        task: Task<Value>,
        #[cfg(feature = "dev")] name: String,
        #[cfg(feature = "dev")] args: Vec<Value>,
    ) -> Self {
        Self {
            task,
            #[cfg(feature = "dev")]
            name,
            #[cfg(feature = "dev")]
            args,
            _marker: PhantomData,
        }
    }
}

/// [`Commands`] extension to resolve a flow's awaited async external.
pub trait BrinkResolveExternalExt {
    /// Resolve the (single) external that `flow` is parked on with `value`,
    /// removing the [`BrinkAwaiting`] marker so the flow resumes on its next
    /// step. A `warn!` no-op if the flow isn't actually awaiting one (stale or
    /// double resolve) — the flow entity is the unambiguous key, since a flow
    /// parks on exactly one external at a time.
    fn resolve_brink_external<M: Send + Sync + 'static>(&mut self, flow: Entity, value: Value);
}

impl BrinkResolveExternalExt for Commands<'_, '_> {
    fn resolve_brink_external<M: Send + Sync + 'static>(&mut self, flow: Entity, value: Value) {
        self.queue(move |world: &mut World| {
            resolve_external_world::<M>(world, flow, value);
        });
    }
}

/// Resolve a flow's pending async external from an exclusive `&mut World`
/// context. Guarded by `has_pending_external()` so stale/double resolves are
/// safe no-ops. Removes the [`BrinkAwaiting`] marker on success.
pub(crate) fn resolve_external_world<M: Send + Sync + 'static>(
    world: &mut World,
    flow: Entity,
    value: Value,
) {
    // Capture the external name (from BrinkAwaiting) + args (while still parked)
    // before we consume `value`, so we can record the resolution into the flow's
    // replay log (dev) for faithful hot-reload replay.
    #[cfg(feature = "dev")]
    let record_info = {
        let name = world.get::<BrinkAwaiting<M>>(flow).map(|a| a.name.clone());
        let args = world
            .get::<BrinkFlow<M>>(flow)
            .filter(|f| f.inner.has_pending_external())
            .map(|f| f.inner.pending_external_args().to_vec());
        name.zip(args).map(|(n, a)| (n, a, value.clone()))
    };

    let resolved = {
        let mut flows = world.query::<&mut BrinkFlow<M>>();
        match flows.get_mut(world, flow) {
            Ok(mut f) if f.inner.has_pending_external() => {
                f.inner.resolve_external(value);
                true
            }
            Ok(_) => {
                warn!(
                    "resolve_brink_external on {flow:?}: flow has no pending external \
                     (already resolved?); ignoring"
                );
                false
            }
            Err(_) => {
                warn!("resolve_brink_external on {flow:?}: not a brink flow; ignoring");
                false
            }
        }
    };
    if resolved {
        world.entity_mut(flow).remove::<BrinkAwaiting<M>>();
        #[cfg(feature = "dev")]
        if let Some((name, args, recorded)) = record_info {
            crate::replay::record_external::<M>(world, flow, &name, &args, &recorded);
        }
    }
}

/// Plugin system: poll detached [`bind_brink_task`](crate::BrinkBindingsAppExt::bind_brink_task)
/// futures; when one finishes, resolve its flow's pending external with the
/// value and drop the [`BrinkPendingTask`]. Polling is non-blocking
/// (`poll_once`). The plugin gates this on `any_with_component::<BrinkPendingTask<M>>`.
pub fn poll_brink_tasks<M: Send + Sync + 'static>(
    mut tasks: Query<(Entity, &mut BrinkPendingTask<M>, &mut BrinkFlow<M>)>,
    mut commands: Commands,
) {
    for (entity, mut pending, mut flow) in &mut tasks {
        if let Some(value) = block_on(poll_once(&mut pending.task)) {
            // Guard: the flow could have been resolved by other means.
            if flow.inner.has_pending_external() {
                // Record the resolved value into the flow's replay log (dev)
                // for faithful hot-reload replay. Deferred via a command so we
                // don't need `BrinkReplayLog` in this non-exclusive query.
                #[cfg(feature = "dev")]
                {
                    let (name, args, recorded) =
                        (pending.name.clone(), pending.args.clone(), value.clone());
                    commands.queue(move |world: &mut World| {
                        crate::replay::record_external::<M>(world, entity, &name, &args, &recorded);
                    });
                }
                flow.inner.resolve_external(value);
            }
            commands.entity(entity).remove::<BrinkPendingTask<M>>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{LineTablesAsset, ProgramAsset};
    use crate::test_support::{add_story_assets, compile_test_story, make_test_app};
    use crate::{
        Advance, BrinkBindings, BrinkBindingsAppExt, BrinkContext, BrinkFlowRequest, BrinkLocale,
        BrinkProgram, advance_flow,
    };
    use bevy_app::{App, Update};
    use bevy_asset::Assets;
    use bevy_ecs::prelude::*;

    #[derive(Resource, Default)]
    struct Lines(Vec<String>);

    /// A normal (non-exclusive) flow driver: step each flow once per frame,
    /// skipping flows parked on a pending external (the plugin's
    /// `resolve_pending_externals` / `poll_brink_tasks` service those; we
    /// resume on a later frame). Mirrors the real consumer pattern.
    #[expect(
        clippy::type_complexity,
        clippy::needless_pass_by_value,
        reason = "bevy systems take their params (Query/Res) by value"
    )]
    fn step_driver(
        mut flows: Query<(
            Entity,
            &mut BrinkFlow<()>,
            &mut BrinkContext<()>,
            &BrinkProgram<()>,
            &BrinkLocale<()>,
        )>,
        globals: Option<ResMut<crate::BrinkGlobals<()>>>,
        programs: Res<Assets<ProgramAsset>>,
        tables: Res<Assets<LineTablesAsset>>,
        bindings: Res<BrinkBindings<()>>,
        mut commands: Commands,
        mut out: ResMut<Lines>,
    ) {
        let Some(mut globals) = globals else {
            return;
        };
        for (entity, mut flow, mut ctx, prog, loc) in &mut flows {
            if flow.inner.has_pending_external() {
                continue;
            }
            let (Some(p), Some(t)) = (programs.get(&prog.handle), tables.get(&loc.handle)) else {
                continue;
            };
            let handler = bindings.handler();
            let mut view = crate::globals::flow_context_view(&mut globals, &mut ctx);
            if let Ok(Advance::Line(line)) = flow.step_one(
                &p.program,
                &t.tables,
                &mut view,
                &handler,
                entity,
                &mut commands,
            ) {
                out.0.push(line.text().to_string());
            }
            handler.flush(&mut commands);
        }
    }

    fn spawn_flow(app: &mut App, src: &str) -> Entity {
        let (program, tables, ctx) = compile_test_story(src);
        let story = add_story_assets(app, program, tables, ctx);
        let entity = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();
        app.update(); // fulfill the request
        entity
    }

    fn pending(app: &App, flow: Entity) -> bool {
        app.world()
            .entity(flow)
            .get::<BrinkFlow<()>>()
            .is_some_and(|f| f.inner.has_pending_external())
    }

    /// A `bind_brink_task` external parks the flow; bevy-brink spawns the
    /// future, `poll_brink_tasks` resolves it when the task finishes, and the
    /// flow resumes with the computed value.
    #[test]
    fn task_binding_resolves_across_frames() {
        let mut app = make_test_app();
        app.init_resource::<Lines>();
        app.add_systems(Update, step_driver);
        app.bind_brink_task::<(), _, _>("expensive_roll", |args: Vec<Value>| async move {
            let n = args.first().and_then(Value::as_int).unwrap_or(0);
            Value::Int(n * 2)
        });

        let flow = spawn_flow(
            &mut app,
            "EXTERNAL expensive_roll(n)\nRolled: {expensive_roll(21)}.\n-> END\n",
        );

        // Drive until the resolved line appears (cap to avoid hangs).
        let mut got = false;
        for _ in 0..200 {
            app.update();
            if app
                .world()
                .resource::<Lines>()
                .0
                .iter()
                .any(|l| l.contains("Rolled: 42."))
            {
                got = true;
                break;
            }
        }
        assert!(
            got,
            "task should resolve to 42 and resume the flow; got {:?}",
            app.world().resource::<Lines>().0
        );
        assert!(!pending(&app, flow), "flow no longer parked after resolve");
    }

    /// A `bind_brink_async` external fires `BrinkExternalAwaited` exactly once;
    /// an observer resolves it via `resolve_brink_external`; the flow resumes.
    #[test]
    fn async_event_binding_fires_once_and_resolves() {
        #[derive(Resource, Default)]
        struct Awaited(Vec<String>);

        let mut app = make_test_app();
        app.init_resource::<Lines>();
        app.init_resource::<Awaited>();
        app.add_systems(Update, step_driver);
        app.bind_brink_async::<()>("pick_target");
        app.add_observer(
            |on: On<BrinkExternalAwaited<()>>, mut commands: Commands, mut log: ResMut<Awaited>| {
                log.0.push(on.event().name.clone());
                commands.resolve_brink_external::<()>(on.event().entity, Value::Int(7));
            },
        );

        spawn_flow(
            &mut app,
            "EXTERNAL pick_target()\nYou aim at {pick_target()}.\n-> END\n",
        );

        let mut got = false;
        for _ in 0..50 {
            app.update();
            if app
                .world()
                .resource::<Lines>()
                .0
                .iter()
                .any(|l| l.contains("You aim at 7."))
            {
                got = true;
                break;
            }
        }
        assert!(
            got,
            "observer should resolve pick_target to 7 and resume; got {:?}",
            app.world().resource::<Lines>().0
        );
        assert_eq!(
            app.world().resource::<Awaited>().0,
            vec!["pick_target".to_string()],
            "BrinkExternalAwaited fires exactly once"
        );
    }

    /// While parked on a `bind_brink_async` external with no resolution, the
    /// flow stays frozen: the event fires only once and `has_pending_external`
    /// stays true across many frames (no advancement, no re-fire).
    #[test]
    fn async_event_binding_stays_frozen_until_resolved() {
        #[derive(Resource, Default)]
        struct FireCount(usize);

        let mut app = make_test_app();
        app.init_resource::<Lines>();
        app.init_resource::<FireCount>();
        app.add_systems(Update, step_driver);
        app.bind_brink_async::<()>("pick_target");
        // Observer counts but never resolves.
        app.add_observer(
            |_on: On<BrinkExternalAwaited<()>>, mut n: ResMut<FireCount>| {
                n.0 += 1;
            },
        );

        let flow = spawn_flow(
            &mut app,
            "EXTERNAL pick_target()\nYou aim at {pick_target()}.\n-> END\n",
        );

        for _ in 0..20 {
            app.update();
        }

        assert!(pending(&app, flow), "flow stays parked without resolution");
        assert_eq!(
            app.world().resource::<FireCount>().0,
            1,
            "event fires once, not per frame"
        );
        assert!(
            !app.world()
                .resource::<Lines>()
                .0
                .iter()
                .any(|l| l.contains("aim at")),
            "no resolved line while frozen"
        );
    }

    /// The one-pass exclusive driver can't await an async external — it returns
    /// a clear `AsyncExternalUnsupported` rather than `UnknownQuery`.
    #[test]
    fn advance_flow_rejects_async_external() {
        let mut app = make_test_app();
        app.bind_brink_async::<()>("pick_target");

        let flow = spawn_flow(
            &mut app,
            "EXTERNAL pick_target()\nYou aim at {pick_target()}.\n-> END\n",
        );

        let err = advance_flow::<()>(app.world_mut(), flow).unwrap_err();
        assert!(
            matches!(err, crate::BrinkCallError::AsyncExternalUnsupported(ref n) if n == "pick_target"),
            "got {err:?}"
        );
    }
}
