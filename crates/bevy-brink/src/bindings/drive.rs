//! Drive/eval API: the systems that actually step flows and resolve pending
//! externals against the World.
//!
//! Two exclusive (`&mut World`) drivers live here: [`call_ink_function`] (and
//! its batch/function-value siblings) evaluates an ink function from engine
//! code without touching the visible story; [`advance_flow`] advances a
//! flow's playback by one line, resolving world-access query bindings inline.
//! [`resolve_pending_externals`] is the plugin system that services flows
//! parked on a pending external during normal (non-exclusive) `step_one`
//! playback. See the parent module's docs (`crate::bindings`) for the
//! conceptual overview of the three synchronous binding kinds plus the async
//! ones these drivers hand off.

use std::future::Future;
use std::pin::Pin;

use bevy_asset::Assets;
use bevy_ecs::entity::Entity;
use bevy_ecs::system::{Query, Res, ResMut, SystemState};
use bevy_ecs::world::World;
use bevy_log::warn;
use bevy_tasks::{AsyncComputeTaskPool, TaskPool};
use brink_format::Value;
use brink_runtime::{FastRng, FlowInstance, Line, Program, RuntimeError, StepOutcome};
#[cfg(feature = "dev")]
use brink_runtime::{RecordingHandler, ReplayRecorder};
use thiserror::Error;

use crate::asset::{BrinkProgram, LineTablesAsset, ProgramAsset};
use crate::async_bind::{BrinkAwaiting, BrinkExternalAwaited, BrinkPendingTask};
use crate::flow::BrinkFlow;
use crate::globals::BrinkContext;
use crate::line_tables::BrinkLocale;

use super::registration::{AsyncKind, BrinkBindings, BrinkHandler, QuerySystemId, TriggerFn};

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
    /// The function called an **async** external (`bind_brink_async` /
    /// `bind_brink_task`), which can't resolve in a single `&mut World` pass.
    /// Drive such stories via the `step_one` playback path + the plugin's
    /// `resolve_pending_externals` resolver instead.
    #[error("external '{0}' is async; drive the flow via step_one, not the exclusive driver")]
    AsyncExternalUnsupported(String),
    /// A query binding's system failed to run.
    #[error("query binding system failed: {0}")]
    QueryFailed(String),
    /// The `SystemState` snapshot of flow components + assets + bindings
    /// failed to validate against the world (e.g. a required resource,
    /// like `Assets<ProgramAsset>`, isn't present — the plugin wasn't added).
    #[error("system param validation failed: {0}")]
    SystemParamInvalid(String),
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

/// The `SystemState` param bundle the engine→ink eval driver re-borrows each
/// suspension: the flow components, the (optional) globals resource, the two
/// asset stores, and the bindings registry. Factored out so
/// [`call_ink_function`] (by name) and [`call_ink_function_value`] (by opaque
/// function-value token) share one driver.
type EvalSystemState<M> = SystemState<(
    Query<
        'static,
        'static,
        (
            &'static BrinkProgram<M>,
            &'static BrinkLocale<M>,
            &'static mut BrinkFlow<M>,
            &'static mut BrinkContext<M>,
        ),
    >,
    Option<ResMut<'static, crate::BrinkGlobals<M>>>,
    Res<'static, Assets<ProgramAsset>>,
    Res<'static, Assets<LineTablesAsset>>,
    Res<'static, BrinkBindings<M>>,
)>;

/// Drive an in-progress function evaluation to completion: run each pending
/// world-access query against the World (borrows released between calls),
/// resolve it, and resume — until the function returns its value. Shared by
/// [`call_ink_function`] and [`call_ink_function_value`]; the only difference
/// between the two callers is how the evaluation *begins* (by name vs by
/// opaque function-value token), which produces the initial `NextStep`.
fn drive_function_eval_to_done<M: Send + Sync + 'static>(
    world: &mut World,
    entity: Entity,
    state: &mut EvalSystemState<M>,
    mut next: NextStep,
    triggers: &mut Vec<TriggerFn>,
) -> Result<Value, BrinkCallError> {
    loop {
        match next {
            NextStep::Done(value) => return Ok(value),
            NextStep::RunQuery { system, qargs } => {
                let value = world
                    .run_system_with(system, (entity, qargs))
                    .map_err(|e| BrinkCallError::QueryFailed(format!("{e:?}")))?;
                next = {
                    let (mut flows, globals, programs, tables, bindings) = state
                        .get_mut(world)
                        .map_err(|e| BrinkCallError::SystemParamInvalid(e.to_string()))?;
                    let mut globals = globals.ok_or(BrinkCallError::NotAFlow)?;
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
                    let mut view = crate::globals::flow_context_view(&mut globals, &mut ctx);
                    let outcome = flow.inner.resume_function_eval::<FastRng>(
                        program,
                        line_tables,
                        &mut view,
                        &handler,
                        None,
                    )?;
                    triggers.extend(handler.take_queued());
                    classify_eval(&flow.inner, program, &bindings, outcome)?
                };
            }
        }
    }
}

/// Fire buffered command-event triggers from a completed engine→ink eval
/// pass directly against the World — the exclusive driver's equivalent of
/// [`BrinkHandler::flush`], since [`call_ink_function`] and friends already
/// hold `&mut World` rather than a deferred [`bevy_ecs::system::Commands`]
/// queue. Called only once evaluation reaches [`NextStep::Done`]; a mid-eval
/// error drops any triggers queued so far, matching [`advance_flow`]'s
/// existing drop-on-error precedent for buffered command triggers.
///
/// **Ordering this locks in:** because triggers only fire here, at the very
/// end of the call, a `bind_brink_query` invoked *later* in the *same* call
/// always runs (via `run_system_with` in [`drive_function_eval_to_done`])
/// **before** any command trigger buffered earlier in that same call is
/// fired — a query can never observe a command's World effects within one
/// call. This is consistent with [`advance_flow`], which likewise flushes
/// its triggers only once a line is produced, not between suspensions; it's
/// a non-obvious consequence of the buffer-then-flush shape, not a defect.
/// (Across separate calls — e.g. [`call_ink_functions`]'s per-call flush —
/// a later call's query *does* see an earlier call's command effects, since
/// each call flushes before the next begins.)
fn flush_eval_triggers(world: &mut World, triggers: Vec<TriggerFn>) {
    for trigger in triggers {
        trigger(world);
    }
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
            if bindings.async_bindings.contains_key(&name) {
                return Err(BrinkCallError::AsyncExternalUnsupported(name));
            }
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
    let mut state: EvalSystemState<M> = SystemState::new(world);
    let mut triggers: Vec<TriggerFn> = Vec::new();

    // Begin the evaluation (resolve the function by name, then start it).
    let next = begin_eval_by_name(world, entity, &mut state, name, args, &mut triggers)?;

    // Drive: run each pending world-access query against the World, resolve
    // it, and resume — until the function returns.
    let value = drive_function_eval_to_done(world, entity, &mut state, next, &mut triggers)?;

    // Fire any command-event triggers the call queued along the way (#1096).
    flush_eval_triggers(world, triggers);
    Ok(value)
}

/// Begin one by-name function evaluation against an already-built
/// [`EvalSystemState`], resolving the function and taking its first step.
///
/// Factored out of [`call_ink_function`] so the single-call path and the
/// batch path ([`call_ink_functions`]) share identical begin semantics while
/// the batch reuses **one** `SystemState` across every call — the setup
/// (`SystemState::new`) is paid once per batch turn, not once per call.
fn begin_eval_by_name<M: Send + Sync + 'static>(
    world: &mut World,
    entity: Entity,
    state: &mut EvalSystemState<M>,
    name: &str,
    args: &[Value],
    triggers: &mut Vec<TriggerFn>,
) -> Result<NextStep, BrinkCallError> {
    let (mut flows, globals, programs, tables, bindings) = state
        .get_mut(world)
        .map_err(|e| BrinkCallError::SystemParamInvalid(e.to_string()))?;
    let mut globals = globals.ok_or(BrinkCallError::NotAFlow)?;
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
    let mut view = crate::globals::flow_context_view(&mut globals, &mut ctx);
    let outcome = flow.inner.begin_function_eval::<FastRng>(
        program,
        line_tables,
        &mut view,
        &handler,
        idx,
        args,
        None,
    )?;
    triggers.extend(handler.take_queued());
    classify_eval(&flow.inner, program, &bindings, outcome)
}

/// Apply a batch of engine→ink calls to one flow in a **single** VM-eval
/// setup, returning one [`Result`] per call in the order supplied.
///
/// This is the batch counterpart of [`call_ink_function`]: an engine→ink seam
/// (e.g. an event-folding system that pushes a frame's events into ink) can
/// hand the whole frame's calls over at once. The expensive per-call setup —
/// building the [`SystemState`] over the flow components + assets + bindings —
/// is paid **once** for the batch instead of once per call, so a frame with
/// N sightings costs one setup plus N evaluations rather than N setups.
///
/// Per-call semantics are preserved exactly:
///
/// - **Order.** Calls run front-to-back, so `decay` then N × `escalate` then
///   `trigger_global` fold in the same order as N separate
///   [`call_ink_function`]s. State mutated by an earlier call is visible to a
///   later one (they share the flow's globals/context).
/// - **Isolated outputs.** Each call is a fresh `begin_function_eval` — output
///   is isolated, the player-visible story is untouched, visit counts aren't
///   bumped — identical to a standalone [`call_ink_function`].
/// - **Error per call, no silent drops.** A failing call yields `Err(_)` in
///   *its* slot and does **not** abort the batch; every later call still runs
///   and every slot is reported. The returned `Vec` has exactly one entry per
///   input call.
///
/// The result of the *i*-th call is `results[i]`. `M` is the story marker (use
/// `()` for the default). For callers that don't have `&mut World` (a normal
/// system), issue a deferred `commands.brink_call_batch(...)` instead.
///
/// ```no_run
/// # use bevy_ecs::entity::Entity;
/// # use bevy_ecs::world::World;
/// # use bevy_log::warn;
/// # use bevy_brink::{Value, call_ink_functions};
/// # struct AlarmStory;
/// # fn example(
/// #     world: &mut World,
/// #     flow: Entity,
/// #     round_started: bool,
/// #     dt: f32,
/// #     spots: Vec<f32>,
/// #     has_global: bool,
/// # ) {
/// // The alarm write-seam, one VM entry instead of N+2:
/// let mut calls: Vec<(&str, Vec<Value>)> = Vec::new();
/// if round_started { calls.push(("alarm_reset", vec![])); }
/// calls.push(("decay", vec![Value::Float(dt)]));
/// for amount in spots { calls.push(("escalate_spotting", vec![Value::Float(amount)])); }
/// if has_global { calls.push(("trigger_global", vec![])); }
/// for (call, res) in calls.iter().zip(call_ink_functions::<AlarmStory, _, _>(world, flow, calls.clone())) {
///     if let Err(err) = res { warn!("[alarm] ink call {} failed: {err}", call.0); }
/// }
/// # }
/// ```
///
/// # Errors
/// Errors are returned per call in the result `Vec`; the function itself does
/// not short-circuit. See [`BrinkCallError`].
pub fn call_ink_functions<M, N, A>(
    world: &mut World,
    entity: Entity,
    calls: impl IntoIterator<Item = (N, A)>,
) -> Vec<Result<Value, BrinkCallError>>
where
    M: Send + Sync + 'static,
    N: AsRef<str>,
    A: AsRef<[Value]>,
{
    // One setup for the whole batch — the amortization the batch exists for.
    let mut state: EvalSystemState<M> = SystemState::new(world);

    calls
        .into_iter()
        .map(|(name, args)| {
            // Fresh trigger buffer per call so a command-event fires right
            // after *its own* call completes — preserving the "isolated
            // outputs, identical to a standalone call_ink_function" contract
            // this batch API documents, rather than deferring every call's
            // triggers to the end of the whole batch.
            let mut triggers: Vec<TriggerFn> = Vec::new();
            let next = begin_eval_by_name(
                world,
                entity,
                &mut state,
                name.as_ref(),
                args.as_ref(),
                &mut triggers,
            )?;
            let value =
                drive_function_eval_to_done(world, entity, &mut state, next, &mut triggers)?;
            flush_eval_triggers(world, triggers);
            Ok(value)
        })
        .collect()
}

/// Synchronously invoke an ink **function value** (`#fn(…)` — a `FnRef` or
/// `Closure`) on a flow entity from an exclusive (`&mut World`) context,
/// returning its value — the host callback-invocation surface (T1c-3,
/// `docs/t1c-spec.md` §6).
///
/// This is the [`call_ink_function`] sibling for the case where the host
/// holds an opaque function-value token (obtained from a global, a returned
/// value, or a `bind_brink_query` result) rather than a static function name.
/// The host never dereferences the token's env — invocation re-enters the VM
/// (`FlowInstance::begin_function_value_eval`), running any world-access query
/// bindings the callback triggers, and is journaled exactly like a by-name
/// call. `args` supply the remaining (val-only) params after the value's bound
/// prefix.
///
/// The dispatch faults of `docs/t1c-spec.md` §3/§6 (non-function value, wrong
/// arity, rehydration mismatch, cross-flow ref-`#@local`) surface as
/// [`BrinkCallError::Runtime`].
///
/// `M` is the story marker (use `()` for the default).
///
/// # Errors
/// See [`BrinkCallError`].
pub fn call_ink_function_value<M: Send + Sync + 'static>(
    world: &mut World,
    entity: Entity,
    callee: &Value,
    args: &[Value],
) -> Result<Value, BrinkCallError> {
    let mut state: EvalSystemState<M> = SystemState::new(world);
    let mut triggers: Vec<TriggerFn> = Vec::new();

    // Begin the evaluation through the opaque function-value token.
    let next = {
        let (mut flows, globals, programs, tables, bindings) = state
            .get_mut(world)
            .map_err(|e| BrinkCallError::SystemParamInvalid(e.to_string()))?;
        let mut globals = globals.ok_or(BrinkCallError::NotAFlow)?;
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
        let mut view = crate::globals::flow_context_view(&mut globals, &mut ctx);
        let outcome = flow.inner.begin_function_value_eval::<FastRng>(
            program,
            line_tables,
            &mut view,
            &handler,
            callee,
            args,
            None,
        )?;
        triggers.extend(handler.take_queued());
        classify_eval(&flow.inner, program, &bindings, outcome)?
    };

    let value = drive_function_eval_to_done(world, entity, &mut state, next, &mut triggers)?;
    flush_eval_triggers(world, triggers);
    Ok(value)
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
        /// External name, carried only in dev builds so the resolved query
        /// result can be recorded into the flow's replay log.
        #[cfg(feature = "dev")]
        name: String,
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
        // A park (`Line::Suspended`, FS-3r) is a turn boundary like `Done`;
        // runtime-unreachable today behind the E052 fence, grouped here so
        // the exhaustive match keeps compiling as the variant lands.
        Line::Done { text, tags } | Line::Suspended { text, tags } => {
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

/// One `advance` step for [`advance_flow`], wrapping the handler with a
/// [`RecordingHandler`] when a recorder is active (dev) so inline pure/command
/// results are captured into the flow's replay log. A thin pass-through in
/// non-dev builds (the `recorder` parameter doesn't exist there).
fn advance_recording<M: Send + Sync + 'static>(
    flow: &mut FlowInstance,
    program: &Program,
    line_tables: &[Vec<brink_format::LineEntry>],
    context: &mut (impl brink_runtime::ContextAccess + ?Sized),
    handler: &BrinkHandler<'_, M>,
    #[cfg(feature = "dev")] recorder: Option<&mut ReplayRecorder>,
) -> Result<StepOutcome, RuntimeError> {
    #[cfg(feature = "dev")]
    if let Some(rec) = recorder {
        let recording = RecordingHandler::new(handler, rec);
        return flow.advance::<FastRng>(program, line_tables, context, &recording, None);
    }
    flow.advance::<FastRng>(program, line_tables, context, handler, None)
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
/// Bounded by a [`FlowInstance::LINE_LIMIT`] budget shared across every
/// inline resume this call makes (each pending query resolved and each line
/// produced decrements it by one) — reconciled onto the same
/// `RuntimeError::LineLimitExceeded` convention
/// [`FlowInstance::drive`]/`advance_until_terminal` use, rather than
/// looping unboundedly if a story keeps calling inline-resolvable externals
/// without ever producing a line (guard against unbounded growth).
///
/// # Errors
/// See [`BrinkCallError`].
#[expect(
    clippy::too_many_lines,
    reason = "the SystemState re-borrow dance around run_system_with doesn't split cleanly"
)]
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
        Option<ResMut<crate::BrinkGlobals<M>>>,
        Res<Assets<ProgramAsset>>,
        Res<Assets<LineTablesAsset>>,
        Res<BrinkBindings<M>>,
    )> = SystemState::new(world);

    // Command-event triggers accumulate across the suspensions of a single
    // line, then flush once the line is produced.
    let mut triggers: Vec<TriggerFn> = Vec::new();

    // In dev builds, record every external resolved during this pass into the
    // flow's replay log so a hot-reload can replay it faithfully. Taken out of
    // the component up front (and put back before the line returns) so it can
    // wrap the handler / be written without holding `BrinkReplayLog` borrowed
    // across the World re-borrows below. `None` for a non-dev-tracked flow.
    #[cfg(feature = "dev")]
    let mut recorder: Option<ReplayRecorder> = crate::replay::take_recorder::<M>(world, entity);

    let mut budget = FlowInstance::LINE_LIMIT;

    loop {
        if budget == 0 {
            return Err(RuntimeError::LineLimitExceeded(FlowInstance::LINE_LIMIT).into());
        }
        budget -= 1;

        let step = {
            let (mut flows, globals, programs, tables, bindings) = state
                .get_mut(world)
                .map_err(|e| BrinkCallError::SystemParamInvalid(e.to_string()))?;
            let mut globals = globals.ok_or(BrinkCallError::NotAFlow)?;
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
            let mut view = crate::globals::flow_context_view(&mut globals, &mut ctx);
            // Inline pure/command results are captured by `advance_recording`'s
            // RecordingHandler wrap (dev); out-of-band query results are recorded
            // at the resolve site below.
            let outcome = advance_recording(
                &mut flow.inner,
                program,
                line_tables,
                &mut view,
                &handler,
                #[cfg(feature = "dev")]
                recorder.as_mut(),
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
                    if bindings.async_bindings.contains_key(&name) {
                        return Err(BrinkCallError::AsyncExternalUnsupported(name));
                    }
                    let system = bindings
                        .query(&name)
                        .ok_or_else(|| BrinkCallError::UnknownQuery(name.clone()))?;
                    let qargs = flow.inner.pending_external_args().to_vec();
                    FlowStep::Query {
                        system,
                        qargs,
                        #[cfg(feature = "dev")]
                        name,
                    }
                }
            }
        };

        match step {
            FlowStep::Line(line) => {
                for trigger in triggers {
                    trigger(world);
                }
                emit_line_event_world::<M>(world, entity, &line);
                #[cfg(feature = "dev")]
                if let Some(rec) = recorder {
                    crate::replay::put_recorder::<M>(world, entity, rec);
                }
                return Ok(line);
            }
            FlowStep::Query {
                system,
                qargs,
                #[cfg(feature = "dev")]
                name,
            } => {
                let value = world
                    .run_system_with(system, (entity, qargs.clone()))
                    .map_err(|e| BrinkCallError::QueryFailed(format!("{e:?}")))?;
                #[cfg(feature = "dev")]
                if let Some(rec) = recorder.as_mut() {
                    rec.record(&name, &qargs, &value);
                }
                let (mut flows, ..) = state
                    .get_mut(world)
                    .map_err(|e| BrinkCallError::SystemParamInvalid(e.to_string()))?;
                let (_, _, mut flow, _) = flows
                    .get_mut(entity)
                    .map_err(|_| BrinkCallError::NotAFlow)?;
                flow.inner.resolve_external(value);
            }
        }
    }
}

/// What [`dispatch_one_external`] decided to do for a parked flow, computed
/// inside the immutable borrow scope and acted on afterward (each variant
/// needs `&mut World`).
enum Dispatch {
    /// Nothing to do (no pending external, program not loaded yet, already
    /// dispatched, or genuinely unbound — the latter warns inside).
    Nothing,
    /// World-access query: run the system, resolve with its return value.
    Query {
        system: QuerySystemId,
        qargs: Vec<Value>,
        /// External name, carried in dev builds so the resolved query result
        /// can be recorded into the flow's replay log, and in `effect-trace`
        /// builds so the dispatch can be logged against the binding's
        /// captured `Access` (issue #938's ground-truth check).
        #[cfg(any(feature = "dev", feature = "effect-trace"))]
        name: String,
    },
    /// `bind_brink_async` (event): fire [`BrinkExternalAwaited`] + insert the
    /// [`BrinkAwaiting`] marker.
    FireEvent { name: String, qargs: Vec<Value> },
    /// `bind_brink_task`: spawn this future on the async pool, park a
    /// [`BrinkPendingTask`].
    SpawnTask {
        fut: Pin<Box<dyn Future<Output = Value> + Send>>,
        /// Name + args, carried only in dev builds so [`poll_brink_tasks`]
        /// can record the task's result into the flow's replay log when it
        /// completes (the value isn't known until then).
        #[cfg(feature = "dev")]
        name: String,
        #[cfg(feature = "dev")]
        qargs: Vec<Value>,
    },
}

/// Resolve / hand off the (single) external a parked flow is waiting on,
/// dispatched by binding kind:
/// - world-access query → run its system inline and resolve;
/// - `bind_brink_async` → fire [`BrinkExternalAwaited`] once (guarded by the
///   [`BrinkAwaiting`] marker) and leave the flow parked for the engine;
/// - `bind_brink_task` → spawn the future once (guarded by [`BrinkPendingTask`])
///   and leave the flow parked for [`poll_brink_tasks`](crate::poll_brink_tasks).
///
/// Decide what [`dispatch_one_external`] should do for `entity`, computed inside
/// the immutable borrow scope (so the action can re-borrow `&mut World`).
/// [`Dispatch::Nothing`] when the flow has no pending external, isn't loaded, or
/// has already been dispatched.
#[expect(
    clippy::type_complexity,
    reason = "SystemState param tuple for the flow component (+ dispatch markers) + assets + bindings"
)]
fn decide_dispatch<M: Send + Sync + 'static>(world: &mut World, entity: Entity) -> Dispatch {
    let mut state: SystemState<(
        Query<(
            &BrinkProgram<M>,
            &BrinkFlow<M>,
            Option<&BrinkAwaiting<M>>,
            Option<&BrinkPendingTask<M>>,
        )>,
        Res<Assets<ProgramAsset>>,
        Res<BrinkBindings<M>>,
    )> = SystemState::new(world);
    let Ok((flows, programs, bindings)) = state.get(world) else {
        // A required resource (Assets<ProgramAsset>, BrinkBindings<M>) isn't
        // present yet — leave parked; we'll retry next frame.
        return Dispatch::Nothing;
    };
    let Ok((prog_c, flow, awaiting, pending_task)) = flows.get(entity) else {
        return Dispatch::Nothing;
    };
    if !flow.inner.has_pending_external() {
        return Dispatch::Nothing;
    }
    let Some(program) = programs.get(&prog_c.handle) else {
        // Program not loaded yet — leave parked; we'll retry next frame.
        return Dispatch::Nothing;
    };
    let program = &program.program;
    let name = flow
        .inner
        .pending_external_name(program)
        .unwrap_or_default()
        .to_owned();
    let qargs = flow.inner.pending_external_args().to_vec();

    if let Some(system) = bindings.query(&name) {
        Dispatch::Query {
            system,
            qargs,
            #[cfg(any(feature = "dev", feature = "effect-trace"))]
            name,
        }
    } else if let Some(kind) = bindings.async_bindings.get(&name) {
        match kind {
            AsyncKind::Event if awaiting.is_some() => Dispatch::Nothing, // already fired
            AsyncKind::Event => Dispatch::FireEvent { name, qargs },
            AsyncKind::Task(_) if pending_task.is_some() => Dispatch::Nothing, // already spawned
            AsyncKind::Task(factory) => {
                #[cfg(feature = "dev")]
                let fut = factory(qargs.clone());
                #[cfg(not(feature = "dev"))]
                let fut = factory(qargs);
                Dispatch::SpawnTask {
                    fut,
                    #[cfg(feature = "dev")]
                    name,
                    #[cfg(feature = "dev")]
                    qargs,
                }
            }
        }
    } else {
        // Pending but unbound. The handler only pauses on registered names, so
        // this indicates a registration race; warn and leave parked.
        warn!("brink: flow {entity:?} parked on unbound external '{name}'");
        Dispatch::Nothing
    }
}

/// A no-op when the flow has no pending external or its program isn't loaded.
fn dispatch_one_external<M: Send + Sync + 'static>(world: &mut World, entity: Entity) {
    match decide_dispatch::<M>(world, entity) {
        Dispatch::Nothing => {}
        Dispatch::Query {
            system,
            qargs,
            #[cfg(any(feature = "dev", feature = "effect-trace"))]
            name,
        } => match world.run_system_with(system, (entity, qargs.clone())) {
            Ok(value) => {
                #[cfg(feature = "dev")]
                crate::replay::record_external::<M>(world, entity, &name, &qargs, &value);
                // Issue #938's host-side ground-truth check: log this real
                // dispatch's binding-declared `Access` (captured at
                // `bind_brink_query` registration) so a test/harness can
                // later assert it stayed a subset of BH-1's row-join
                // (`crate::ground_truth::check`). A missing `BrinkBindings<M>`
                // resource or an unregistered binding name (should not
                // happen — this dispatch only runs for a name `bindings.query`
                // just resolved) skips recording rather than panicking.
                #[cfg(feature = "effect-trace")]
                if let Some(access) = world
                    .get_resource::<BrinkBindings<M>>()
                    .and_then(|b| b.query_access(&name))
                    .cloned()
                {
                    crate::ground_truth::record::<M>(world, entity, &name, access);
                }
                let mut flows = world.query::<&mut BrinkFlow<M>>();
                if let Ok(mut flow) = flows.get_mut(world, entity) {
                    flow.inner.resolve_external(value);
                }
            }
            Err(err) => warn!("brink: query binding failed on {entity:?}: {err:?}"),
        },
        Dispatch::FireEvent { name, qargs } => {
            // Insert the marker BEFORE firing so a synchronous resolve observer
            // can find + remove it (world.trigger runs observers and flushes
            // their commands inline).
            world
                .entity_mut(entity)
                .insert(BrinkAwaiting::<M>::new(name.clone()));
            world
                .entity_mut(entity)
                .trigger(|e| BrinkExternalAwaited::<M>::new(e, name, qargs));
        }
        Dispatch::SpawnTask {
            fut,
            #[cfg(feature = "dev")]
            name,
            #[cfg(feature = "dev")]
            qargs,
        } => {
            // get_or_init so we don't panic in apps/tests without TaskPoolPlugin;
            // a no-op when the pool is already set up (e.g. by DefaultPlugins).
            let task = AsyncComputeTaskPool::get_or_init(TaskPool::default).spawn(fut);
            world.entity_mut(entity).insert(BrinkPendingTask::<M>::new(
                task,
                #[cfg(feature = "dev")]
                name,
                #[cfg(feature = "dev")]
                qargs,
            ));
        }
    }
}

/// Run condition: `true` if any `BrinkFlow<M>` is paused on a pending
/// external (so the resolver only runs when there's work).
#[must_use]
pub fn any_flow_awaiting_external<M: Send + Sync + 'static>(flows: Query<&BrinkFlow<M>>) -> bool {
    flows.iter().any(|f| f.inner.has_pending_external())
}

/// Exclusive plugin system: service flows that paused on a pending external
/// during normal playback (after a non-exclusive
/// [`step_one`](crate::BrinkFlow::step_one) yielded
/// [`Advance::AwaitingQuery`](crate::Advance::AwaitingQuery)).
///
/// For each parked flow, [`dispatch_one_external`] resolves a world-access
/// query inline, fires [`BrinkExternalAwaited`] for a `bind_brink_async`
/// binding, or spawns the task for a `bind_brink_task` binding. Registered by
/// the plugin, gated on [`any_flow_awaiting_external`].
pub fn resolve_pending_externals<M: Send + Sync + 'static>(world: &mut World) {
    let paused: Vec<Entity> = {
        let mut flows = world.query::<(Entity, &BrinkFlow<M>)>();
        flows
            .iter(world)
            .filter(|(_, f)| f.inner.has_pending_external())
            .map(|(e, _)| e)
            .collect()
    };
    for entity in paused {
        dispatch_one_external::<M>(world, entity);
    }
}
