//! Ink → engine external-function bindings (synchronous kinds).
//!
//! When an ink story calls an `EXTERNAL` function, the runtime asks an
//! [`ExternalFnHandler`] how to resolve it. This module provides a
//! registry-backed handler ([`BrinkHandler`]) plus app-level registration
//! verbs for the two *synchronous* binding kinds:
//!
//! - **`bind_brink_fn`** — a pure function `Fn(&[Value]) -> impl Into<Value>`.
//!   No World access; resolved inline while the VM steps. Use for math,
//!   formatting, table lookups against captured data.
//! - **`bind_brink_command`** — fire-and-forget: parse the ink args into a
//!   Bevy [`Event`] and trigger it. The event is *buffered* during stepping
//!   (the handler can't touch the World mid-step) and flushed afterward via
//!   [`BrinkHandler::flush`]. Optionally returns a value to ink via
//!   [`BrinkCommand::reply`].
//!
//! plus a third, *world-access* kind used by engine→ink calls:
//!
//! - **`bind_brink_query`** — a Bevy system with arbitrary `SystemParam`s
//!   that reads the World and returns a [`Value`]. It can't run inline
//!   while the VM steps, so [`call_ink_function`] (an exclusive-`&mut World`
//!   driver) runs it via `run_system_with` between evaluation suspensions —
//!   letting an ink function called from the engine query anything in the
//!   World, with no upfront declaration.
//!
//! ## Wiring it up
//!
//! ```ignore
//! app.bind_brink_fn::<(), _, _>("clamp01", |args| {
//!     args.first().and_then(Value::as_float).unwrap_or(0.0).clamp(0.0, 1.0)
//! });
//! app.bind_brink_command::<(), PlaySound>("play_sound");
//! ```
//!
//! Then, in the system that drives flows, build a handler from the
//! registry, step the flow with it, and flush:
//!
//! ```ignore
//! let handler = bindings.handler();
//! let line = flow.step_one(program, tables, &mut ctx.inner, &handler, entity, &mut commands)?;
//! handler.flush(&mut commands);
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;

use bevy_app::App;
use bevy_asset::Assets;
use bevy_ecs::entity::Entity;
use bevy_ecs::event::Event;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, In, IntoSystem, Query, Res, SystemId, SystemState};
use bevy_ecs::world::World;
use bevy_log::warn;
use brink_format::Value;
use brink_runtime::{
    ExternalFnHandler, ExternalResult, FastRng, FlowInstance, Line, Program, RuntimeError,
    StepOutcome,
};
use thiserror::Error;

use crate::asset::{BrinkProgram, LineTablesAsset, ProgramAsset};
use crate::flow::BrinkFlow;
use crate::globals::BrinkContext;
use crate::line_tables::BrinkLocale;

/// Input type for a world-access (`bind_brink_query`) binding system: the
/// flow entity that triggered the call, plus the ink arguments.
pub type BrinkQueryInput = (Entity, Vec<Value>);

/// The [`SystemId`] of a registered query binding — a Bevy system taking
/// [`BrinkQueryInput`] and returning a [`Value`].
type QuerySystemId = SystemId<In<BrinkQueryInput>, Value>;

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
/// A deferred World mutation that triggers a parsed command event. Boxed
/// so heterogeneous command types share one buffer; run during flush.
type TriggerFn = Box<dyn FnOnce(&mut World) + Send>;

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
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for BrinkBindings<M> {
    fn default() -> Self {
        Self {
            pure: HashMap::new(),
            commands: HashMap::new(),
            queries: HashMap::new(),
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
    /// resolver (or [`advance_flow`]) runs the query against the World and
    /// resumes.
    #[must_use]
    pub fn handler(&self) -> BrinkHandler<'_, M> {
        BrinkHandler {
            bindings: self,
            queued: RefCell::new(Vec::new()),
        }
    }

    /// The [`SystemId`] of the query binding registered under `name`, if any.
    fn query(&self, name: &str) -> Option<QuerySystemId> {
        self.queries.get(name).copied()
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
    queued: RefCell<Vec<TriggerFn>>,
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
    /// Used by [`advance_flow`] to accumulate triggers across the
    /// suspensions of a single line and flush them against the World.
    fn take_queued(&self) -> Vec<TriggerFn> {
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
        if self.bindings.queries.contains_key(name) {
            // World-access binding — pause so the driver/resolver can run
            // it against the World, then resume.
            return ExternalResult::Pending;
        }
        ExternalResult::Fallback
    }
}

impl<M: Send + Sync + 'static> BrinkBindings<M> {
    /// Build an [`EvalHandler`] for an engine→ink call. Pure bindings
    /// resolve inline; query bindings yield
    /// [`ExternalResult::Pending`] so the exclusive driver
    /// ([`call_ink_function`]) can run them against the World between
    /// suspensions; everything else falls back to the in-story body.
    fn eval_handler(&self) -> EvalHandler<'_, M> {
        EvalHandler { bindings: self }
    }
}

/// Handler used while evaluating an ink function from engine code
/// ([`call_ink_function`]). Unlike [`BrinkHandler`], it cannot buffer
/// commands or touch the World — it only resolves pure bindings inline and
/// defers world-access (query) bindings to the driver via
/// [`ExternalResult::Pending`].
struct EvalHandler<'a, M: Send + Sync + 'static> {
    bindings: &'a BrinkBindings<M>,
}

impl<M: Send + Sync + 'static> ExternalFnHandler for EvalHandler<'_, M> {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
        if let Some(f) = self.bindings.pure.get(name) {
            return ExternalResult::Resolved(f(args));
        }
        if self.bindings.queries.contains_key(name) {
            // World-access binding — the driver resolves it between
            // suspensions, where it can borrow the World.
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
    /// the VM steps. Engine→ink calls ([`call_ink_function`]) drive it via
    /// `run_system_with` between suspensions; the binding can therefore
    /// query anything in the World, with no upfront declaration.
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
        let id = self.world_mut().register_system(system);
        {
            let mut reg = self
                .world_mut()
                .get_resource_or_insert_with(BrinkBindings::<M>::default);
            reg.queries.insert(name, id);
        }
        self
    }
}

/// Errors from an engine→ink call ([`call_ink_function`]).
#[derive(Debug, Error)]
pub enum BrinkCallError {
    /// The entity isn't a fulfilled flow (missing `BrinkFlow`/`BrinkProgram`/
    /// `BrinkLocale`/`BrinkContext`).
    #[error("entity is not a fulfilled brink flow")]
    NotAFlow,
    /// The flow's program asset isn't loaded.
    #[error("program asset not loaded")]
    ProgramNotLoaded,
    /// The flow's line-tables asset isn't loaded.
    #[error("line tables asset not loaded")]
    LineTablesNotLoaded,
    /// No function with this name exists in the program.
    #[error("function '{0}' not found")]
    FunctionNotFound(String),
    /// The function called a world-access external with no registered
    /// query binding (and no in-story fallback).
    #[error("no query binding registered for external '{0}'")]
    UnknownQuery(String),
    /// A query binding's system failed to run.
    #[error("query binding system failed: {0}")]
    QueryFailed(String),
    /// The runtime raised an error during evaluation.
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
}

/// The next thing the [`call_ink_function`] driver must do.
enum NextStep {
    /// The function returned this value — evaluation is complete.
    Done(Value),
    /// The function is awaiting a world-access query; run this system with
    /// the given ink args, resolve, and resume.
    RunQuery {
        system: QuerySystemId,
        qargs: Vec<Value>,
    },
}

/// Classify a [`FunctionEval`] outcome into the driver's [`NextStep`],
/// looking up the query system for a pending external. Called inside the
/// borrow scope where `flow`/`program`/`bindings` are available.
fn classify_eval<M: Send + Sync + 'static>(
    flow: &FlowInstance,
    program: &Program,
    bindings: &BrinkBindings<M>,
    outcome: brink_runtime::FunctionEval,
) -> Result<NextStep, BrinkCallError> {
    match outcome {
        brink_runtime::FunctionEval::Returned(value) => Ok(NextStep::Done(value)),
        brink_runtime::FunctionEval::AwaitingExternal => {
            let name = flow
                .pending_external_name(program)
                .unwrap_or_default()
                .to_owned();
            let system = bindings
                .query(&name)
                .ok_or(BrinkCallError::UnknownQuery(name))?;
            let qargs = flow.pending_external_args().to_vec();
            Ok(NextStep::RunQuery { system, qargs })
        }
    }
}

/// Synchronously evaluate an ink function on a flow entity from an
/// exclusive (`&mut World`) context, returning its value.
///
/// Pure bindings resolve inline; world-access (`bind_brink_query`) bindings
/// are run via `run_system_with` between evaluation suspensions — so the
/// function can query anything in the World. The whole call completes in
/// one pass (one frame): the function's output is isolated, the
/// player-visible story is untouched, and visit counts aren't bumped.
///
/// `M` is the story marker (use `()` for the default). For callers that
/// don't have `&mut World` (a normal system), use the deferred
/// `commands.brink_call(...)` API instead.
///
/// # Errors
/// See [`BrinkCallError`].
pub fn call_ink_function<M: Send + Sync + 'static>(
    world: &mut World,
    entity: Entity,
    name: &str,
    args: &[Value],
) -> Result<Value, BrinkCallError> {
    #[expect(
        clippy::type_complexity,
        reason = "SystemState param tuple for the flow components + assets + bindings"
    )]
    let mut state: SystemState<(
        Query<(
            &BrinkProgram<M>,
            &BrinkLocale<M>,
            &mut BrinkFlow<M>,
            &mut BrinkContext<M>,
        )>,
        Res<Assets<ProgramAsset>>,
        Res<Assets<LineTablesAsset>>,
        Res<BrinkBindings<M>>,
    )> = SystemState::new(world);

    // Begin the evaluation.
    let mut next = {
        let (mut flows, programs, tables, bindings) = state.get_mut(world);
        let (prog_c, loc_c, mut flow, mut ctx) = flows
            .get_mut(entity)
            .map_err(|_| BrinkCallError::NotAFlow)?;
        let program = &programs
            .get(&prog_c.handle)
            .ok_or(BrinkCallError::ProgramNotLoaded)?
            .program;
        let line_tables = &tables
            .get(&loc_c.handle)
            .ok_or(BrinkCallError::LineTablesNotLoaded)?
            .tables;
        let idx = program
            .find_address(name)
            .ok_or_else(|| BrinkCallError::FunctionNotFound(name.to_owned()))?
            .0;
        let handler = bindings.eval_handler();
        let outcome = flow.inner.begin_function_eval::<FastRng>(
            program,
            line_tables,
            &mut ctx.inner,
            &handler,
            idx,
            args,
            None,
        )?;
        classify_eval(&flow.inner, program, &bindings, outcome)?
    };

    // Drive: run each pending world-access query against the World (borrows
    // released here), resolve it, and resume — until the function returns.
    loop {
        match next {
            NextStep::Done(value) => return Ok(value),
            NextStep::RunQuery { system, qargs } => {
                let value = world
                    .run_system_with(system, (entity, qargs))
                    .map_err(|e| BrinkCallError::QueryFailed(format!("{e:?}")))?;
                next = {
                    let (mut flows, programs, tables, bindings) = state.get_mut(world);
                    let (prog_c, loc_c, mut flow, mut ctx) = flows
                        .get_mut(entity)
                        .map_err(|_| BrinkCallError::NotAFlow)?;
                    let program = &programs
                        .get(&prog_c.handle)
                        .ok_or(BrinkCallError::ProgramNotLoaded)?
                        .program;
                    let line_tables = &tables
                        .get(&loc_c.handle)
                        .ok_or(BrinkCallError::LineTablesNotLoaded)?
                        .tables;
                    let handler = bindings.eval_handler();
                    flow.inner.resolve_external(value);
                    let outcome = flow.inner.resume_function_eval::<FastRng>(
                        program,
                        line_tables,
                        &mut ctx.inner,
                        &handler,
                        None,
                    )?;
                    classify_eval(&flow.inner, program, &bindings, outcome)?
                };
            }
        }
    }
}

/// One step of the [`advance_flow`] loop, captured inside the borrow scope
/// so the World can be re-borrowed (for `run_system_with`) afterward.
enum FlowStep {
    /// A line was produced.
    Line(Line),
    /// The flow paused on a world-access query; run this system then resume.
    Query {
        system: QuerySystemId,
        qargs: Vec<Value>,
    },
}

/// Fire the per-line observer event (matching [`step_one`](crate::BrinkFlow::step_one))
/// from an exclusive `&mut World` context.
fn emit_line_event_world<M: Send + Sync + 'static>(world: &mut World, entity: Entity, line: &Line) {
    use crate::event::{BrinkChoicesPresented, BrinkLineDelivered, BrinkStoryEnded, BrinkTurnDone};
    match line {
        Line::Text { text, tags } => {
            world
                .entity_mut(entity)
                .trigger(|e| BrinkLineDelivered::<M>::new(e, text.clone(), tags.clone()));
        }
        Line::Choices {
            text,
            tags,
            choices,
        } => {
            world.entity_mut(entity).trigger(|e| {
                BrinkChoicesPresented::<M>::new(e, text.clone(), tags.clone(), choices.clone())
            });
        }
        Line::Done { text, tags } => {
            world
                .entity_mut(entity)
                .trigger(|e| BrinkTurnDone::<M>::new(e, text.clone(), tags.clone()));
        }
        Line::End { text, tags } => {
            world
                .entity_mut(entity)
                .trigger(|e| BrinkStoryEnded::<M>::new(e, text.clone(), tags.clone()));
        }
    }
}

/// Advance a flow by one line from an exclusive (`&mut World`) context,
/// resolving any world-access query bindings inline via `run_system_with`.
///
/// This is the playback counterpart to [`call_ink_function`]: where a
/// non-exclusive `step_one` can only resolve pure/command bindings (query
/// bindings fall back), `advance_flow` runs the query binding's system
/// between the runtime's eval suspensions — so a story line like
/// `{enemy_count()}` resolves transparently in one frame. Buffered command
/// events are flushed and the line's observer event is fired, exactly as
/// `step_one` would.
///
/// # Errors
/// See [`BrinkCallError`].
pub fn advance_flow<M: Send + Sync + 'static>(
    world: &mut World,
    entity: Entity,
) -> Result<Line, BrinkCallError> {
    #[expect(
        clippy::type_complexity,
        reason = "SystemState param tuple for the flow components + assets + bindings"
    )]
    let mut state: SystemState<(
        Query<(
            &BrinkProgram<M>,
            &BrinkLocale<M>,
            &mut BrinkFlow<M>,
            &mut BrinkContext<M>,
        )>,
        Res<Assets<ProgramAsset>>,
        Res<Assets<LineTablesAsset>>,
        Res<BrinkBindings<M>>,
    )> = SystemState::new(world);

    // Command-event triggers accumulate across the suspensions of a single
    // line, then flush once the line is produced.
    let mut triggers: Vec<TriggerFn> = Vec::new();

    loop {
        let step = {
            let (mut flows, programs, tables, bindings) = state.get_mut(world);
            let (prog_c, loc_c, mut flow, mut ctx) = flows
                .get_mut(entity)
                .map_err(|_| BrinkCallError::NotAFlow)?;
            let program = &programs
                .get(&prog_c.handle)
                .ok_or(BrinkCallError::ProgramNotLoaded)?
                .program;
            let line_tables = &tables
                .get(&loc_c.handle)
                .ok_or(BrinkCallError::LineTablesNotLoaded)?
                .tables;
            let handler = bindings.handler();
            let outcome = flow.inner.advance::<FastRng>(
                program,
                line_tables,
                &mut ctx.inner,
                &handler,
                None,
            )?;
            triggers.extend(handler.take_queued());
            match outcome {
                StepOutcome::Line(line) => FlowStep::Line(line),
                StepOutcome::AwaitingExternal => {
                    let name = flow
                        .inner
                        .pending_external_name(program)
                        .unwrap_or_default()
                        .to_owned();
                    let system = bindings
                        .query(&name)
                        .ok_or(BrinkCallError::UnknownQuery(name))?;
                    let qargs = flow.inner.pending_external_args().to_vec();
                    FlowStep::Query { system, qargs }
                }
            }
        };

        match step {
            FlowStep::Line(line) => {
                for trigger in triggers {
                    trigger(world);
                }
                emit_line_event_world::<M>(world, entity, &line);
                return Ok(line);
            }
            FlowStep::Query { system, qargs } => {
                let value = world
                    .run_system_with(system, (entity, qargs))
                    .map_err(|e| BrinkCallError::QueryFailed(format!("{e:?}")))?;
                let (mut flows, ..) = state.get_mut(world);
                let (_, _, mut flow, _) = flows
                    .get_mut(entity)
                    .map_err(|_| BrinkCallError::NotAFlow)?;
                flow.inner.resolve_external(value);
            }
        }
    }
}

/// If the flow at `entity` is paused on a world-access query binding, run
/// the query's system and resolve the external. Returns `Ok(true)` if a
/// query was resolved, `Ok(false)` if the flow had no pending external.
fn resolve_one_query<M: Send + Sync + 'static>(
    world: &mut World,
    entity: Entity,
) -> Result<bool, BrinkCallError> {
    #[expect(
        clippy::type_complexity,
        reason = "SystemState param tuple for the flow component + assets + bindings"
    )]
    let (system, qargs) = {
        let mut state: SystemState<(
            Query<(&BrinkProgram<M>, &BrinkFlow<M>)>,
            Res<Assets<ProgramAsset>>,
            Res<BrinkBindings<M>>,
        )> = SystemState::new(world);
        let (flows, programs, bindings) = state.get(world);
        let Ok((prog_c, flow)) = flows.get(entity) else {
            return Ok(false);
        };
        if !flow.inner.has_pending_external() {
            return Ok(false);
        }
        let program = &programs
            .get(&prog_c.handle)
            .ok_or(BrinkCallError::ProgramNotLoaded)?
            .program;
        let name = flow
            .inner
            .pending_external_name(program)
            .unwrap_or_default()
            .to_owned();
        let system = bindings
            .query(&name)
            .ok_or(BrinkCallError::UnknownQuery(name))?;
        let qargs = flow.inner.pending_external_args().to_vec();
        (system, qargs)
    };

    let value = world
        .run_system_with(system, (entity, qargs))
        .map_err(|e| BrinkCallError::QueryFailed(format!("{e:?}")))?;
    let mut flows = world.query::<&mut BrinkFlow<M>>();
    if let Ok(mut flow) = flows.get_mut(world, entity) {
        flow.inner.resolve_external(value);
    }
    Ok(true)
}

/// Run condition: `true` if any `BrinkFlow<M>` is paused on a pending
/// external (so the resolver only runs when there's work).
#[must_use]
pub fn any_flow_awaiting_external<M: Send + Sync + 'static>(flows: Query<&BrinkFlow<M>>) -> bool {
    flows.iter().any(|f| f.inner.has_pending_external())
}

/// Exclusive plugin system: resolve world-access query bindings for flows
/// that paused during normal playback (after a non-exclusive
/// [`step_one`](crate::BrinkFlow::step_one) yielded
/// [`Advance::AwaitingQuery`](crate::Advance::AwaitingQuery)). Runs each
/// pending query's system and resolves the external; the flow resumes on
/// the next `step_one`. Registered by the plugin, gated on
/// [`any_flow_awaiting_external`].
pub fn resolve_pending_queries<M: Send + Sync + 'static>(world: &mut World) {
    let paused: Vec<Entity> = {
        let mut flows = world.query::<(Entity, &BrinkFlow<M>)>();
        flows
            .iter(world)
            .filter(|(_, f)| f.inner.has_pending_external())
            .map(|(e, _)| e)
            .collect()
    };
    for entity in paused {
        if let Err(err) = resolve_one_query::<M>(world, entity) {
            bevy_log::warn!("brink: failed to resolve pending query on {entity:?}: {err}");
        }
    }
}

#[cfg(test)]
#[expect(clippy::panic, reason = "tests assert via panic on the error arm")]
mod tests {
    use super::*;
    use crate::test_support::compile_test_story;
    use bevy_ecs::prelude::*;
    use brink_runtime::{FastRng, FlowInstance};

    /// A command event used by tests. `reply` is overridden to echo the
    /// label length back to ink, exercising the value-return path.
    #[derive(Event, Clone, Debug, PartialEq, Eq)]
    struct Ping {
        label: String,
    }

    impl BrinkCommand for Ping {
        fn from_ink_args(args: &[Value]) -> Result<Self, BrinkArgError> {
            let label = args
                .first()
                .and_then(Value::as_str)
                .ok_or(BrinkArgError::Type {
                    index: 0,
                    expected: "string",
                })?
                .to_string();
            Ok(Self { label })
        }

        fn reply(&self) -> Value {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap,
                reason = "test value, small"
            )]
            Value::Int(self.label.len() as i32)
        }
    }

    /// A multi-field command whose `BrinkCommand` impl is generated by the
    /// derive macro (strict types, default `reply` of `Null`).
    #[derive(Event, Clone, Debug, PartialEq, bevy_brink_derive::BrinkCommand)]
    struct SetVolume {
        channel: i32,
        level: f32,
    }

    fn app_with_double_and_ping() -> App {
        let mut app = App::new();
        app.bind_brink_fn::<(), _, _>("double", |args| {
            args.first().and_then(Value::as_int).unwrap_or(0) * 2
        });
        app.bind_brink_command::<(), Ping>("ping");
        app
    }

    #[test]
    fn pure_fn_resolves_inline() {
        let app = app_with_double_and_ping();
        let bindings = app.world().resource::<BrinkBindings<()>>();
        let handler = bindings.handler();
        match handler.call("double", &[Value::Int(21)]) {
            ExternalResult::Resolved(Value::Int(42)) => {}
            other => panic!("expected Resolved(Int(42)), got {other:?}"),
        }
        assert_eq!(handler.queued_len(), 0, "pure fn buffers nothing");
    }

    #[test]
    fn command_buffers_trigger_and_returns_reply() {
        let app = app_with_double_and_ping();
        let bindings = app.world().resource::<BrinkBindings<()>>();
        let handler = bindings.handler();
        // "hi" has length 2 → reply Int(2); one trigger buffered.
        match handler.call("ping", &[Value::from("hi")]) {
            ExternalResult::Resolved(Value::Int(2)) => {}
            other => panic!("expected Resolved(Int(2)), got {other:?}"),
        }
        assert_eq!(handler.queued_len(), 1, "command buffers one trigger");
    }

    #[test]
    fn unknown_name_falls_back() {
        let app = app_with_double_and_ping();
        let bindings = app.world().resource::<BrinkBindings<()>>();
        let handler = bindings.handler();
        match handler.call("nonexistent", &[]) {
            ExternalResult::Fallback => {}
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn bad_command_args_resolve_null_without_buffering() {
        let app = app_with_double_and_ping();
        let bindings = app.world().resource::<BrinkBindings<()>>();
        let handler = bindings.handler();
        // ping wants a string; give it an int → parse fails → Null, no buffer.
        match handler.call("ping", &[Value::Int(7)]) {
            ExternalResult::Resolved(Value::Null) => {}
            other => panic!("expected Resolved(Null), got {other:?}"),
        }
        assert_eq!(handler.queued_len(), 0, "failed parse buffers nothing");
    }

    /// End-to-end: a pure-fn binding's return value is inlined into story
    /// text by the real VM.
    #[test]
    fn e2e_pure_fn_value_appears_in_text() {
        let (program, tables, _ctx) =
            compile_test_story("EXTERNAL double(x)\nResult: {double(21)}.\n-> END\n");

        let app = app_with_double_and_ping();
        let bindings = app.world().resource::<BrinkBindings<()>>();
        let handler = bindings.handler();

        let (mut flow, mut ctx) = FlowInstance::new_at_root(&program);
        let mut text = String::new();
        loop {
            let line = flow
                .step_single_line::<FastRng>(&program, &tables, &mut ctx, &handler, None)
                .unwrap();
            text.push_str(line.text());
            if line.is_terminal() {
                break;
            }
        }
        assert!(
            text.contains("Result: 42"),
            "expected 'Result: 42' in story text; got {text:?}"
        );
    }

    /// End-to-end: a command binding fires its Bevy event when the VM hits
    /// the external call. We drive the VM, then apply the buffered trigger
    /// to the world and confirm an observer saw the event.
    #[test]
    fn e2e_command_triggers_event() {
        #[derive(Resource, Default)]
        struct PingLog(Vec<String>);

        let (program, tables, _ctx) =
            compile_test_story("EXTERNAL ping(label)\nA{ping(\"hi\")}B\n-> END\n");

        let mut app = app_with_double_and_ping();
        app.init_resource::<PingLog>();
        app.add_observer(|on: On<Ping>, mut log: ResMut<PingLog>| {
            log.0.push(on.event().label.clone());
        });

        // Drive the flow inside a scope so the borrow of BrinkBindings
        // ends before we mutate the world to apply triggers.
        let triggers = {
            let bindings = app.world().resource::<BrinkBindings<()>>();
            let handler = bindings.handler();
            let (mut flow, mut ctx) = FlowInstance::new_at_root(&program);
            loop {
                let line = flow
                    .step_single_line::<FastRng>(&program, &tables, &mut ctx, &handler, None)
                    .unwrap();
                if line.is_terminal() {
                    break;
                }
            }
            handler.queued.into_inner()
        };
        assert_eq!(triggers.len(), 1, "exactly one ping trigger buffered");

        for trigger in triggers {
            trigger(app.world_mut());
        }

        let log = app.world().resource::<PingLog>();
        assert_eq!(
            log.0,
            vec!["hi".to_string()],
            "observer should see ping(\"hi\")"
        );
    }

    #[test]
    fn derived_from_ink_args_parses_strictly() {
        // Correct count + types.
        let ok = SetVolume::from_ink_args(&[Value::Int(2), Value::Float(0.5)]).unwrap();
        assert_eq!(
            ok,
            SetVolume {
                channel: 2,
                level: 0.5
            }
        );
        // Default reply is Null.
        assert!(matches!(ok.reply(), Value::Null));

        // Wrong count.
        assert_eq!(
            SetVolume::from_ink_args(&[Value::Int(2)]),
            Err(BrinkArgError::Count {
                expected: 2,
                got: 1
            })
        );

        // Wrong type at index 1 (int where float expected — strict, no
        // coercion, mirroring bladeink's derive).
        assert_eq!(
            SetVolume::from_ink_args(&[Value::Int(2), Value::Int(3)]),
            Err(BrinkArgError::Type {
                index: 1,
                expected: "float"
            })
        );
    }

    /// End-to-end: a derived command binding fires its event when the VM
    /// hits the external call, just like a hand-written one.
    #[test]
    fn e2e_derived_command_triggers_event() {
        #[derive(Resource, Default)]
        struct VolumeLog(Vec<(i32, f32)>);

        let (program, tables, _ctx) =
            compile_test_story("EXTERNAL set_volume(ch, lvl)\nA{set_volume(2, 0.5)}B\n-> END\n");

        let mut app = App::new();
        app.bind_brink_command::<(), SetVolume>("set_volume");
        app.init_resource::<VolumeLog>();
        app.add_observer(|on: On<SetVolume>, mut log: ResMut<VolumeLog>| {
            log.0.push((on.event().channel, on.event().level));
        });

        let triggers = {
            let bindings = app.world().resource::<BrinkBindings<()>>();
            let handler = bindings.handler();
            let (mut flow, mut ctx) = FlowInstance::new_at_root(&program);
            loop {
                let line = flow
                    .step_single_line::<FastRng>(&program, &tables, &mut ctx, &handler, None)
                    .unwrap();
                if line.is_terminal() {
                    break;
                }
            }
            handler.queued.into_inner()
        };
        assert_eq!(triggers.len(), 1);
        for trigger in triggers {
            trigger(app.world_mut());
        }

        let log = app.world().resource::<VolumeLog>();
        assert_eq!(
            log.0,
            vec![(2, 0.5)],
            "observer should see set_volume(2, 0.5)"
        );
    }

    // ── Engine → ink: call_ink_function + bind_brink_query ───────────

    #[derive(Component)]
    struct Enemy;

    /// A world-access query binding: count `Enemy` entities.
    fn enemy_count(In((_entity, _args)): In<BrinkQueryInput>, enemies: Query<&Enemy>) -> Value {
        #[expect(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        Value::Int(enemies.iter().count() as i32)
    }

    /// End-to-end engine→ink: an ink function that queries the World via a
    /// `bind_brink_query` binding, driven synchronously by
    /// `call_ink_function`.
    #[test]
    fn call_ink_function_resolves_world_query() {
        use crate::BrinkFlowRequest;
        use crate::test_support::{add_story_assets, make_test_app};

        let mut app = make_test_app();
        app.bind_brink_query::<(), _, _>("enemy_count", enemy_count);

        // can_spawn() := enemy_count() < 3
        let (program, tables, ctx) = compile_test_story(
            "EXTERNAL enemy_count()\n-> END\n=== function can_spawn() ===\n~ return enemy_count() < 3\n",
        );
        let story = add_story_assets(&mut app, program, tables, ctx);

        // Two enemies → can_spawn should be true (2 < 3).
        app.world_mut().spawn(Enemy);
        app.world_mut().spawn(Enemy);

        let entity = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();
        app.update(); // fulfill the request → flow components on `entity`

        let result = call_ink_function::<()>(app.world_mut(), entity, "can_spawn", &[]).unwrap();
        assert_eq!(result.as_bool(), Some(true), "2 enemies < 3 → can spawn");

        // Add two more enemies (4 total) → can_spawn should now be false.
        app.world_mut().spawn(Enemy);
        app.world_mut().spawn(Enemy);
        let result = call_ink_function::<()>(app.world_mut(), entity, "can_spawn", &[]).unwrap();
        assert_eq!(
            result.as_bool(),
            Some(false),
            "4 enemies !< 3 → cannot spawn"
        );
    }

    /// A pure binding called from inside an engine→ink function resolves
    /// inline (no World access), and an unknown function errors clearly.
    #[test]
    fn call_ink_function_pure_and_errors() {
        use crate::BrinkFlowRequest;
        use crate::test_support::{add_story_assets, make_test_app};

        let mut app = make_test_app();
        app.bind_brink_fn::<(), _, _>("triple", |args| {
            args.first().and_then(Value::as_int).unwrap_or(0) * 3
        });

        let (program, tables, ctx) = compile_test_story(
            "EXTERNAL triple(n)\n-> END\n=== function scaled(n) ===\n~ return triple(n) + 1\n",
        );
        let story = add_story_assets(&mut app, program, tables, ctx);
        let entity = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();
        app.update();

        // triple(7) + 1 = 22
        let result =
            call_ink_function::<()>(app.world_mut(), entity, "scaled", &[Value::Int(7)]).unwrap();
        assert_eq!(result, Value::Int(22));

        // Unknown function → clear error.
        let err = call_ink_function::<()>(app.world_mut(), entity, "nope", &[]).unwrap_err();
        assert!(
            matches!(err, BrinkCallError::FunctionNotFound(_)),
            "got {err:?}"
        );
    }

    /// A story line that calls a world-access query binding inline
    /// (`{enemy_count()}`) resolves transparently when driven by
    /// `advance_flow`.
    #[test]
    fn advance_flow_resolves_inline_query_during_playback() {
        use crate::BrinkFlowRequest;
        use crate::test_support::{add_story_assets, make_test_app};

        let mut app = make_test_app();
        app.bind_brink_query::<(), _, _>("enemy_count", enemy_count);

        let (program, tables, ctx) =
            compile_test_story("EXTERNAL enemy_count()\nEnemies near: {enemy_count()}.\n-> END\n");
        let story = add_story_assets(&mut app, program, tables, ctx);
        app.world_mut().spawn(Enemy);
        app.world_mut().spawn(Enemy);

        let entity = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();
        app.update(); // fulfill

        let line = advance_flow::<()>(app.world_mut(), entity).unwrap();
        assert!(
            line.text().contains("Enemies near: 2."),
            "inline query should resolve to 2; got {:?}",
            line.text()
        );
    }

    /// The non-exclusive path: a normal `step_one` driver pauses on a query
    /// (`Advance::AwaitingQuery`), the plugin's resolver resolves it across
    /// frames, and the driver resumes to produce the line.
    #[test]
    #[expect(clippy::type_complexity, reason = "bevy driver closure query tuple")]
    fn step_one_query_pauses_and_plugin_resolver_resumes() {
        use crate::test_support::{add_story_assets, make_test_app};
        use crate::{
            Advance, BrinkContext, BrinkFlow, BrinkFlowRequest, BrinkLocale, BrinkProgram,
        };
        use bevy_app::Update;

        #[derive(Resource, Default)]
        struct Lines(Vec<String>);

        let mut app = make_test_app();
        app.init_resource::<Lines>();
        app.bind_brink_query::<(), _, _>("enemy_count", enemy_count);

        let (program, tables, ctx) =
            compile_test_story("EXTERNAL enemy_count()\nEnemies near: {enemy_count()}.\n-> END\n");
        let story = add_story_assets(&mut app, program, tables, ctx);
        app.world_mut().spawn(Enemy);
        app.world_mut().spawn(Enemy);
        app.world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build());

        // A normal (non-exclusive) driver: step each flow once per frame,
        // skipping flows paused on a query (the plugin resolver handles
        // those; we resume next frame).
        app.add_systems(
            Update,
            |mut flows: Query<(
                Entity,
                &mut BrinkFlow<()>,
                &mut BrinkContext<()>,
                &BrinkProgram<()>,
                &BrinkLocale<()>,
            )>,
             programs: Res<Assets<ProgramAsset>>,
             tables: Res<Assets<LineTablesAsset>>,
             bindings: Res<BrinkBindings<()>>,
             mut commands: Commands,
             mut out: ResMut<Lines>| {
                for (entity, mut flow, mut ctx, prog, loc) in &mut flows {
                    if flow.inner.has_pending_external() {
                        continue; // paused on a query; wait for the resolver
                    }
                    let (Some(p), Some(t)) = (programs.get(&prog.handle), tables.get(&loc.handle))
                    else {
                        continue;
                    };
                    let handler = bindings.handler();
                    if let Ok(Advance::Line(line)) = flow.step_one(
                        &p.program,
                        &t.tables,
                        &mut ctx.inner,
                        &handler,
                        entity,
                        &mut commands,
                    ) {
                        out.0.push(line.text().to_string());
                    }
                    handler.flush(&mut commands);
                }
            },
        );

        // First update fulfills the request; subsequent updates drive +
        // resolve. A handful is plenty regardless of intra-frame ordering.
        for _ in 0..6 {
            app.update();
        }

        let lines = &app.world().resource::<Lines>().0;
        assert!(
            lines.iter().any(|l| l.contains("Enemies near: 2.")),
            "expected the resolved inline-query line; got {lines:?}"
        );
    }
}
