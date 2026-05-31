//! Deferred engine→ink calls for non-exclusive systems.
//!
//! [`call_ink_function`](crate::call_ink_function) needs `&mut World`, so a
//! normal system (which only has `Query`/`Res` access) can't use it
//! directly. Instead it *requests* a call via
//! [`brink_call`](BrinkCallCommandsExt::brink_call) and reacts to the
//! result with an observer scoped to a unique per-call entity:
//!
//! ```ignore
//! commands
//!     .brink_call::<()>(flow_entity, "can_player_advance", (in_combat,))
//!     .observe(|on: On<BrinkCallResolved>, mut moves: ResMut<PendingMoves>| {
//!         if on.event().value.as_bool() == Some(true) {
//!             moves.execute_queued();
//!         }
//!     });
//! ```
//!
//! Each `brink_call` spawns its own entity; the plugin's exclusive
//! [`resolve_brink_calls`] system evaluates the function (running any
//! world-access query bindings) and fires [`BrinkCallResolved`] /
//! [`BrinkCallFailed`] **targeted at that entity**, so the observer runs
//! exactly once and can never be confused with another call's result. The
//! call entity (and its observer) is despawned afterward.

use std::marker::PhantomData;

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::event::EntityEvent;
use bevy_ecs::system::{Commands, EntityCommands};
use bevy_ecs::world::World;
use brink_format::Value;

use crate::bindings::call_ink_function;

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

/// `Commands` extension for requesting a deferred engine→ink call.
pub trait BrinkCallCommandsExt {
    /// Request an ink function evaluation on `flow` (the flow entity),
    /// returning the [`EntityCommands`] of the spawned per-call entity so
    /// you can attach result observers:
    ///
    /// ```ignore
    /// commands.brink_call::<()>(flow, "can_spawn", ())
    ///     .observe(|on: On<BrinkCallResolved>| { /* use on.event().value */ });
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
}
