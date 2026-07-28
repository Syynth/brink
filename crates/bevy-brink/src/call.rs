//! Deferred engine→ink calls for non-exclusive systems.
//!
//! [`call_ink_function`](crate::call_ink_function) needs `&mut World`, so a
//! normal system (which only has `Query`/`Res` access) can't use it
//! directly. Instead it *requests* a call via
//! [`brink_call`](BrinkCallCommandsExt::brink_call) and reacts to the
//! result with an observer scoped to a unique per-call entity:
//!
//! ```no_run
//! # use bevy_ecs::entity::Entity;
//! # use bevy_ecs::observer::On;
//! # use bevy_ecs::resource::Resource;
//! # use bevy_ecs::system::{Commands, ResMut};
//! # use bevy_brink::{BrinkCallCommandsExt, BrinkCallResolved};
//! # #[derive(Resource, Default)]
//! # struct PendingMoves;
//! # impl PendingMoves {
//! #     fn execute_queued(&mut self) {}
//! # }
//! # fn example(mut commands: Commands, flow_entity: Entity, in_combat: bool) {
//! commands
//!     .brink_call::<()>(flow_entity, "can_player_advance", (in_combat,))
//!     .observe(|on: On<BrinkCallResolved>, mut moves: ResMut<PendingMoves>| {
//!         if on.event().value.as_bool() == Some(true) {
//!             moves.execute_queued();
//!         }
//!     });
//! # }
//! ```
//!
//! Each `brink_call` spawns its own entity; the plugin's exclusive
//! [`resolve_brink_calls`] system evaluates the function (running any
//! world-access query bindings) and fires [`BrinkCallResolved`] /
//! [`BrinkCallFailed`] **targeted at that entity**, so the observer runs
//! exactly once and can never be confused with another call's result. The
//! call entity (and its observer) is despawned afterward.
//!
//! [`brink_call_batch`](BrinkCallCommandsExt::brink_call_batch) is the
//! non-exclusive counterpart of [`call_ink_functions`](crate::call_ink_functions)
//! (#1076): a normal system queues a whole ordered batch of calls at once
//! (e.g. an event-folding system that hands a frame's worth of sightings to
//! ink) instead of issuing one `brink_call` per call and paying a
//! `SystemState` setup — and per-call resolution order — that isn't
//! pinned across separate deferred requests. `brink_call_batch` spawns one
//! request entity holding the whole ordered call list; the plugin's
//! exclusive [`resolve_brink_call_batches`] system resolves the whole list
//! through [`call_ink_functions`](crate::call_ink_functions) in a single
//! call, which is what pins the front-to-back ordering and per-call
//! isolation [`call_ink_functions`](crate::call_ink_functions) documents —
//! not merely "these requests happen to run in the same frame." (The
//! **single VM-eval setup** it also does is a separate amortization — one
//! `SystemState` build instead of one per call — not the mechanism that
//! pins ordering.) The whole
//! batch's results (one `Result` per call, in call order — a failing call
//! does not abort the batch, matching [`call_ink_functions`](crate::call_ink_functions)'s
//! no-short-circuit contract) are delivered in one
//! [`BrinkCallBatchResolved`] event at the call entity:
//!
//! ```no_run
//! # use bevy_ecs::entity::Entity;
//! # use bevy_ecs::observer::On;
//! # use bevy_ecs::system::Commands;
//! # use bevy_brink::{BrinkCallBatchResolved, BrinkCallCommandsExt, Value};
//! # fn example(mut commands: Commands, flow_entity: Entity, dt: f32, amount: f32) {
//! commands
//!     .brink_call_batch::<()>(flow_entity, [
//!         ("decay", vec![Value::Float(dt)]),
//!         ("escalate_spotting", vec![Value::Float(amount)]),
//!     ])
//!     .observe(|on: On<BrinkCallBatchResolved>| {
//!         for result in &on.event().results { /* … */ }
//!     });
//! # }
//! ```
//!
//! Same-frame ordering *across* separate deferred requests (whether two
//! `brink_call`s, two `brink_call_batch`es, or a mix, targeting the same
//! flow) is **not** pinned by either resolver — each is a distinct ECS
//! query result, and Bevy's per-archetype iteration order (not a
//! documented guarantee) is all that governs it. This mirrors
//! [`resolve_brink_calls`]'s pre-existing posture for concurrent single
//! calls; `brink_call_batch` only pins ordering *within* the one batch a
//! single deferred request carries. A host that needs a guaranteed order
//! across several call groups targeting one flow in one frame should fold
//! them into a single `brink_call_batch` (or call `call_ink_functions`
//! directly from an exclusive system).

use std::marker::PhantomData;

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::event::EntityEvent;
use bevy_ecs::system::{Commands, EntityCommands};
use bevy_ecs::world::World;
use brink_format::Value;

use crate::bindings::{call_ink_function, call_ink_functions};

/// Converts call-site arguments into the ink argument vector. Implemented
/// for `()`, tuples of `Into<Value>` (up to 4), `Vec<Value>`, and
/// `&[Value]` — so both `(in_combat, 3)` and an explicit `&[..]` work.
pub trait IntoBrinkArgs {
    /// Produce the ink arguments in declaration order.
    fn into_brink_args(self) -> Vec<Value>;
}

impl IntoBrinkArgs for () {
    fn into_brink_args(self) -> Vec<Value> {
        Vec::new()
    }
}

impl IntoBrinkArgs for Vec<Value> {
    fn into_brink_args(self) -> Vec<Value> {
        self
    }
}

impl IntoBrinkArgs for &[Value] {
    fn into_brink_args(self) -> Vec<Value> {
        self.to_vec()
    }
}

macro_rules! impl_into_brink_args_tuple {
    ($($T:ident $idx:tt),+) => {
        impl<$($T: Into<Value>),+> IntoBrinkArgs for ($($T,)+) {
            fn into_brink_args(self) -> Vec<Value> {
                vec![$(self.$idx.into()),+]
            }
        }
    };
}

impl_into_brink_args_tuple!(A 0);
impl_into_brink_args_tuple!(A 0, B 1);
impl_into_brink_args_tuple!(A 0, B 1, C 2);
impl_into_brink_args_tuple!(A 0, B 1, C 2, D 3);

/// A pending deferred engine→ink call. Spawned on its own entity by
/// [`brink_call`](BrinkCallCommandsExt::brink_call); consumed by
/// [`resolve_brink_calls`].
#[derive(Component)]
pub struct BrinkCallRequest<M: Send + Sync + 'static = ()> {
    /// The flow entity to evaluate the function on.
    pub target: Entity,
    /// The ink function name.
    pub name: String,
    /// The arguments, in declaration order.
    pub args: Vec<Value>,
    _marker: PhantomData<fn() -> M>,
}

/// Fired (targeted at the per-call entity) when a deferred call succeeds.
/// React with `.observe(|on: On<BrinkCallResolved>| …)` on the
/// [`brink_call`](BrinkCallCommandsExt::brink_call) return value.
#[derive(EntityEvent)]
pub struct BrinkCallResolved<M: Send + Sync + 'static = ()> {
    /// The per-call entity (the observer target).
    pub entity: Entity,
    /// The function's return value.
    pub value: Value,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkCallResolved<M> {
    pub(crate) fn new(entity: Entity, value: Value) -> Self {
        Self {
            entity,
            value,
            _marker: PhantomData,
        }
    }
}

/// Fired (targeted at the per-call entity) when a deferred call fails
/// (unknown function, unbound world query, runtime error, …).
#[derive(EntityEvent)]
pub struct BrinkCallFailed<M: Send + Sync + 'static = ()> {
    /// The per-call entity (the observer target).
    pub entity: Entity,
    /// Human-readable failure description.
    pub error: String,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkCallFailed<M> {
    pub(crate) fn new(entity: Entity, error: String) -> Self {
        Self {
            entity,
            error,
            _marker: PhantomData,
        }
    }
}

/// A pending deferred *batch* engine→ink call. Spawned on its own entity by
/// [`brink_call_batch`](BrinkCallCommandsExt::brink_call_batch); consumed by
/// [`resolve_brink_call_batches`].
#[derive(Component)]
pub struct BrinkCallBatchRequest<M: Send + Sync + 'static = ()> {
    /// The flow entity to evaluate the batch on.
    pub target: Entity,
    /// The calls, in the order they must run.
    pub calls: Vec<(String, Vec<Value>)>,
    _marker: PhantomData<fn() -> M>,
}

/// Fired (targeted at the per-call entity) when a deferred batch call
/// finishes. One entry per queued call, in call order — a failing call
/// yields `Err` in its own slot rather than aborting the batch, matching
/// [`call_ink_functions`](crate::call_ink_functions)'s no-short-circuit
/// contract. React with `.observe(|on: On<BrinkCallBatchResolved>| …)` on
/// the [`brink_call_batch`](BrinkCallCommandsExt::brink_call_batch) return
/// value.
#[derive(EntityEvent)]
pub struct BrinkCallBatchResolved<M: Send + Sync + 'static = ()> {
    /// The per-call entity (the observer target).
    pub entity: Entity,
    /// One result per queued call, in call order.
    pub results: Vec<Result<Value, String>>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkCallBatchResolved<M> {
    pub(crate) fn new(entity: Entity, results: Vec<Result<Value, String>>) -> Self {
        Self {
            entity,
            results,
            _marker: PhantomData,
        }
    }
}

/// `Commands` extension for requesting a deferred engine→ink call.
pub trait BrinkCallCommandsExt {
    /// Request an ink function evaluation on `flow` (the flow entity),
    /// returning the [`EntityCommands`] of the spawned per-call entity so
    /// you can attach result observers:
    ///
    /// ```no_run
    /// # use bevy_ecs::entity::Entity;
    /// # use bevy_ecs::observer::On;
    /// # use bevy_ecs::system::Commands;
    /// # use bevy_brink::{BrinkCallCommandsExt, BrinkCallResolved};
    /// # fn example(mut commands: Commands, flow: Entity) {
    /// commands.brink_call::<()>(flow, "can_spawn", ())
    ///     .observe(|on: On<BrinkCallResolved>| { /* use on.event().value */ });
    /// # }
    /// ```
    ///
    /// The result is delivered exactly once, scoped to the returned entity
    /// — there is no way to mis-correlate it with another call.
    fn brink_call<M: Send + Sync + 'static>(
        &mut self,
        flow: Entity,
        name: impl Into<String>,
        args: impl IntoBrinkArgs,
    ) -> EntityCommands<'_>;

    /// Request a deferred **batch** of ink function evaluations on `flow`,
    /// run front-to-back in a single VM-eval setup — the non-exclusive
    /// counterpart of [`call_ink_functions`](crate::call_ink_functions).
    /// Returns the [`EntityCommands`] of the spawned per-batch entity so
    /// you can attach a result observer:
    ///
    /// ```no_run
    /// # use bevy_ecs::entity::Entity;
    /// # use bevy_ecs::observer::On;
    /// # use bevy_ecs::system::Commands;
    /// # use bevy_brink::{BrinkCallBatchResolved, BrinkCallCommandsExt, Value};
    /// # fn example(mut commands: Commands, flow: Entity, dt: f32, amount: f32) {
    /// commands
    ///     .brink_call_batch::<()>(flow, [
    ///         ("decay", vec![Value::Float(dt)]),
    ///         ("escalate_spotting", vec![Value::Float(amount)]),
    ///     ])
    ///     .observe(|on: On<BrinkCallBatchResolved>| { /* on.event().results */ });
    /// # }
    /// ```
    ///
    /// The whole batch's results (one `Result` per call, in call order) are
    /// delivered exactly once, in one [`BrinkCallBatchResolved`] event
    /// scoped to the returned entity. See the module docs for the ordering
    /// guarantee this pins (within the batch) versus what it leaves
    /// unpinned (across separate deferred requests).
    fn brink_call_batch<M: Send + Sync + 'static>(
        &mut self,
        flow: Entity,
        calls: impl IntoIterator<Item = (impl Into<String>, impl IntoBrinkArgs)>,
    ) -> EntityCommands<'_>;
}

impl BrinkCallCommandsExt for Commands<'_, '_> {
    fn brink_call<M: Send + Sync + 'static>(
        &mut self,
        flow: Entity,
        name: impl Into<String>,
        args: impl IntoBrinkArgs,
    ) -> EntityCommands<'_> {
        self.spawn(BrinkCallRequest::<M> {
            target: flow,
            name: name.into(),
            args: args.into_brink_args(),
            _marker: PhantomData,
        })
    }

    fn brink_call_batch<M: Send + Sync + 'static>(
        &mut self,
        flow: Entity,
        calls: impl IntoIterator<Item = (impl Into<String>, impl IntoBrinkArgs)>,
    ) -> EntityCommands<'_> {
        let calls = calls
            .into_iter()
            .map(|(name, args)| (name.into(), args.into_brink_args()))
            .collect();
        self.spawn(BrinkCallBatchRequest::<M> {
            target: flow,
            calls,
            _marker: PhantomData,
        })
    }
}

/// Exclusive system (registered by the plugin) that resolves pending
/// [`BrinkCallRequest<M>`]s: evaluates each function via
/// [`call_ink_function`], fires [`BrinkCallResolved`] / [`BrinkCallFailed`]
/// at the call entity, and despawns it.
pub fn resolve_brink_calls<M: Send + Sync + 'static>(world: &mut World) {
    let mut query = world.query::<(Entity, &BrinkCallRequest<M>)>();
    let pending: Vec<(Entity, Entity, String, Vec<Value>)> = query
        .iter(world)
        .map(|(call_entity, req)| (call_entity, req.target, req.name.clone(), req.args.clone()))
        .collect();

    for (call_entity, target, name, args) in pending {
        match call_ink_function::<M>(world, target, &name, &args) {
            Ok(value) => {
                world
                    .entity_mut(call_entity)
                    .trigger(|e| BrinkCallResolved::<M>::new(e, value));
            }
            Err(err) => {
                let message = err.to_string();
                world
                    .entity_mut(call_entity)
                    .trigger(|e| BrinkCallFailed::<M>::new(e, message));
            }
        }
        world.despawn(call_entity);
    }
}

/// One pending batch request snapshot: `(call entity, target flow, its
/// ordered calls)`. Factored out of [`resolve_brink_call_batches`] purely
/// to keep the collected `Vec`'s element type nameable (clippy
/// `type_complexity`).
type PendingBatch = (Entity, Entity, Vec<(String, Vec<Value>)>);

/// Exclusive system (registered by the plugin) that resolves pending
/// [`BrinkCallBatchRequest<M>`]s: evaluates each queued batch through
/// [`call_ink_functions`] — one `SystemState` setup per batch, calls
/// running front-to-back — fires [`BrinkCallBatchResolved`] at the batch
/// entity with the full per-call result `Vec`, and despawns it.
pub fn resolve_brink_call_batches<M: Send + Sync + 'static>(world: &mut World) {
    let mut query = world.query::<(Entity, &BrinkCallBatchRequest<M>)>();
    let pending: Vec<PendingBatch> = query
        .iter(world)
        .map(|(call_entity, req)| (call_entity, req.target, req.calls.clone()))
        .collect();

    for (call_entity, target, calls) in pending {
        let results: Vec<Result<Value, String>> =
            call_ink_functions::<M, _, _>(world, target, calls)
                .into_iter()
                .map(|result| result.map_err(|err| err.to_string()))
                .collect();
        world
            .entity_mut(call_entity)
            .trigger(|e| BrinkCallBatchResolved::<M>::new(e, results));
        world.despawn(call_entity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{add_story_assets, compile_test_story, make_test_app};
    use crate::{BrinkBindingsAppExt, BrinkFlow, BrinkFlowRequest};
    use bevy_app::Update;
    use bevy_ecs::prelude::*;

    #[derive(Component)]
    struct Enemy;

    fn enemy_count(In((_e, _args)): In<crate::BrinkQueryInput>, q: Query<&Enemy>) -> Value {
        #[expect(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        Value::Int(q.iter().count() as i32)
    }

    /// A deferred `brink_call` from a normal system resolves via the
    /// plugin's exclusive system and delivers the value to a scoped
    /// observer — exactly once.
    #[test]
    fn brink_call_resolves_to_observer() {
        #[derive(Resource, Default)]
        struct Result(Vec<bool>);

        let mut app = make_test_app();
        app.init_resource::<Result>();
        app.bind_brink_query::<(), _, _>("enemy_count", enemy_count);

        let (program, tables, ctx) = compile_test_story(
            "EXTERNAL enemy_count()\n-> END\n=== function can_spawn() ===\n~ return enemy_count() < 3\n",
        );
        let story = add_story_assets(&mut app, program, tables, ctx);
        app.world_mut().spawn(Enemy);

        let flow = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();
        app.update(); // fulfill

        // A normal (non-exclusive) system issues the deferred call.
        let mut once = true;
        app.add_systems(
            Update,
            move |mut commands: Commands, flows: Query<Entity, With<BrinkFlow<()>>>| {
                if !once {
                    return;
                }
                once = false;
                if let Ok(f) = flows.single() {
                    commands.brink_call::<()>(f, "can_spawn", ()).observe(
                        |on: On<BrinkCallResolved<()>>, mut out: ResMut<Result>| {
                            out.0.push(on.event().value.as_bool().unwrap_or(false));
                        },
                    );
                }
            },
        );

        // Tick 1: the system issues brink_call (spawns the call entity +
        // observer). Tick 2: the exclusive resolver evaluates and fires
        // BrinkCallResolved at the call entity; the observer records it.
        app.update();
        app.update();

        let _ = flow;
        let out = &app.world().resource::<Result>().0;
        assert_eq!(
            out.as_slice(),
            [true],
            "1 enemy < 3 → can_spawn true, delivered once"
        );
    }

    /// A deferred `brink_call_batch` from a normal system resolves via the
    /// plugin's exclusive [`resolve_brink_call_batches`] and delivers one
    /// [`BrinkCallBatchResolved`] to a scoped observer, exactly once, with
    /// results in call order. Also proves the batch's #1076 core property
    /// end-to-end through the deferred path: a failing call (unknown
    /// function) fails in its own slot without aborting the batch or
    /// perturbing story state, a later call still sees an earlier call's
    /// mutation, and — critically, since this is the reason the deferred
    /// resolver must be exclusive — a **world-access `bind_brink_query`
    /// binding** queued right after the failing slot still resolves
    /// against the World, the same ordering/isolation `call_ink_functions`
    /// guarantees for the exclusive path.
    #[test]
    fn brink_call_batch_resolves_ordered_results_to_observer() {
        #[derive(Resource, Default)]
        struct Result(Vec<Vec<std::result::Result<Value, String>>>);

        let mut app = make_test_app();
        app.init_resource::<Result>();
        app.bind_brink_query::<(), _, _>("enemy_count", enemy_count);
        app.world_mut().spawn(Enemy);
        app.world_mut().spawn(Enemy);

        let (program, tables, ctx) = compile_test_story(
            "EXTERNAL enemy_count()\nVAR total = 0\n-> END\n\
             === function add(n) ===\n~ total = total + n\n~ return total\n\
             === function get() ===\n~ return total\n\
             === function seen() ===\n~ return enemy_count()\n",
        );
        let story = add_story_assets(&mut app, program, tables, ctx);
        app.world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build());
        app.update(); // fulfill

        // A normal (non-exclusive) system issues the deferred batch call.
        let mut once = true;
        app.add_systems(
            Update,
            move |mut commands: Commands, flows: Query<Entity, With<BrinkFlow<()>>>| {
                if !once {
                    return;
                }
                once = false;
                if let Ok(f) = flows.single() {
                    commands
                        .brink_call_batch::<()>(
                            f,
                            [
                                ("add", vec![Value::Int(1)]),
                                ("nope", vec![]), // unknown fn — must not abort the batch
                                // A world-access query call, queued right after the
                                // failing slot: proves the failure didn't wedge the
                                // batch's shared SystemState/query access.
                                ("seen", vec![]),
                                ("add", vec![Value::Int(10)]),
                                ("get", vec![]),
                            ],
                        )
                        .observe(
                            |on: On<BrinkCallBatchResolved<()>>, mut out: ResMut<Result>| {
                                out.0.push(on.event().results.clone());
                            },
                        );
                }
            },
        );

        // Tick 1: the system issues brink_call_batch (spawns the request
        // entity + observer). Tick 2: the exclusive resolver evaluates the
        // whole batch in one VM-eval setup and fires BrinkCallBatchResolved
        // at the request entity; the observer records it.
        app.update();
        app.update();

        let out = &app.world().resource::<Result>().0;
        assert_eq!(out.len(), 1, "delivered exactly once");
        let results = &out[0];
        assert_eq!(results.len(), 5, "one slot per call, no drops");
        assert_eq!(results[0].as_ref().unwrap(), &Value::Int(1));
        assert!(
            results[1].is_err(),
            "the bad call fails in its own slot; got {:?}",
            results[1]
        );
        // The world-access query call right after the failing slot still
        // resolves against the World (2 enemies spawned above).
        assert_eq!(
            results[2].as_ref().unwrap(),
            &Value::Int(2),
            "a query-backed call still runs post-error"
        );
        // The failed call did not perturb `total`: the next add sees 1, not 0.
        assert_eq!(results[3].as_ref().unwrap(), &Value::Int(11));
        assert_eq!(results[4].as_ref().unwrap(), &Value::Int(11));
    }
}
