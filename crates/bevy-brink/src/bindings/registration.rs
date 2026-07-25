//! Registration API: how engine code declares callable ink→engine bindings.
//!
//! [`BrinkBindings`] is the registry `Resource`; [`BrinkBindingsAppExt`] is
//! the `App`-level surface authors call at app-build time
//! (`bind_brink_fn`/`bind_brink_command`/`bind_brink_query`/`bind_brink_async`/
//! `bind_brink_task`); [`BrinkHandler`] is the [`ExternalFnHandler`] built
//! from the registry and handed to a flow's step methods during normal
//! playback. See the parent module's docs (`crate::bindings`) for the
//! conceptual overview of the three synchronous binding kinds.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

use bevy_app::App;
use bevy_ecs::entity::Entity;
use bevy_ecs::event::Event;
#[cfg(feature = "effect-trace")]
use bevy_ecs::query::Access;
use bevy_ecs::resource::Resource;
#[cfg(feature = "effect-trace")]
use bevy_ecs::system::BoxedSystem;
use bevy_ecs::system::{Commands, In, IntoSystem, SystemId};
use bevy_ecs::world::World;
use bevy_log::warn;
use brink_format::Value;
use brink_runtime::{ExternalFnHandler, ExternalResult};
use thiserror::Error;

/// Input type for a world-access (`bind_brink_query`) binding system: the
/// flow entity that triggered the call, plus the ink arguments.
pub type BrinkQueryInput = (Entity, Vec<Value>);

/// The [`SystemId`] of a registered query binding — a Bevy system taking
/// [`BrinkQueryInput`] and returning a [`Value`].
///
/// `pub(crate)` so the drive/eval API (`super::drive`) can resolve a pending
/// query external's [`SystemId`] and run it via `run_system_with`.
pub(crate) type QuerySystemId = SystemId<In<BrinkQueryInput>, Value>;

/// Error produced when ink arguments can't be parsed into a binding's
/// expected shape. Returned by [`BrinkCommand::from_ink_args`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum BrinkArgError {
    /// Wrong number of arguments.
    #[error("expected {expected} argument(s), got {got}")]
    Count {
        /// How many arguments the binding declared.
        expected: usize,
        /// How many ink actually passed.
        got: usize,
    },
    /// An argument had the wrong runtime type.
    #[error("argument {index}: expected {expected}")]
    Type {
        /// Zero-based argument position.
        index: usize,
        /// The type the binding expected (e.g. `"int"`, `"string"`).
        expected: &'static str,
    },
}

/// A Bevy [`Event`] that can be built from an ink external call's
/// arguments, for use with [`bind_brink_command`](BrinkBindingsAppExt::bind_brink_command).
///
/// Implement (or `#[derive(BrinkCommand)]`) this for the event your
/// binding fires. The derive generates [`from_ink_args`](Self::from_ink_args)
/// for structs whose fields are `i32`, `f32`, `bool`, or `String`. To
/// return a value to ink, hand-implement the trait and override
/// [`reply`](Self::reply).
pub trait BrinkCommand: Sized {
    /// Parse the ink call's arguments (in declaration order) into `Self`.
    fn from_ink_args(args: &[Value]) -> Result<Self, BrinkArgError>;

    /// The value handed back to ink as this external's return value.
    ///
    /// Defaults to [`Value::Null`] — the natural "fire-and-forget, no
    /// return" behavior. Override to feed a computed value back into the
    /// story (e.g. a dice roll).
    fn reply(&self) -> Value {
        Value::Null
    }
}

// Type aliases for the boxed registry entries.
type PureFn = Box<dyn Fn(&[Value]) -> Value + Send + Sync>;
type CommandFn = Box<dyn Fn(&[Value]) -> Result<QueuedCommand, BrinkArgError> + Send + Sync>;
/// Factory for a [`bind_brink_task`](BrinkBindingsAppExt::bind_brink_task)
/// future: given the ink args, produce a boxed `Send + 'static` future that
/// computes the external's return value off the main thread.
///
/// `pub(crate)` so the drive/eval API (`super::drive`) can spawn the factory's
/// future when a parked flow's pending external is a `bind_brink_task`.
pub(crate) type TaskFn =
    Box<dyn Fn(Vec<Value>) -> Pin<Box<dyn Future<Output = Value> + Send>> + Send + Sync>;

/// How an async (defer-across-frames) external resolves once a flow parks on
/// it. Stored in [`BrinkBindings::async_bindings`].
///
/// `pub(crate)` so `super::drive`'s dispatcher can match on the binding kind
/// (fire an event vs. spawn a task) for a parked flow's pending external.
pub(crate) enum AsyncKind {
    /// `bind_brink_async`: fire `BrinkExternalAwaited` and wait for the
    /// engine to call `resolve_brink_external`.
    Event,
    /// `bind_brink_task`: spawn the future on the async task pool and resolve
    /// with its output when it completes.
    Task(TaskFn),
}
/// A deferred World mutation that triggers a parsed command event. Boxed
/// so heterogeneous command types share one buffer; run during flush.
///
/// `pub(crate)` so the batch driver ([`crate::batch`]) can hold a flow's
/// buffered command triggers across the batch's Step phase and replay them
/// in deterministic flow-id order at Apply (`docs/effects-spec.md` §12.4),
/// rather than flushing each flow's commands immediately as the serial API
/// does.
pub(crate) type TriggerFn = Box<dyn FnOnce(&mut World) + Send>;

/// A parsed command ready to be triggered against the World, plus the
/// value to return to ink.
struct QueuedCommand {
    /// Triggers the parsed event when run against the World.
    trigger: TriggerFn,
    /// Value returned to ink (usually [`Value::Null`]).
    reply: Value,
}

/// Registry of synchronous ink→engine bindings for story marker `M`.
///
/// A `Resource`. Populate it at app-build time with
/// [`bind_brink_fn`](BrinkBindingsAppExt::bind_brink_fn) and
/// [`bind_brink_command`](BrinkBindingsAppExt::bind_brink_command), then,
/// in the flow-driving system, call [`handler`](Self::handler) to get a
/// [`BrinkHandler`] to pass to the flow's step methods.
#[derive(Resource)]
pub struct BrinkBindings<M: Send + Sync + 'static = ()> {
    pure: HashMap<String, PureFn>,
    commands: HashMap<String, CommandFn>,
    queries: HashMap<String, QuerySystemId>,
    /// Each query binding's real, bevy-declared [`Access`], captured once at
    /// `bind_brink_query` registration time (issue #938's host-side
    /// ground-truth check — see `crate::ground_truth`'s module docs for why
    /// registration time is the right moment: bevy's own component access
    /// is static, so it never varies dispatch to dispatch).
    #[cfg(feature = "effect-trace")]
    query_access: HashMap<String, Access>,
    /// Async (defer-across-frames) bindings: `bind_brink_async` (event) and
    /// `bind_brink_task` (detached task). A flow pauses on these and resolves
    /// out-of-band.
    ///
    /// `pub(crate)` so `super::drive`'s dispatcher can read a pending
    /// external's binding kind directly.
    pub(crate) async_bindings: HashMap<String, AsyncKind>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for BrinkBindings<M> {
    fn default() -> Self {
        Self {
            pure: HashMap::new(),
            commands: HashMap::new(),
            queries: HashMap::new(),
            #[cfg(feature = "effect-trace")]
            query_access: HashMap::new(),
            async_bindings: HashMap::new(),
            _marker: PhantomData,
        }
    }
}

impl<M: Send + Sync + 'static> BrinkBindings<M> {
    /// Build a [`BrinkHandler`] borrowing this registry. Pass `&handler`
    /// to a flow's step method, then call [`BrinkHandler::flush`] to emit
    /// any buffered command events.
    ///
    /// Query bindings (which need World access) yield
    /// [`ExternalResult::Pending`] so a flow pauses on them; the plugin's
    /// resolver (or [`advance_flow`](crate::advance_flow)) runs the query
    /// against the World and resumes.
    #[must_use]
    pub fn handler(&self) -> BrinkHandler<'_, M> {
        BrinkHandler {
            bindings: self,
            queued: RefCell::new(Vec::new()),
        }
    }

    /// The [`SystemId`] of the query binding registered under `name`, if any.
    ///
    /// `pub(crate)` so `super::drive` can resolve a pending query external's
    /// system to run via `run_system_with`.
    pub(crate) fn query(&self, name: &str) -> Option<QuerySystemId> {
        self.queries.get(name).copied()
    }

    /// The real bevy [`Access`] a `bind_brink_query` binding's system
    /// carries, if `name` was registered while the `effect-trace` feature
    /// was enabled (see [`crate::ground_truth`]'s `check` function).
    #[cfg(feature = "effect-trace")]
    pub(crate) fn query_access(&self, name: &str) -> Option<&Access> {
        self.query_access.get(name)
    }
}

/// An [`ExternalFnHandler`] backed by a [`BrinkBindings`] registry.
///
/// Resolves pure-function bindings inline and buffers command-event
/// triggers (it has no World access mid-step). After stepping, call
/// [`flush`](Self::flush) to drain the buffered triggers into a
/// [`Commands`] queue. Unknown names fall through to
/// [`ExternalResult::Fallback`] so the in-story fallback body (if any)
/// runs.
pub struct BrinkHandler<'a, M: Send + Sync + 'static = ()> {
    bindings: &'a BrinkBindings<M>,
    /// `pub(crate)` so the `bindings::tests` module can assert on the raw
    /// buffer in cases where `queued_len`/`take_queued` don't fit (draining
    /// it directly to apply the triggers).
    pub(crate) queued: RefCell<Vec<TriggerFn>>,
}

impl<M: Send + Sync + 'static> BrinkHandler<'_, M> {
    /// Drain buffered command-event triggers into `commands`. Call once
    /// after the flow's step method returns (the borrow of `self` taken
    /// by stepping has ended by then). Consumes the handler.
    pub fn flush(self, commands: &mut Commands) {
        for trigger in self.queued.into_inner() {
            commands.queue(trigger);
        }
    }

    /// Take the buffered command-event triggers, leaving the handler empty.
    /// Used by [`advance_flow`](crate::advance_flow) to accumulate triggers
    /// across the suspensions of a single line, and by the batch driver
    /// ([`crate::batch`]) to move a flow's buffered command triggers into
    /// its per-flow batch outcome for deterministic flow-id-ordered replay
    /// at Apply.
    pub(crate) fn take_queued(&self) -> Vec<TriggerFn> {
        std::mem::take(&mut self.queued.borrow_mut())
    }

    /// Number of command triggers buffered so far (for tests/diagnostics).
    #[must_use]
    pub fn queued_len(&self) -> usize {
        self.queued.borrow().len()
    }
}

impl<M: Send + Sync + 'static> ExternalFnHandler for BrinkHandler<'_, M> {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
        if let Some(f) = self.bindings.pure.get(name) {
            return ExternalResult::Resolved(f(args));
        }
        if let Some(parse) = self.bindings.commands.get(name) {
            return match parse(args) {
                Ok(queued) => {
                    self.queued.borrow_mut().push(queued.trigger);
                    ExternalResult::Resolved(queued.reply)
                }
                Err(err) => {
                    warn!("brink command '{name}': {err}; emitting nothing, returning null");
                    ExternalResult::Resolved(Value::Null)
                }
            };
        }
        if self.bindings.queries.contains_key(name)
            || self.bindings.async_bindings.contains_key(name)
        {
            // World-access query or async (defer-across-frames) binding —
            // pause so the driver/resolver can run the query against the World
            // (sync) or hand off to the engine/task pool (async), then resume.
            return ExternalResult::Pending;
        }
        ExternalResult::Fallback
    }
}

impl<M: Send + Sync + 'static> BrinkBindings<M> {
    /// Build an [`EvalHandler`] for an engine→ink call. Pure bindings
    /// resolve inline; query bindings yield
    /// [`ExternalResult::Pending`] so the exclusive driver
    /// (`super::drive::call_ink_function`) can run them against the World
    /// between suspensions; everything else falls back to the in-story body.
    ///
    /// `pub(crate)` so `super::drive`'s exclusive eval driver can build one.
    pub(crate) fn eval_handler(&self) -> EvalHandler<'_, M> {
        EvalHandler { bindings: self }
    }
}

/// Handler used while evaluating an ink function from engine code
/// (`super::drive::call_ink_function`). Unlike [`BrinkHandler`], it cannot
/// buffer commands or touch the World — it only resolves pure bindings inline
/// and defers world-access (query) bindings to the driver via
/// [`ExternalResult::Pending`].
///
/// `pub(crate)` — this is `super::drive::call_ink_function`'s handler type,
/// returned from [`BrinkBindings::eval_handler`].
pub(crate) struct EvalHandler<'a, M: Send + Sync + 'static> {
    bindings: &'a BrinkBindings<M>,
}

impl<M: Send + Sync + 'static> ExternalFnHandler for EvalHandler<'_, M> {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
        if let Some(f) = self.bindings.pure.get(name) {
            return ExternalResult::Resolved(f(args));
        }
        if self.bindings.queries.contains_key(name)
            || self.bindings.async_bindings.contains_key(name)
        {
            // World-access query (resolved between suspensions) or async
            // binding (unsupported in the one-pass engine→ink driver — the
            // driver maps it to AsyncExternalUnsupported). Pause either way.
            return ExternalResult::Pending;
        }
        ExternalResult::Fallback
    }
}

/// App-extension verbs for registering synchronous ink→engine bindings.
///
/// Both verbs take the story marker `M` as the first explicit type
/// parameter (use `()` for the default single-story case). They insert
/// into the [`BrinkBindings<M>`] resource, creating it on first use.
pub trait BrinkBindingsAppExt {
    /// Register a **pure** binding: a side-effect-free function of the ink
    /// arguments that returns a value to the story. Resolved inline while
    /// the VM steps — no World access, no latency.
    ///
    /// The return type is anything `Into<Value>`, so primitives work
    /// directly: `|args| 1.5_f32`, `|args| count as i32`, etc.
    fn bind_brink_fn<M, F, R>(&mut self, name: impl Into<String>, f: F) -> &mut Self
    where
        M: Send + Sync + 'static,
        F: Fn(&[Value]) -> R + Send + Sync + 'static,
        R: Into<Value>;

    /// Register a **command** binding: parse the ink arguments into a Bevy
    /// [`Event`] and trigger it (fire-and-forget). The event is buffered
    /// during stepping and emitted when the handler is flushed. The story
    /// receives [`BrinkCommand::reply`] as the call's return value
    /// (`Value::Null` by default).
    ///
    /// `E` should be a plain `#[derive(Event)]` (a global observer event):
    /// react to it with `app.add_observer(|on: On<E>| { … })`.
    fn bind_brink_command<M, E>(&mut self, name: impl Into<String>) -> &mut Self
    where
        M: Send + Sync + 'static,
        E: Event + BrinkCommand,
        for<'a> <E as Event>::Trigger<'a>: Default;

    /// Register a **query** binding: a Bevy system with arbitrary
    /// `SystemParam`s that reads the World and returns a [`Value`] to the
    /// story. The system takes [`BrinkQueryInput`] — the flow [`Entity`]
    /// that triggered the call plus the ink arguments.
    ///
    /// Resolving a query needs World access, so it can't run inline while
    /// the VM steps. Engine→ink calls (`super::drive::call_ink_function`)
    /// drive it via `run_system_with` between suspensions; the binding can
    /// therefore query anything in the World, with no upfront declaration.
    ///
    /// ```ignore
    /// fn enemy_count(In((_e, _args)): In<BrinkQueryInput>, q: Query<&Enemy>) -> Value {
    ///     Value::Int(q.iter().count() as i32)
    /// }
    /// app.bind_brink_query::<(), _, _>("enemy_count", enemy_count);
    /// ```
    fn bind_brink_query<M, S, SM>(&mut self, name: impl Into<String>, system: S) -> &mut Self
    where
        M: Send + Sync + 'static,
        S: IntoSystem<In<BrinkQueryInput>, Value, SM> + 'static;

    /// Register an **async (event) primitive** binding: when ink calls the
    /// external, the flow *parks* and
    /// [`BrinkExternalAwaited`](crate::BrinkExternalAwaited) fires (once) at
    /// the flow entity. The engine does whatever multi-frame work the external
    /// represents (UI, input, world state) and resolves with
    /// [`resolve_brink_external`](crate::BrinkResolveExternalExt::resolve_brink_external).
    /// Use this when the value can't be produced in one pass and needs World
    /// access over several frames.
    ///
    /// Only usable on the `step_one` playback path — the one-pass exclusive
    /// drivers (`super::drive::advance_flow`/`super::drive::call_ink_function`)
    /// return [`BrinkCallError::AsyncExternalUnsupported`](crate::BrinkCallError::AsyncExternalUnsupported)
    /// on an async external.
    fn bind_brink_async<M>(&mut self, name: impl Into<String>) -> &mut Self
    where
        M: Send + Sync + 'static;

    /// Register an **async task** binding: when ink calls the external,
    /// bevy-brink spawns `f(args)` on [`bevy_tasks::AsyncComputeTaskPool`] and
    /// resolves the flow's external with the future's output once it completes
    /// (polled each frame by [`poll_brink_tasks`](crate::poll_brink_tasks)).
    ///
    /// The future is `Send + 'static` and runs off the main thread, so it
    /// **cannot access the World** — it computes from the ink arguments only
    /// (heavy compute, IO, network). For World-dependent async, use
    /// [`bind_brink_async`](Self::bind_brink_async).
    ///
    /// ```ignore
    /// app.bind_brink_task::<(), _, _>("expensive_roll", |args| async move {
    ///     let n = args.first().and_then(Value::as_int).unwrap_or(1);
    ///     Value::Int(compute_roll(n).await)
    /// });
    /// ```
    fn bind_brink_task<M, F, Fut>(&mut self, name: impl Into<String>, f: F) -> &mut Self
    where
        M: Send + Sync + 'static,
        F: Fn(Vec<Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Value> + Send + 'static;
}

impl BrinkBindingsAppExt for App {
    fn bind_brink_fn<M, F, R>(&mut self, name: impl Into<String>, f: F) -> &mut Self
    where
        M: Send + Sync + 'static,
        F: Fn(&[Value]) -> R + Send + Sync + 'static,
        R: Into<Value>,
    {
        let name = name.into();
        {
            let mut reg = self
                .world_mut()
                .get_resource_or_insert_with(BrinkBindings::<M>::default);
            reg.pure.insert(name, Box::new(move |args| f(args).into()));
        }
        self
    }

    fn bind_brink_command<M, E>(&mut self, name: impl Into<String>) -> &mut Self
    where
        M: Send + Sync + 'static,
        E: Event + BrinkCommand,
        for<'a> <E as Event>::Trigger<'a>: Default,
    {
        let name = name.into();
        {
            let mut reg = self
                .world_mut()
                .get_resource_or_insert_with(BrinkBindings::<M>::default);
            reg.commands.insert(
                name,
                Box::new(move |args: &[Value]| {
                    let event = E::from_ink_args(args)?;
                    let reply = event.reply();
                    Ok(QueuedCommand {
                        trigger: Box::new(move |world: &mut World| {
                            world.trigger(event);
                        }),
                        reply,
                    })
                }),
            );
        }
        self
    }

    fn bind_brink_query<M, S, SM>(&mut self, name: impl Into<String>, system: S) -> &mut Self
    where
        M: Send + Sync + 'static,
        S: IntoSystem<In<BrinkQueryInput>, Value, SM> + 'static,
    {
        let name = name.into();
        // With `effect-trace`, capture the system's real bevy-declared
        // `Access` once, at registration — bevy's own component access is
        // static (a `Query<&Foo>` declares the same access whether or not it
        // ever matches an entity), so this is the exact ground truth
        // `crate::ground_truth::check` later compares real dispatches
        // against. `register_boxed_system` (rather than `register_system`)
        // lets us call `System::initialize` ourselves first without needing
        // `S: Clone`; the already-initialized boxed system is then handed to
        // bevy exactly as `register_system` would have built it, so the
        // registered binding behaves identically either way.
        #[cfg(feature = "effect-trace")]
        let (id, access) = {
            let mut boxed: BoxedSystem<In<BrinkQueryInput>, Value> =
                Box::new(IntoSystem::into_system(system));
            let access = boxed.initialize(self.world_mut()).combined_access().clone();
            let id = self.world_mut().register_boxed_system(boxed);
            (id, access)
        };
        #[cfg(not(feature = "effect-trace"))]
        let id = self.world_mut().register_system(system);
        {
            let mut reg = self
                .world_mut()
                .get_resource_or_insert_with(BrinkBindings::<M>::default);
            #[cfg(feature = "effect-trace")]
            reg.query_access.insert(name.clone(), access);
            reg.queries.insert(name, id);
        }
        self
    }

    fn bind_brink_async<M>(&mut self, name: impl Into<String>) -> &mut Self
    where
        M: Send + Sync + 'static,
    {
        let name = name.into();
        {
            let mut reg = self
                .world_mut()
                .get_resource_or_insert_with(BrinkBindings::<M>::default);
            reg.async_bindings.insert(name, AsyncKind::Event);
        }
        self
    }

    fn bind_brink_task<M, F, Fut>(&mut self, name: impl Into<String>, f: F) -> &mut Self
    where
        M: Send + Sync + 'static,
        F: Fn(Vec<Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Value> + Send + 'static,
    {
        let name = name.into();
        let factory: TaskFn = Box::new(move |args| Box::pin(f(args)));
        {
            let mut reg = self
                .world_mut()
                .get_resource_or_insert_with(BrinkBindings::<M>::default);
            reg.async_bindings.insert(name, AsyncKind::Task(factory));
        }
        self
    }
}
