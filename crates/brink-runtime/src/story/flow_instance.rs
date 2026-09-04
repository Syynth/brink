//! [`FlowInstance`] — a single independent execution context within a
//! story, and the low-level orchestration entry points documented in
//! `CLAUDE.md`'s "Runtime public API" (`advance`/`begin_function_eval`/etc.).

use alloc::borrow::ToOwned;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use brink_format::{DefinitionId, PluralResolver, Value};

use crate::error::{RanOutOfContentCause, RuntimeError};
use crate::output::OutputBuffer;
use crate::program::Program;
use crate::rng::StoryRng;
use crate::state::ContextAccess;
use crate::vm;
use crate::world::{ResolvedPolicy, World};

use super::call_stack::{
    CallFrame, CallFrameType, CallStack, ChoiceDisplay, ContainerPosition, ExecMode, Flow,
    PendingTerminal, Thread,
};
use super::external::{ExternalFnHandler, ExternalResult, FunctionEval};
use super::types::{BlockId, Choice, Element, OutputLine, Stats, Step, StepOutcome, StoryStatus};

// ── FlowInstance ────────────────────────────────────────────────────────────

/// A single independent execution context within a story. The default flow
/// runs from the root container; named flows can be spawned at arbitrary
/// entry points via [`FlowInstance::new_at`].
///
/// A `FlowInstance` is opaque from outside the crate: its internal fields
/// (`flow`, `status`, `stats`) are crate-private, but consumers can hold,
/// clone, serialize, and pass `&mut FlowInstance` to the runtime's step
/// functions. Use the inherent methods ([`step_single_line`](Self::step_single_line),
/// [`choose`](Self::choose), [`transcript`](Self::transcript),
/// [`status`](Self::status), etc.) for all interaction.
#[derive(Clone, Debug)]
pub struct FlowInstance {
    pub(crate) flow: Flow,
    pub(crate) status: StoryStatus,
    pub(crate) stats: Stats,
    /// Transient state for an in-progress engine→ink function evaluation
    /// ([`begin_function_eval`](Self::begin_function_eval)). `Some` only
    /// while a from-game call is mid-flight (possibly paused on an
    /// external); `None` during normal play. Not meaningful to persist.
    pub(crate) eval: Option<EvalState>,
    /// Whether host **semantic** access to `#@private` definitions is
    /// refused on this flow instance (M-2b, `docs/modules-spec.md` §4
    /// boundary rule 2/3). `true` by default. Mirrors
    /// [`Story`]'s own flag for consumers — `bevy-brink`'s per-entity
    /// orchestration, [`crate::Speculation`] — that drive a `FlowInstance`
    /// directly, bypassing `Story` entirely. [`Story`] keeps every
    /// `FlowInstance` it owns (`default`, named, shared) synced to its own
    /// flag via [`Story::set_visibility_enforcement`](crate::Story::set_visibility_enforcement),
    /// so the two never diverge for story-owned flows.
    pub(crate) enforce_visibility: bool,
}

/// Bookkeeping for an in-progress engine→ink function evaluation.
#[derive(Debug, Clone)]
pub(crate) struct EvalState {
    /// Value-stack length recorded before arguments were pushed, so the
    /// return value (and any leftover args) can be reclaimed on return.
    pub value_floor: usize,
    /// Pending-choice count when the eval began. A function that *grows*
    /// this presented a choice — illegal, and distinct from choices the
    /// main story may already have waiting.
    pub choice_floor: usize,
}

/// Outcome of a single [`FlowInstance::drive`] call: either the drive
/// reached a terminal step, or it paused on a deferred external mid-drive.
/// Both variants carry every [`Step`] produced during *this* call — for
/// `AwaitingExternal`, that's the (possibly empty) run of `Step::Line`
/// produced before the pause; for `Terminal`, the terminal step is always
/// the last element (see [`FlowInstance::drive`]).
#[derive(Debug, Clone)]
pub enum DriveOutcome {
    /// Reached a terminal step ([`Step::Done`], [`Step::Choices`], or
    /// [`Step::End`]) — always the last element of the `Vec`.
    Terminal(Vec<Step>),
    /// Paused on a deferred external
    /// ([`ExternalResult::Pending`](crate::ExternalResult::Pending)).
    /// Resolve it ([`FlowInstance::resolve_external`]) and call
    /// [`FlowInstance::drive`] again — with the **same** `budget` — to
    /// resume.
    AwaitingExternal(Vec<Step>),
}

impl FlowInstance {
    /// Create a new flow instance starting at the program's root container,
    /// along with a fresh [`World`] initialized from the program's global
    /// defaults.
    pub fn new_at_root(program: &Program) -> (Self, World) {
        Self::new_at(program, program.root_idx())
    }

    /// Create a new flow instance starting at an arbitrary container index,
    /// along with a fresh [`World`]. Use this to spawn a named flow at a
    /// specific entry point. The caller is responsible for deciding whether
    /// to share the returned `World` with other flows or discard it and
    /// reuse an existing one.
    pub fn new_at(program: &Program, container_idx: u32) -> (Self, World) {
        let globals = program.global_defaults();
        let initial_frame = CallFrame {
            return_address: None,
            temps: Vec::new(),
            temps_written: Vec::new(),
            container_stack: vec![ContainerPosition {
                container_idx,
                offset: 0,
            }],
            frame_type: CallFrameType::Root,
            external_fn_id: None,
            function_output_start: None,
        };
        let initial_thread = Thread {
            call_stack: CallStack::new(initial_frame),
        };
        let flow_instance = Self {
            flow: Flow {
                threads: vec![initial_thread],
                value_stack: Vec::new(),
                output: OutputBuffer::new(),
                pending_choices: Vec::new(),
                current_tags: Vec::new(),
                in_tag: false,
                skipping_choice: false,
                did_safe_exit: false,
                did_unsafe_yield: false,
                line_delivered_this_turn: false,
                ran_out_of_content_cause: RanOutOfContentCause::default(),
                exec_mode: ExecMode::default(),
                pure_callback: crate::story::PureCallbackState::default(),
                next_block_id: 0,
                pending_terminal: PendingTerminal::default(),
                warnings: Vec::new(),
            },
            status: StoryStatus::Active,
            stats: Stats::default(),
            eval: None,
            enforce_visibility: true,
        };
        // All existing construction paths default to the all-`World`
        // policy (see `docs/scoped-flow-state-spec.md` "The policy") — this
        // is the fast path that needs no `Program` symbol lookups and
        // can't fail, so `new_at`/`new_at_root` keep their infallible
        // `(Self, World)` signature.
        let world = World::from_globals(globals, ResolvedPolicy::all_world());
        (flow_instance, world)
    }

    /// Enable or disable host visibility enforcement on this flow instance
    /// (M-2b, `docs/modules-spec.md` §4 boundary rule 3). Enforcement is
    /// **on** by default: [`choose_path_string`](Self::choose_path_string)/
    /// [`choose_path_string_with_args`](Self::choose_path_string_with_args)
    /// into a `#@private` knot/stitch, and
    /// [`begin_function_eval`](Self::begin_function_eval)/
    /// [`begin_function_value_eval`](Self::begin_function_value_eval) of a
    /// `#@private` function, return [`RuntimeError::PrivateAccess`].
    ///
    /// This mirrors [`Story::set_visibility_enforcement`](crate::Story::set_visibility_enforcement)
    /// for consumers that drive a `FlowInstance` directly — `bevy-brink`'s
    /// per-entity orchestration and [`crate::Speculation`] — rather than
    /// through a [`Story`](crate::Story). A `Story` keeps every
    /// `FlowInstance` it owns synced to its own flag when this is called on
    /// the `Story`, so callers that only ever go through `Story` never need
    /// to call this directly.
    pub fn set_visibility_enforcement(&mut self, enforce: bool) {
        self.enforce_visibility = enforce;
    }

    /// Whether host visibility enforcement is currently on for this flow
    /// instance (default `true`).
    #[must_use]
    pub fn visibility_enforced(&self) -> bool {
        self.enforce_visibility
    }

    /// Set the dev/prod execution mode on this flow instance (NS-A4,
    /// [`ExecMode`] — see its docs for the §4b doctrine). Mirrors
    /// [`Story::set_exec_mode`](crate::Story::set_exec_mode) for consumers
    /// that drive a `FlowInstance` directly (`bevy-brink`,
    /// [`crate::Speculation`]). Takes effect immediately — the mode is
    /// consulted at each ordering-verb execution.
    pub fn set_exec_mode(&mut self, mode: ExecMode) {
        self.flow.exec_mode = mode;
    }

    /// The current dev/prod execution mode (NS-A4, [`ExecMode`]).
    #[must_use]
    pub fn exec_mode(&self) -> ExecMode {
        self.flow.exec_mode
    }

    /// Maximum VM steps per `continue_maximally` call before erroring.
    /// Prevents infinite loops from malformed bytecode.
    const STEP_LIMIT: u64 = 1_000_000;

    /// Execute until one complete line of output is available, or until a
    /// yield point (choices/done/ended) if no newline occurs first.
    ///
    /// Returns a [`Step`] telling the caller what happened (`Line`/`Done`/
    /// `Choices`/`End`). This is the simple API for consumers whose
    /// external handler never defers: if the handler returns
    /// [`ExternalResult::Pending`], this errors with
    /// [`UnresolvedExternalCall`](RuntimeError::UnresolvedExternalCall).
    /// For pausable world-access bindings, use [`advance`](Self::advance).
    pub fn step_single_line<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        resolver: Option<&dyn PluralResolver>,
    ) -> Result<Step, RuntimeError> {
        match self.advance::<R>(program, line_tables, context, handler, resolver)? {
            StepOutcome::Step(step) => Ok(step),
            StepOutcome::AwaitingExternal => {
                // Preserve historical behavior for consumers using this
                // (non-pausing) API: a deferred external they can't resolve
                // is an error.
                let id = self
                    .flow
                    .external_fn_id()
                    .ok_or(RuntimeError::CallStackUnderflow)?;
                Err(RuntimeError::UnresolvedExternalCall(id))
            }
        }
    }

    /// Maximum lines produced by a single [`drive_to_terminal`](Self::drive_to_terminal)
    /// call before erroring. Safety net against infinite loops from
    /// malformed bytecode.
    pub const LINE_LIMIT: usize = 10_000;

    /// Step this flow forward until the next terminal step (`Done`,
    /// `Choices`, or `End`), collecting every [`Step`] produced along the
    /// way.
    ///
    /// This is the single Layer-2 "drive to terminal" loop: [`Story`]'s
    /// `continue_maximally*` family is a thin wrapper over it, and any other
    /// holder of a `FlowInstance` (e.g. an engine integration like
    /// `bevy-brink`) should reach for this instead of hand-rolling the same
    /// loop. Semantics:
    ///
    /// - Steps via [`step_single_line`](Self::step_single_line): a deferred
    ///   external ([`ExternalResult::Pending`]) is **not** paused on here —
    ///   it errors with [`RuntimeError::UnresolvedExternalCall`], exactly as
    ///   `step_single_line` does. Callers that need to pause on world-access
    ///   externals mid-drive should drive [`advance`](Self::advance)
    ///   themselves rather than use this method.
    /// - Stops at the first [`Step`] for which [`Step::is_terminal`] returns
    ///   `true`; that step is always the last element of the returned
    ///   `Vec`, and every element before it is a [`Step::Line`].
    /// - Bounded by [`Self::LINE_LIMIT`] (10,000) lines produced in a single
    ///   call; exceeding it returns [`RuntimeError::LineLimitExceeded`]
    ///   rather than looping forever.
    ///
    /// # Errors
    /// Any error [`step_single_line`](Self::step_single_line) itself can
    /// produce, plus [`RuntimeError::LineLimitExceeded`] if the drive
    /// produces [`Self::LINE_LIMIT`] lines without reaching a terminal one.
    pub fn drive_to_terminal<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        resolver: Option<&dyn PluralResolver>,
    ) -> Result<Vec<Step>, RuntimeError> {
        let mut steps = Vec::new();
        loop {
            let step =
                self.step_single_line::<R>(program, line_tables, context, handler, resolver)?;
            let terminal = step.is_terminal();
            steps.push(step);
            if terminal {
                return Ok(steps);
            }
            if steps.len() >= Self::LINE_LIMIT {
                return Err(RuntimeError::LineLimitExceeded(Self::LINE_LIMIT));
            }
        }
    }

    /// The pausable Layer-2 "drive to terminal or external pause" op:
    /// [`drive_to_terminal`](Self::drive_to_terminal)'s sibling for callers
    /// (e.g. `bevy-brink`) whose external bindings need to pause mid-drive
    /// for out-of-band (world-access) resolution rather than erroring.
    ///
    /// Steps via [`advance`](Self::advance) instead of
    /// [`step_single_line`](Self::step_single_line): a deferred external
    /// yields [`DriveOutcome::AwaitingExternal`] (carrying every line
    /// produced so far this call) instead of
    /// [`RuntimeError::UnresolvedExternalCall`]. Resolve it and call `drive`
    /// again to continue — the drive is logically one operation spanning
    /// however many pauses it takes.
    ///
    /// `budget` is the caller-owned line budget for that whole logical
    /// operation: each line `drive` produces (whether the call ends in
    /// `Terminal` or `AwaitingExternal`) decrements it by one, and it is
    /// **not** reset between calls — the caller passes the same `&mut
    /// usize` back in on resume, so a drive spanning many external pauses
    /// still has exactly one bound on total output, not a fresh
    /// [`Self::LINE_LIMIT`] per resume (see the "guard against unbounded
    /// growth" rule). Start a fresh logical drive with a fresh
    /// `budget = FlowInstance::LINE_LIMIT` (or any caller-chosen cap).
    ///
    /// Like `drive_to_terminal`, the terminal step is always the last
    /// element of the returned `Vec` and every step before it is
    /// [`Step::Line`].
    ///
    /// # Errors
    /// Any error [`advance`](Self::advance) itself can produce, plus
    /// [`RuntimeError::LineLimitExceeded`] if `budget` reaches zero before a
    /// terminal step is produced.
    pub fn drive<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        resolver: Option<&dyn PluralResolver>,
        budget: &mut usize,
    ) -> Result<DriveOutcome, RuntimeError> {
        // Captured only to report a meaningful number on exhaustion: the
        // remaining budget *this call* started with, not the (possibly
        // already-partially-spent, across earlier resumes) original cap.
        let starting_budget = *budget;
        let mut steps = Vec::new();
        loop {
            if *budget == 0 {
                return Err(RuntimeError::LineLimitExceeded(starting_budget));
            }
            match self.advance::<R>(program, line_tables, context, handler, resolver)? {
                StepOutcome::AwaitingExternal => return Ok(DriveOutcome::AwaitingExternal(steps)),
                StepOutcome::Step(step) => {
                    let terminal = step.is_terminal();
                    *budget -= 1;
                    steps.push(step);
                    if terminal {
                        return Ok(DriveOutcome::Terminal(steps));
                    }
                }
            }
        }
    }

    /// Like [`step_single_line`](Self::step_single_line), but surfaces a
    /// deferred external ([`ExternalResult::Pending`]) as
    /// [`StepOutcome::AwaitingExternal`] instead of an error — so a
    /// world-access binding hit during normal playback can pause cleanly.
    /// Resolve the pending external and call `advance` again to continue.
    pub fn advance<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        resolver: Option<&dyn PluralResolver>,
    ) -> Result<StepOutcome, RuntimeError> {
        self.advance_with_limit::<R>(
            program,
            line_tables,
            context,
            handler,
            resolver,
            Self::STEP_LIMIT,
        )
    }

    /// Like [`advance`](Self::advance), but the per-call VM step budget is
    /// `step_limit` rather than the hardcoded [`Self::STEP_LIMIT`].
    ///
    /// This is what lets [`crate::Speculation::advance`] cap a single
    /// visible-line drive at a small, caller-supplied budget instead of the
    /// production 1,000,000-step ceiling, so a runaway speculative probe
    /// errors quickly instead of burning a huge step budget before giving
    /// up. `advance` itself is a thin wrapper over this with
    /// `step_limit: Self::STEP_LIMIT` — every existing call site keeps its
    /// exact prior behavior.
    #[expect(clippy::too_many_lines)]
    pub(crate) fn advance_with_limit<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        resolver: Option<&dyn PluralResolver>,
        step_limit: u64,
    ) -> Result<StepOutcome, RuntimeError> {
        // 0. A terminal was computed on the previous call but held back
        //    because its trailing content had to go out first as its own
        //    `Step::Line` (terminals carry no text — §7). Deliver it now,
        //    bare, with no VM stepping — but only if no new run has begun
        //    since it was stashed (`PendingTerminal`'s invalidation
        //    invariant, #2104): a host-directed jump or choice between the
        //    two calls bumps `next_block_id`, and `take_if_current` silently
        //    drops a stash stamped with a now-stale block id instead of
        //    replaying it.
        if let Some(pending) = self
            .flow
            .pending_terminal
            .take_if_current(self.flow.next_block_id)
        {
            return Ok(StepOutcome::Step(pending));
        }

        // 1. If buffer already has a completed line from a previous step,
        //    take it immediately (no VM stepping needed).
        if self.flow.output.has_completed_line()
            && let Some((text, tags, element, source)) =
                self.flow
                    .output
                    .take_first_line(program, line_tables, resolver)
        {
            return Ok(StepOutcome::Step(make_output_line(
                &mut self.flow,
                text,
                tags,
                element,
                source,
            )));
        }

        // 2. If buffer has partial content but VM has already yielded
        //    (any non-Active state), flush it. At a yield point, no more
        //    output is coming, so trailing Newlines are committed.
        if self.flow.output.has_unread() && self.status != StoryStatus::Active {
            let (text, tags, element, source) =
                flush_remaining(&mut self.flow, program, line_tables, resolver);
            return Ok(StepOutcome::Step(yield_step(
                self.status,
                text,
                tags,
                element,
                source,
                &mut self.flow,
                program,
                line_tables,
                resolver,
            )));
        }

        // 3. Status checks.
        if self.status == StoryStatus::Ended {
            return Err(RuntimeError::StoryEnded);
        }
        if self.status == StoryStatus::WaitingForChoice {
            return Err(RuntimeError::NotWaitingForChoice);
        }

        // 4. Reset Done → Active (resuming after output).
        //    If the previous cycle ended without a safe exit (no explicit
        //    -> DONE opcode), the story ran out of content. The previous
        //    call delivered the text — error now.
        if self.status == StoryStatus::Done {
            if !self.flow.did_safe_exit {
                return Err(RuntimeError::RanOutOfContent(
                    self.flow.ran_out_of_content_cause,
                ));
            }
            self.status = StoryStatus::Active;
            // A fresh run begins wherever the story resumes from `Done`.
            self.flow.next_block_id += 1;
            self.flow.line_delivered_this_turn = false;
        }

        // Clear flags — will be set during this cycle if relevant.
        self.flow.did_safe_exit = false;
        self.flow.did_unsafe_yield = false;

        // 5. Step VM loop.
        let Self {
            flow,
            status,
            stats,
            ..
        } = self;
        let step_start = stats.steps;

        loop {
            stats.steps += 1;

            if stats.steps - step_start > step_limit {
                return Err(RuntimeError::StepLimitExceeded(step_limit));
            }

            let stepped = vm::step::<R>(flow, program, line_tables, context, stats, resolver)?;
            stats.materializations += flow.drain_materializations();

            match stepped {
                vm::Stepped::Continue | vm::Stepped::ThreadCompleted => {
                    if flow.output.has_completed_line()
                        && let Some((text, tags, element, source)) =
                            flow.output.take_first_line(program, line_tables, resolver)
                    {
                        return Ok(StepOutcome::Step(make_output_line(
                            flow, text, tags, element, source,
                        )));
                    }
                }

                vm::Stepped::ExternalCall => {
                    // `false` means the handler deferred (Pending): pause
                    // cleanly so the caller can resolve it out-of-band.
                    if !resolve_external_call(flow, program, handler)? {
                        return Ok(StepOutcome::AwaitingExternal);
                    }
                    if flow.output.has_completed_line()
                        && let Some((text, tags, element, source)) =
                            flow.output.take_first_line(program, line_tables, resolver)
                    {
                        return Ok(StepOutcome::Step(make_output_line(
                            flow, text, tags, element, source,
                        )));
                    }
                }

                vm::Stepped::Done => {
                    context.increment_turn_index();

                    // Handle invisible default choices: auto-select and keep running.
                    if !flow.pending_choices.is_empty() {
                        let all_invisible = flow
                            .pending_choices
                            .iter()
                            .all(|pc| pc.flags.is_invisible_default);
                        if all_invisible {
                            select_choice(flow, context, status, stats, 0)?;
                            if flow.output.has_completed_line()
                                && let Some((text, tags, element, source)) =
                                    flow.output.take_first_line(program, line_tables, resolver)
                            {
                                return Ok(StepOutcome::Step(make_output_line(
                                    flow, text, tags, element, source,
                                )));
                            }
                            continue;
                        }
                    }

                    // Set status based on remaining choices.
                    if flow.pending_choices.is_empty() {
                        *status = StoryStatus::Done;
                    } else {
                        *status = StoryStatus::WaitingForChoice;
                        stats.choices_presented += 1;
                    }

                    if flow.output.has_completed_line()
                        && let Some((text, tags, element, source)) =
                            flow.output.take_first_line(program, line_tables, resolver)
                    {
                        return Ok(StepOutcome::Step(make_output_line(
                            flow, text, tags, element, source,
                        )));
                    }

                    let (text, tags, element, source) =
                        flush_remaining(flow, program, line_tables, resolver);
                    return Ok(StepOutcome::Step(yield_step(
                        *status,
                        text,
                        tags,
                        element,
                        source,
                        flow,
                        program,
                        line_tables,
                        resolver,
                    )));
                }

                vm::Stepped::Ended => {
                    context.increment_turn_index();
                    *status = StoryStatus::Ended;

                    if flow.output.has_completed_line()
                        && let Some((text, tags, element, source)) =
                            flow.output.take_first_line(program, line_tables, resolver)
                    {
                        return Ok(StepOutcome::Step(make_output_line(
                            flow, text, tags, element, source,
                        )));
                    }

                    let (text, tags, element, source) =
                        flush_remaining(flow, program, line_tables, resolver);
                    return Ok(StepOutcome::Step(yield_step(
                        *status,
                        text,
                        tags,
                        element,
                        source,
                        flow,
                        program,
                        line_tables,
                        resolver,
                    )));
                }
            }
        }
    }

    /// Select a choice by index. Call [`step_single_line`](Self::step_single_line)
    /// afterward to continue execution from the chosen branch.
    pub fn choose(
        &mut self,
        context: &mut (impl ContextAccess + ?Sized),
        index: usize,
    ) -> Result<(), RuntimeError> {
        if self.status != StoryStatus::WaitingForChoice {
            return Err(RuntimeError::NotWaitingForChoice);
        }
        // `index` numbers the VISIBLE choices (what `Step::Choices` hands
        // out and what C#'s `ChooseChoiceIndex` takes); an invisible
        // fallback ahead of a visible choice — a thread's `+ ->` merged in
        // front of the main flow's choices — sits in `pending_choices`
        // but never in that numbering (issue #3527).
        let position = self
            .flow
            .pending_choices
            .iter()
            .enumerate()
            .filter(|(_, pc)| !pc.flags.is_invisible_default)
            .nth(index)
            .map(|(position, _)| position);
        let Some(position) = position else {
            let available = self
                .flow
                .pending_choices
                .iter()
                .filter(|pc| !pc.flags.is_invisible_default)
                .count();
            return Err(RuntimeError::InvalidChoiceIndex { index, available });
        };
        select_choice(
            &mut self.flow,
            context,
            &mut self.status,
            &mut self.stats,
            position,
        )
    }

    /// Move the play head to a named knot/stitch path — the equivalent of
    /// ink's `Story.ChoosePathString(path)` (with its default
    /// `resetCallstack: true`). Call [`step_single_line`](Self::step_single_line)
    /// (or any continue method) afterward to run from there.
    ///
    /// `path` is a dot-separated runtime path: a knot (`intro`), a qualified
    /// stitch (`intro.dock`), or — for programs compiled by `brink-compiler` —
    /// an author label (`knot.label`, `knot.stitch.label`; an extension over
    /// C#, which cannot address labels).
    ///
    /// Mirroring the C# reference (`Story.ChoosePathString` →
    /// `ResetCallstack`/`ForceEnd` → `ChoosePath` → `state.SetChosenPath` +
    /// `VisitChangedContainersDueToDivert`):
    ///
    /// - The current flow is **force-completed** first: the call stack
    ///   collapses to a single fresh root frame (abandoning any tunnels,
    ///   threads, or in-progress weave), pending choices are cleared, and
    ///   the jump counts as a safe exit (as if the story had hit `-> DONE`).
    /// - The jump **counts as a visit** to the target, with exactly the
    ///   semantics of an in-story `-> path` divert (it goes through the same
    ///   goto machinery, so counting flags are honored identically).
    /// - Output already produced but not yet consumed is **kept** (C# leaves
    ///   the output stream untouched); it is delivered before content from
    ///   the new location. The value stack is likewise left as-is.
    /// - A permanently **ended** story (`-> END`) may be re-entered by
    ///   jumping, matching C# where `ChoosePathString` + `Continue` works
    ///   after the story has ended.
    ///
    /// # Errors
    /// - [`UnknownPath`](RuntimeError::UnknownPath) if `path` resolves to no
    ///   target (the message names the path).
    /// - [`JumpWhileAwaitingExternal`](RuntimeError::JumpWhileAwaitingExternal)
    ///   if the flow is parked on an unresolved external call — a pending
    ///   host call must be resolved, not silently abandoned.
    /// - [`AlreadyEvaluatingFunction`](RuntimeError::AlreadyEvaluatingFunction)
    ///   if an engine→ink function evaluation is in progress (C# likewise
    ///   refuses to redirect mid-function).
    pub fn choose_path_string(
        &mut self,
        program: &Program,
        context: &mut (impl ContextAccess + ?Sized),
        path: &str,
    ) -> Result<(), RuntimeError> {
        self.choose_path_string_with_args(program, context, path, &[])
    }

    /// Like [`choose_path_string`](Self::choose_path_string) but **binds the
    /// target knot's declared parameters** from `args` — host-directed entry
    /// into a parameterized knot/stitch (`=== call(action, present) ===`),
    /// which a plain path jump can't reach with its params bound.
    ///
    /// Semantics are otherwise identical to `choose_path_string` (force-ends
    /// the current flow, counts as a visit, etc.). The args are pushed onto the
    /// value stack in declaration order and bound by the target's prologue —
    /// exactly as an in-story `-> call(a, b)` divert binds them, so this enters
    /// at the container start (where the prologue runs).
    ///
    /// # Errors
    /// In addition to [`choose_path_string`](Self::choose_path_string)'s errors:
    /// [`ArgCountMismatch`](RuntimeError::ArgCountMismatch) if `args.len()`
    /// differs from the target container's declared parameter count. (Programs
    /// built by the converter record no param counts, so they report `0` — pass
    /// no args.)
    pub fn choose_path_string_with_args(
        &mut self,
        program: &Program,
        context: &mut (impl ContextAccess + ?Sized),
        path: &str,
        args: &[Value],
    ) -> Result<(), RuntimeError> {
        // M-2b: refuse host-driven entry into a `#@private` knot/stitch
        // while visibility enforcement is on (`docs/modules-spec.md` §4
        // boundary rule 2). Mirrors `Story`'s own `check_entry_visibility`
        // for callers that drive this `FlowInstance` directly — `bevy-brink`,
        // `Speculation` — without going through `Story`. Checked before any
        // other error path so a private name reports as private, not as
        // "not found" or "awaiting external".
        if self.enforce_visibility && program.has_private_defs() && program.path_is_private(path) {
            return Err(RuntimeError::PrivateAccess {
                name: path.to_owned(),
            });
        }
        // A parked host call cannot be silently abandoned: erroring is the
        // strictest safe behavior (brink-specific — C# has no pausable
        // externals during normal playback).
        if let Some(id) = self.flow.external_fn_id() {
            let external = program
                .external_fn(id)
                .map_or_else(|| format!("{id}"), |e| program.name(e.name).to_owned());
            return Err(RuntimeError::JumpWhileAwaitingExternal {
                path: path.to_owned(),
                external,
            });
        }
        // An in-flight engine→ink evaluation (possibly paused on an external)
        // must finish or be aborted before the flow can be redirected.
        if self.eval.is_some() {
            return Err(RuntimeError::AlreadyEvaluatingFunction);
        }

        let target_id = program
            .find_path_target(path)
            .ok_or_else(|| RuntimeError::UnknownPath(path.to_owned()))?;

        // Arity-check before mutating any state. The target container's
        // declared param count is what its prologue's `DeclareTemp`s will pop.
        let expected = program.path_param_count(path).unwrap_or(0);
        if args.len() != expected as usize {
            return Err(RuntimeError::ArgCountMismatch {
                target: path.to_owned(),
                expected,
                got: args.len(),
            });
        }

        // Force-end the current flow, mirroring C# `ResetCallstack` →
        // `StoryState.ForceEnd`: a single fresh root frame (callStack.Reset),
        // cleared choices, null pointers (the empty container stack), and
        // didSafeExit = true. The output buffer and value stack are
        // deliberately left untouched — C# `ForceEnd` does not clear the
        // output stream or the evaluation stack.
        let root_frame = CallFrame {
            return_address: None,
            temps: Vec::new(),
            temps_written: Vec::new(),
            container_stack: Vec::new(),
            frame_type: CallFrameType::Root,
            external_fn_id: None,
            function_output_start: None,
        };
        self.flow.threads = vec![Thread {
            call_stack: CallStack::new(root_frame),
        }];
        self.flow.pending_choices.clear();
        // No explicit pending-terminal clear needed here: `next_block_id`'s
        // bump below (a fresh run begins at the jump target) is exactly
        // what invalidates any stash from before the jump — see
        // `PendingTerminal`'s doc comment. The host explicitly redirected
        // execution, so the next `advance`/`step_single_line` call correctly
        // steps the VM at the new target rather than handing back a stale
        // `Done`/`Choices`/`End` left over from before the jump.
        // Transient intra-step flags. Both are false at any point a host can
        // observe (between lines / at a yield), but the jump abandons whatever
        // produced them, so clear defensively.
        self.flow.skipping_choice = false;
        self.flow.in_tag = false;
        self.flow.did_safe_exit = true;
        // The jump force-completes the current flow like `-> DONE` (see
        // this method's own doc comment) — a fresh run begins at the
        // target (`BlockId`, §3.7/§8d.2).
        self.flow.next_block_id += 1;
        self.flow.line_delivered_this_turn = false;

        // Push the arguments in declaration order; the target's prologue
        // (`DeclareTemp`) binds them, exactly as `begin_function_eval` and an
        // in-story `-> call(a, b)` divert do.
        self.flow.value_stack.extend_from_slice(args);

        // Jump via the same divert machinery as an in-story `-> path`
        // (mirrors C# `ChoosePath` → `SetChosenPath` +
        // `VisitChangedContainersDueToDivert`): sets the position and
        // increments the target's visit/turn counts per its counting flags.
        vm::goto_target(&mut self.flow, program, context, target_id)?;

        self.status = StoryStatus::Active;
        Ok(())
    }

    /// The current execution status of this flow.
    #[must_use]
    pub fn status(&self) -> StoryStatus {
        self.status
    }

    /// Whether the most recent execution cycle ended with a *safe exit* —
    /// an explicit `-> DONE` opcode — as opposed to falling off the end of
    /// its content with nothing left to run.
    ///
    /// Both cases deliver a terminal [`Step::Done`]; this is the only way
    /// to tell them apart without issuing an extra `advance`/
    /// `step_single_line` call and observing whether it returns
    /// [`RuntimeError::RanOutOfContent`](crate::RuntimeError::RanOutOfContent).
    /// Read it right after receiving a `Step::Done` — it is cleared at the
    /// start of the *next* execution cycle, so a value read before a
    /// terminal step is not meaningful.
    ///
    /// `true`: the story chose to stop (a knot/stitch reached `-> DONE`);
    /// resuming later is well-formed. `false`: the flow ran out of
    /// content; the trailing text was still delivered, but resuming will
    /// fault.
    #[must_use]
    pub fn did_safe_exit(&self) -> bool {
        self.flow.did_safe_exit
    }

    /// The knot or `knot.stitch` this flow is executing in — see
    /// [`Story::current_path`](super::Story::current_path). Hosts that
    /// drive instances directly (`bevy-brink`) pass the program they run.
    #[must_use]
    pub fn current_path(&self, program: &Program) -> Option<String> {
        self.flow
            .current_thread()
            .call_stack
            .last()
            .and_then(|frame| super::frame_path(program, frame))
            // The root scope's empty path is "no named container".
            .filter(|path| !path.is_empty())
    }

    /// Runtime statistics (instructions, materialization counts, etc.)
    /// accumulated over this flow's execution.
    #[must_use]
    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// Take every non-fatal [`crate::RuntimeWarning`] this flow has raised
    /// since the last drain, leaving the list empty (issue #3354).
    ///
    /// Draining rather than borrowing is deliberate: a host that prints
    /// warnings as it plays wants each one once, and a host that ignores
    /// them wants the list not to grow. Accumulation between drains is
    /// capped at [`crate::RUNTIME_WARNING_CAP`].
    pub fn take_runtime_warnings(&mut self) -> Vec<crate::RuntimeWarning> {
        core::mem::take(&mut self.flow.warnings)
    }

    /// The full append-only transcript of all output parts produced so far.
    ///
    /// The transcript stores structural references (e.g. `LineRef`) rather
    /// than resolved strings, so it can be re-rendered in any locale by
    /// passing a different set of line tables to
    /// [`transcript::render_transcript`](crate::transcript::render_transcript).
    #[must_use]
    pub fn transcript(&self) -> &[crate::output::OutputPart] {
        self.flow.output.transcript()
    }

    /// Number of parts in the transcript.
    #[must_use]
    pub fn transcript_len(&self) -> usize {
        self.flow.output.transcript_len()
    }

    /// Reset the transcript read cursor to the beginning (for re-rendering,
    /// e.g. after a locale swap).
    pub fn reset_cursor(&mut self) {
        self.flow.output.reset_cursor();
    }

    /// The fragments captured during execution (for re-rendering choice
    /// display text and computed substrings in a different locale).
    #[must_use]
    pub fn fragments(&self) -> &[crate::output::Fragment] {
        self.flow.output.fragments()
    }

    // ── External calls (ink → engine) ────────────────────────────────

    /// Returns `true` if this flow is frozen on an unresolved external
    /// call — i.e. the VM hit a `CallExternal` opcode and the handler
    /// returned [`ExternalResult::Pending`], leaving the `External` frame
    /// on top of the call stack.
    ///
    /// The orchestration layer (e.g. a Bevy resolver system) polls this to
    /// decide whether the flow needs an external resolved before it can be
    /// driven further. Resolve via [`resolve_external`](Self::resolve_external).
    #[must_use]
    pub fn has_pending_external(&self) -> bool {
        self.flow.external_fn_id().is_some()
    }

    /// The [`DefinitionId`] of the pending external function, if this flow
    /// is frozen on one. Returns `None` otherwise.
    #[must_use]
    pub fn pending_external_fn_id(&self) -> Option<DefinitionId> {
        self.flow.external_fn_id()
    }

    /// The arguments to the pending external call, in declaration order.
    /// Empty if no external call is pending.
    #[must_use]
    pub fn pending_external_args(&self) -> &[Value] {
        self.flow.external_args()
    }

    /// The ink-declared name of the pending external function, resolved
    /// against `program`'s name table. Returns `None` if no external is
    /// pending (or the entry is missing, which would indicate a malformed
    /// program).
    ///
    /// The orchestration layer uses this to look up the binding registered
    /// for this name.
    #[must_use]
    pub fn pending_external_name<'p>(&self, program: &'p Program) -> Option<&'p str> {
        let id = self.flow.external_fn_id()?;
        let entry = program.external_fn(id)?;
        Some(program.name(entry.name))
    }

    /// Resolve a pending external call by supplying its return value. Pops
    /// the `External` frame and pushes `value` onto the value stack so the
    /// VM can resume. For fire-and-forget externals, pass [`Value::Null`].
    ///
    /// No-op if no external call is pending. After resolving, drive the
    /// flow forward with [`step_single_line`](Self::step_single_line).
    pub fn resolve_external(&mut self, value: Value) {
        self.flow.resolve_external(value);
    }

    // ── Engine → ink calls ───────────────────────────────────────────

    /// Evaluate an ink function from engine code, returning its value.
    ///
    /// This does **not** advance the player-visible story: a
    /// `FunctionEvalFromGame` boundary frame is pushed, `args` are passed
    /// in declaration order (exactly as a normal call site would), output
    /// is captured and discarded, and the function runs until it returns.
    ///
    /// If the function calls an external whose handler returns
    /// [`ExternalResult::Pending`] (e.g. a binding that needs Bevy World
    /// access), evaluation pauses and returns
    /// [`FunctionEval::AwaitingExternal`]; the caller resolves the
    /// external (see [`resolve_external`](Self::resolve_external)) and
    /// calls [`resume_function_eval`](Self::resume_function_eval).
    ///
    /// `container_idx` is the function's container, typically obtained from
    /// [`Program::find_address`](crate::Program::find_address) on the
    /// function name. Unlike a normal `Call`, this does not increment the
    /// function's visit count — an engine query is out-of-band, matching
    /// C#'s `EvaluateFunction`.
    ///
    /// # Errors
    /// - [`AlreadyEvaluatingFunction`](RuntimeError::AlreadyEvaluatingFunction)
    ///   if a function evaluation is already in progress on this flow.
    /// - [`FunctionYielded`](RuntimeError::FunctionYielded) if the function
    ///   presents choices or ends the story (functions must not yield).
    /// - [`UnresolvedExternalCall`](RuntimeError::UnresolvedExternalCall)
    ///   if an external has neither a binding nor a fallback.
    #[expect(
        clippy::too_many_arguments,
        reason = "the VM environment (program, line tables, context, handler, resolver) plus the call target and args"
    )]
    pub fn begin_function_eval<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        container_idx: u32,
        args: &[Value],
        resolver: Option<&dyn PluralResolver>,
    ) -> Result<FunctionEval, RuntimeError> {
        self.begin_function_eval_with_limit::<R>(
            program,
            line_tables,
            context,
            handler,
            container_idx,
            args,
            resolver,
            Self::STEP_LIMIT,
        )
    }

    /// Like [`begin_function_eval`](Self::begin_function_eval), but the VM
    /// step budget for the whole evaluation is `step_limit` rather than the
    /// hardcoded [`Self::STEP_LIMIT`] (#1868).
    ///
    /// This is what lets a caller give an engine→ink evaluation its own,
    /// appropriately scoped budget — e.g. a compile-time registry walk,
    /// which wants a small ceiling of its own rather than the 1,000,000-step
    /// production default — mirroring how [`advance_with_limit`](Self::advance_with_limit)
    /// already lets [`crate::Speculation`] cap the line-stepping path.
    /// `begin_function_eval` itself is a thin wrapper over this with
    /// `step_limit: Self::STEP_LIMIT` — every existing call site keeps its
    /// exact prior behavior.
    ///
    /// # Errors
    /// Same as [`begin_function_eval`](Self::begin_function_eval), plus
    /// [`StepLimitExceeded`](RuntimeError::StepLimitExceeded) is now bounded
    /// by the caller-supplied `step_limit` rather than the fixed default.
    #[expect(
        clippy::too_many_arguments,
        reason = "the VM environment (program, line tables, context, handler, resolver) plus the call target, args, and step_limit"
    )]
    pub fn begin_function_eval_with_limit<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        container_idx: u32,
        args: &[Value],
        resolver: Option<&dyn PluralResolver>,
        step_limit: u64,
    ) -> Result<FunctionEval, RuntimeError> {
        // M-2b: refuse host-driven evaluation of a `#@private` function
        // while visibility enforcement is on (`docs/modules-spec.md` §4
        // boundary rule 2). Mirrors `Story::call_function`'s own check for
        // callers that drive this `FlowInstance` directly — `bevy-brink`,
        // `Speculation` — without going through `Story`. The caller
        // resolves `container_idx` itself (typically via
        // [`Program::find_address`](crate::Program::find_address) on the
        // function name), so the error names the definition by its compiled
        // id rather than the original name string, which isn't available
        // here.
        if self.enforce_visibility
            && program.has_private_defs()
            && program.container_is_private(container_idx)
        {
            return Err(RuntimeError::PrivateAccess {
                name: format!("{}", program.container(container_idx).id),
            });
        }
        if self.eval.is_some() {
            return Err(RuntimeError::AlreadyEvaluatingFunction);
        }

        // Record floors BEFORE pushing args: the value-stack length (so the
        // return value and any leftover args can be reclaimed), and the
        // pending-choice count (so we can tell a choice the function
        // presents from choices the main story already has waiting).
        let value_floor = self.flow.value_stack.len();
        let choice_floor = self.flow.pending_choices.len();

        // Isolate output: anything the function emits routes to the
        // capture scratch space and never reaches the transcript.
        self.flow.output.begin_capture();

        let output_start = self.flow.output.mark();
        let boundary = CallFrame {
            return_address: None,
            temps: Vec::new(),
            temps_written: Vec::new(),
            container_stack: vec![ContainerPosition {
                container_idx,
                offset: 0,
            }],
            frame_type: CallFrameType::FunctionEvalFromGame,
            external_fn_id: None,
            function_output_start: Some(output_start),
        };
        self.flow.current_thread_mut().call_stack.push(boundary);
        self.stats.frames_pushed += 1;

        // Pass arguments onto the value stack in declaration order — the
        // function's prologue (`DeclareTemp`) binds them exactly as it
        // would for an in-story call.
        self.flow.value_stack.extend_from_slice(args);

        self.eval = Some(EvalState {
            value_floor,
            choice_floor,
        });
        self.drive_function_eval::<R>(program, line_tables, context, handler, resolver, step_limit)
    }

    /// Evaluate an ink **function value** (`FnRef`/`Closure`) from engine
    /// code — the host callback-invocation surface (T1c-3,
    /// `docs/t1c-spec.md` §6). A function value crosses to the host as an
    /// opaque token `{DefinitionId, env}`; the host never dereferences the
    /// env — invocation always re-enters the VM here and is journaled
    /// exactly like [`begin_function_eval`](Self::begin_function_eval).
    ///
    /// `callee` must be a [`Value::FnRef`] / [`Value::Closure`]; `args`
    /// supply the remaining (val-only) params after the value's bound
    /// prefix. The same fault set as in-story dispatch applies — non-function
    /// callee, wrong arity, rehydration mismatch, cross-flow ref-`#@local`
    /// (`docs/t1c-spec.md` §3/§6) — surfaced as the `Err` here rather than as
    /// a turn-terminating story fault, since this is out-of-band evaluation.
    ///
    /// Like [`begin_function_eval`](Self::begin_function_eval) this does not
    /// advance the player-visible story (output isolated, transcript
    /// untouched, no visit-count increment) and pauses on world-access
    /// externals — resume with
    /// [`resume_function_eval`](Self::resume_function_eval).
    ///
    /// # Errors
    /// - [`AlreadyEvaluatingFunction`](RuntimeError::AlreadyEvaluatingFunction)
    ///   if an evaluation is already in progress on this flow.
    /// - The function-value dispatch faults above (via
    ///   `vm::prepare_fn_value_call`), before any frame is pushed.
    /// - The same evaluation errors as
    ///   [`begin_function_eval`](Self::begin_function_eval).
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors begin_function_eval: the VM environment plus the callee value and args"
    )]
    pub fn begin_function_value_eval<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        callee: &Value,
        args: &[Value],
        resolver: Option<&dyn PluralResolver>,
    ) -> Result<FunctionEval, RuntimeError> {
        self.begin_function_value_eval_with_limit::<R>(
            program,
            line_tables,
            context,
            handler,
            callee,
            args,
            resolver,
            Self::STEP_LIMIT,
        )
    }

    /// Like [`begin_function_value_eval`](Self::begin_function_value_eval),
    /// but the VM step budget for the whole evaluation is `step_limit`
    /// rather than the hardcoded [`Self::STEP_LIMIT`] (#1868) — the function-value
    /// sibling of [`begin_function_eval_with_limit`](Self::begin_function_eval_with_limit).
    ///
    /// # Errors
    /// Same as [`begin_function_value_eval`](Self::begin_function_value_eval),
    /// plus [`StepLimitExceeded`](RuntimeError::StepLimitExceeded) is now
    /// bounded by the caller-supplied `step_limit` rather than the fixed
    /// default.
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors begin_function_eval_with_limit: the VM environment plus the callee value, args, and step_limit"
    )]
    pub fn begin_function_value_eval_with_limit<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        callee: &Value,
        args: &[Value],
        resolver: Option<&dyn PluralResolver>,
        step_limit: u64,
    ) -> Result<FunctionEval, RuntimeError> {
        if self.eval.is_some() {
            return Err(RuntimeError::AlreadyEvaluatingFunction);
        }

        // Validate + assemble the full arg row (bound prefix then supplied)
        // through the shared dispatch path, so a bad callee faults *before*
        // any boundary frame or capture scope is set up — no partial state.
        let (container_idx, _target, full_args) =
            vm::prepare_fn_value_call(program, callee, args.to_vec())?;

        // M-2b: refuse a `#@private` function value the same way
        // `begin_function_eval` refuses a `#@private` name — this is the
        // sibling engine→ink call-dispatch path (T1c function values), sharing
        // the same `container_idx` resolve-then-enter shape, so it shares the
        // same gap and the same fix. Checked before any boundary frame or
        // capture scope is set up, same as the `eval.is_some()` check above.
        if self.enforce_visibility
            && program.has_private_defs()
            && program.container_is_private(container_idx)
        {
            return Err(RuntimeError::PrivateAccess {
                name: format!("{}", program.container(container_idx).id),
            });
        }

        let value_floor = self.flow.value_stack.len();
        let choice_floor = self.flow.pending_choices.len();

        self.flow.output.begin_capture();
        let output_start = self.flow.output.mark();
        let boundary = CallFrame {
            return_address: None,
            temps: Vec::new(),
            temps_written: Vec::new(),
            container_stack: vec![ContainerPosition {
                container_idx,
                offset: 0,
            }],
            frame_type: CallFrameType::FunctionEvalFromGame,
            external_fn_id: None,
            function_output_start: Some(output_start),
        };
        self.flow.current_thread_mut().call_stack.push(boundary);
        self.stats.frames_pushed += 1;

        // Pass the full arg row (bound prefix then supplied) onto the value
        // stack in declaration order — the prologue binds it exactly as an
        // in-story call would.
        self.flow.value_stack.extend_from_slice(&full_args);

        self.eval = Some(EvalState {
            value_floor,
            choice_floor,
        });
        self.drive_function_eval::<R>(program, line_tables, context, handler, resolver, step_limit)
    }

    /// Resume a function evaluation that paused on
    /// [`FunctionEval::AwaitingExternal`], after the pending external has
    /// been resolved via [`resolve_external`](Self::resolve_external).
    ///
    /// # Errors
    /// - [`NotEvaluatingFunction`](RuntimeError::NotEvaluatingFunction) if
    ///   no evaluation is in progress.
    /// - Same evaluation errors as
    ///   [`begin_function_eval`](Self::begin_function_eval).
    pub fn resume_function_eval<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        resolver: Option<&dyn PluralResolver>,
    ) -> Result<FunctionEval, RuntimeError> {
        self.resume_function_eval_with_limit::<R>(
            program,
            line_tables,
            context,
            handler,
            resolver,
            Self::STEP_LIMIT,
        )
    }

    /// Like [`resume_function_eval`](Self::resume_function_eval), but the VM
    /// step budget for the remainder of the evaluation is `step_limit`
    /// rather than the hardcoded [`Self::STEP_LIMIT`] (#1868). A caller that
    /// began the evaluation with
    /// [`begin_function_eval_with_limit`](Self::begin_function_eval_with_limit)/
    /// [`begin_function_value_eval_with_limit`](Self::begin_function_value_eval_with_limit)
    /// should resume with the same `step_limit` to keep one consistent
    /// budget across pauses — this call's step count starts fresh (mirrors
    /// [`advance_with_limit`](Self::advance_with_limit): each call gets its
    /// own `step_limit`-sized allowance, not a running total).
    ///
    /// # Errors
    /// Same as [`resume_function_eval`](Self::resume_function_eval), plus
    /// [`StepLimitExceeded`](RuntimeError::StepLimitExceeded) is now bounded
    /// by the caller-supplied `step_limit` rather than the fixed default.
    pub fn resume_function_eval_with_limit<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        resolver: Option<&dyn PluralResolver>,
        step_limit: u64,
    ) -> Result<FunctionEval, RuntimeError> {
        if self.eval.is_none() {
            return Err(RuntimeError::NotEvaluatingFunction);
        }
        self.drive_function_eval::<R>(program, line_tables, context, handler, resolver, step_limit)
    }

    /// Returns `true` if a function evaluation is in progress (possibly
    /// paused awaiting an external).
    #[must_use]
    pub fn is_evaluating_function(&self) -> bool {
        self.eval.is_some()
    }

    /// Step the VM until the in-progress function evaluation returns or
    /// pauses on a pending external. Shared by `begin`/`resume`. `step_limit`
    /// bounds this call's own step loop (#1868) — see
    /// [`begin_function_eval_with_limit`](Self::begin_function_eval_with_limit)
    /// for why a caller-supplied budget matters here.
    fn drive_function_eval<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        resolver: Option<&dyn PluralResolver>,
        step_limit: u64,
    ) -> Result<FunctionEval, RuntimeError> {
        let step_start = self.stats.steps;
        loop {
            self.stats.steps += 1;
            if self.stats.steps - step_start > step_limit {
                self.abort_eval(program, line_tables, resolver);
                return Err(RuntimeError::StepLimitExceeded(step_limit));
            }

            let stepped = vm::step::<R>(
                &mut self.flow,
                program,
                line_tables,
                context,
                &mut self.stats,
                resolver,
            )?;
            self.stats.materializations += self.flow.drain_materializations();

            match stepped {
                vm::Stepped::Done | vm::Stepped::Ended => {
                    // A function reached `-> DONE`/`-> END` — illegal.
                    self.abort_eval(program, line_tables, resolver);
                    return Err(RuntimeError::FunctionYielded);
                }
                vm::Stepped::ExternalCall => {
                    if let Some(pending) =
                        self.resolve_eval_external(program, line_tables, resolver, handler)?
                    {
                        return Ok(pending);
                    }
                }
                vm::Stepped::Continue | vm::Stepped::ThreadCompleted => {}
            }

            // Did the boundary frame pop? Then the function has returned
            // (via `~ return` or implicit exhaustion).
            if !self.flow.has_eval_boundary() {
                let _captured = self.flow.output.end_capture(program, line_tables, resolver);
                let floor = self.eval.take().map_or(0, |e| e.value_floor);
                let mut ret: Option<Value> = None;
                while self.flow.value_stack.len() > floor {
                    let v = self.flow.value_stack.pop();
                    if ret.is_none() {
                        ret = v; // first popped = top of stack = the return value
                    }
                }
                return Ok(FunctionEval::Returned(ret.unwrap_or(Value::Null)));
            }

            // A function must not present choices. Compare against the
            // count when the eval began — the main story may already have
            // choices waiting, which are none of our concern.
            let choice_floor = self.eval.as_ref().map_or(0, |e| e.choice_floor);
            if self.flow.pending_choices.len() > choice_floor {
                self.abort_eval(program, line_tables, resolver);
                return Err(RuntimeError::FunctionYielded);
            }
        }
    }

    /// Resolve an external hit during function evaluation, mirroring the
    /// normal step path but surfacing [`ExternalResult::Pending`] as
    /// [`FunctionEval::AwaitingExternal`] (returned as `Some`) rather than
    /// an error. Returns `None` when the external resolved and stepping
    /// should continue.
    fn resolve_eval_external(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        resolver: Option<&dyn PluralResolver>,
        handler: &dyn ExternalFnHandler,
    ) -> Result<Option<FunctionEval>, RuntimeError> {
        let fn_id = self
            .flow
            .external_fn_id()
            .ok_or(RuntimeError::CallStackUnderflow)?;
        let entry = program.external_fn(fn_id);
        let fn_name = entry.map_or("?", |e| program.name(e.name));
        match handler.call(fn_name, self.flow.external_args()) {
            ExternalResult::Resolved(value) => {
                self.flow.resolve_external(value);
                Ok(None)
            }
            ExternalResult::Fallback => {
                if let Some(fb_id) = entry.and_then(|e| e.fallback) {
                    let container_idx = program
                        .resolve_target(fb_id)
                        .map(|(idx, _)| idx)
                        .ok_or(RuntimeError::UnresolvedDefinition(fb_id))?;
                    self.flow.invoke_fallback(container_idx);
                    Ok(None)
                } else {
                    self.abort_eval(program, line_tables, resolver);
                    Err(RuntimeError::UnresolvedExternalCall(fn_id))
                }
            }
            ExternalResult::Pending => Ok(Some(FunctionEval::AwaitingExternal)),
        }
    }

    /// Tear down an aborted/failed evaluation: end the output capture and
    /// clear the eval marker. Leaves the call stack as-is (the caller is
    /// erroring out).
    pub(crate) fn abort_eval(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        resolver: Option<&dyn PluralResolver>,
    ) {
        if self.eval.take().is_some() {
            let _ = self.flow.output.end_capture(program, line_tables, resolver);
        }
    }
}

/// Outcome of [`apply_done_bookkeeping`] — mirrors the branches
/// [`FlowInstance::step_single_line`]'s own `vm::Stepped::Done` arm takes,
/// minus the buffered-output handling the `debug-hooks` seam
/// (`Story::debug_run`/`debug_step`/`debug_run_watching`) doesn't use.
#[cfg(feature = "debug-hooks")]
pub(crate) enum DoneBookkeeping {
    /// All pending choices were invisible defaults and got auto-selected —
    /// `status` is `Active` again; the caller should keep stepping, not
    /// treat this as a stop.
    AutoSelected,
    /// Real choices are pending; `status` is now `WaitingForChoice`.
    WaitingForChoice,
    /// No choices pending; `status` is now `Done` (mirrors an explicit
    /// `-> DONE` or the flow otherwise running out of content).
    Terminal,
}

/// Apply the same bookkeeping `step_single_line`'s per-turn loop performs
/// when `vm::step` returns `vm::Stepped::Done` (turn-index bump,
/// invisible-default auto-select, `WaitingForChoice`/`Done` status) —
/// shared with the `debug-hooks` seam so a choice/turn boundary reached
/// via opcode-level debug stepping leaves the same `FlowInstance` state a
/// production-path caller would see: `Story::choose()` and
/// `Story::continue_single()` keep working after a debug session hands
/// control back to the production API, and the turn index never diverges
/// between the two (issue #3186 review).
#[cfg(feature = "debug-hooks")]
#[expect(
    clippy::similar_names,
    reason = "status/stats mirrors select_choice's identical pair"
)]
pub(crate) fn apply_done_bookkeeping(
    flow: &mut Flow,
    context: &mut (impl ContextAccess + ?Sized),
    status: &mut StoryStatus,
    stats: &mut Stats,
) -> Result<DoneBookkeeping, RuntimeError> {
    context.increment_turn_index();

    if !flow.pending_choices.is_empty() {
        let all_invisible = flow
            .pending_choices
            .iter()
            .all(|pc| pc.flags.is_invisible_default);
        if all_invisible {
            select_choice(flow, context, status, stats, 0)?;
            return Ok(DoneBookkeeping::AutoSelected);
        }
    }

    if flow.pending_choices.is_empty() {
        *status = StoryStatus::Done;
        Ok(DoneBookkeeping::Terminal)
    } else {
        *status = StoryStatus::WaitingForChoice;
        stats.choices_presented += 1;
        Ok(DoneBookkeeping::WaitingForChoice)
    }
}

/// Internal: set execution position to the given choice target, clear
/// pending choices, and set status to Active. No status precondition.
#[expect(clippy::similar_names)]
/// Returns the `DefinitionId` of the selected choice target, so the
/// caller can notify observers if needed.
fn select_choice(
    flow: &mut Flow,
    context: &mut (impl ContextAccess + ?Sized),
    status: &mut StoryStatus,
    stats: &mut Stats,
    index: usize,
) -> Result<(), RuntimeError> {
    let available = flow.pending_choices.len();
    if index >= available {
        return Err(RuntimeError::InvalidChoiceIndex { index, available });
    }

    let choice = flow.pending_choices.swap_remove(index);
    let target_id = choice.target_id;

    // Increment visit count for the choice target container so that
    // once-only choices can be filtered on subsequent passes.
    context.increment_visit(target_id);
    context.set_turn_count(target_id, context.turn_index());

    // Replace the current thread with the fork from choice creation
    // time. By selection time, all spawned threads should have
    // completed — only the main thread remains.
    let current = flow.current_thread_mut();
    *current = choice.thread_fork;

    // Set execution position to the choice target. We reset the top
    // frame's container_stack to just the target — the snapshot may
    // have captured stale nesting from inside the choice eval block.
    let frame = current
        .call_stack
        .last_mut()
        .ok_or(RuntimeError::CallStackUnderflow)?;

    frame.container_stack.clear();
    frame.container_stack.push(ContainerPosition {
        container_idx: choice.target_idx,
        offset: choice.target_offset,
    });

    flow.pending_choices.clear();
    // No explicit pending-terminal clear needed here either (same reasoning
    // as `choose_path_string_with_args`): `next_block_id`'s bump below moves
    // this choice to a fresh run, which is exactly what `PendingTerminal`
    // uses to invalidate a stash from before the choice — so the next step
    // correctly runs the VM at the chosen target rather than replaying
    // whatever `Done`/`Choices`/`End` was pending before selection.
    *status = StoryStatus::Active;
    stats.choices_selected += 1;
    // A fresh run begins at the chosen branch (`BlockId`, §3.7/§8d.2).
    flow.next_block_id += 1;
    flow.line_delivered_this_turn = false;

    Ok(())
}

/// Resolve an external function call using the handler and program metadata.
///
/// Returns `Ok(true)` if the call was resolved (a value was supplied or the
/// in-story fallback was invoked) and stepping should continue; `Ok(false)`
/// if the handler deferred ([`ExternalResult::Pending`]), leaving the
/// `External` frame intact for the caller to resolve out-of-band. Errors
/// only when the handler declined and no fallback exists.
///
/// `pub(super)` since #3224: the `Story` debug loops resolve externals
/// through this exact function, so debug and production stepping can
/// never disagree about binding semantics.
pub(super) fn resolve_external_call(
    flow: &mut Flow,
    program: &Program,
    handler: &dyn ExternalFnHandler,
) -> Result<bool, RuntimeError> {
    let fn_id = flow
        .external_fn_id()
        .ok_or(RuntimeError::CallStackUnderflow)?;

    let entry = program.external_fn(fn_id);
    let fn_name = entry.map_or("?", |e| program.name(e.name));

    let result = handler.call(fn_name, flow.external_args());
    match result {
        ExternalResult::Resolved(value) => {
            flow.resolve_external(value);
            Ok(true)
        }
        ExternalResult::Fallback => {
            let fallback_id = entry.and_then(|e| e.fallback);
            if let Some(fb_id) = fallback_id {
                let container_idx = program
                    .resolve_target(fb_id)
                    .map(|(idx, _)| idx)
                    .ok_or(RuntimeError::UnresolvedDefinition(fb_id))?;

                flow.invoke_fallback(container_idx);
                Ok(true)
            } else {
                Err(RuntimeError::UnresolvedExternalCall(fn_id))
            }
        }
        ExternalResult::Pending => {
            // Leave the External frame intact — the caller resolves it
            // out-of-band (via resolve_external) before continuing.
            Ok(false)
        }
    }
}

/// Flush remaining output buffer content into `(text, tags, element_data)`.
///
/// At a yield point (Done/Choices/Ended), no more output is coming, so
/// trailing newlines are committed. Lines are joined with `\n`, tags are
/// flattened into a single vec, and element-attachment data (issue #2108) is
/// merged the same way — later lines' keys win on conflict, matching the
/// existing "just flatten" precision this function already had for tags:
/// multiple flushed-at-once lines belonging to genuinely different attach
/// runs is a pre-existing imprecision this fix does not newly introduce.
/// `flush_remaining`'s flattened yield-time output: joined text, all tags,
/// merged element data, and the FIRST flushed line's source location
/// (W7/#3300 provenance — the run "is" where it starts).
type FlushedRemaining = (
    String,
    Vec<String>,
    BTreeMap<String, String>,
    Option<brink_format::SourceLocation>,
);

fn flush_remaining(
    flow: &mut Flow,
    program: &Program,
    line_tables: &[Vec<brink_format::LineEntry>],
    resolver: Option<&dyn brink_format::PluralResolver>,
) -> FlushedRemaining {
    let lines = flow.output.flush_lines_at_yield(
        program,
        line_tables,
        resolver,
        flow.line_delivered_this_turn,
    );
    let mut text = String::new();
    let mut tags = Vec::new();
    let mut element = BTreeMap::new();
    let mut source: Option<brink_format::SourceLocation> = None;
    for (i, (line_text, line_tags, line_element, line_source)) in lines.iter().enumerate() {
        if i > 0 {
            text.push('\n');
        }
        text.push_str(line_text);
        tags.extend_from_slice(line_tags);
        element.extend(line_element.iter().map(|(k, v)| (k.clone(), v.clone())));
        // First line's provenance wins — the flushed run "is" where it starts.
        if source.is_none() {
            source.clone_from(line_source);
        }
    }
    (text, tags, element, source)
}

/// Build a [`Step::Line`] stamped with the flow's current [`BlockId`] and
/// its [`Element`] classification.
///
/// Issue #2108 (`docs/decision-log.md` 2026-08-03 "The element output
/// model") populates `element.data` from `data` — the per-line element-
/// attachment snapshot [`OutputBuffer::take_first_line`]/`flush_lines`
/// already resolved from the output buffer's own transcript (see
/// `OutputPart::ElementAttach`'s doc for why it lives there rather than on
/// `Flow`). The common case — no attach convention preceded this line —
/// passes an empty map, falling back to the always-correct
/// [`Element::narrative`] default, unchanged from #1683.
///
/// `element.kind` stays [`Element::NARRATIVE`] either way: only *data* is
/// populated here. Classifying `kind` itself for a non-attach single-line
/// handler (`heading`/`transition` reporting their own handler name) is a
/// distinct, separately-tractable gap this PR does not close — see
/// `docs/decision-log.md`/this issue's follow-up notes.
fn make_output_line(
    flow: &mut Flow,
    text: String,
    tags: Vec<String>,
    data: BTreeMap<String, String>,
    source: Option<brink_format::SourceLocation>,
) -> Step {
    flow.line_delivered_this_turn = true;
    let element = if data.is_empty() {
        Element::narrative()
    } else {
        Element {
            kind: Element::NARRATIVE.to_string(),
            data,
        }
    };
    Step::Line(OutputLine {
        text,
        tags,
        block_id: BlockId(flow.next_block_id),
        element,
        source,
    })
}

/// Collect the currently pending choices into the public [`Choice`] shape,
/// resolving each display text (trimming spaces/tabs, matching C#:
/// `choice.text = (startText + choiceOnlyText).Trim(' ', '\t')`).
fn collect_choices(
    flow: &Flow,
    program: &Program,
    line_tables: &[Vec<brink_format::LineEntry>],
    resolver: Option<&dyn brink_format::PluralResolver>,
) -> Vec<Choice> {
    // `index` counts visible choices only — C#'s `currentChoices`
    // numbering, and what `choose` takes (issue #3527).
    flow.pending_choices
        .iter()
        .filter(|pc| !pc.flags.is_invisible_default)
        .enumerate()
        .map(|(i, pc)| {
            let display_text = match &pc.display {
                ChoiceDisplay::Text(s) => s.clone(),
                ChoiceDisplay::Fragment(idx) => {
                    flow.output
                        .resolve_fragment(*idx, program, line_tables, resolver)
                }
            };
            let display_text = display_text
                .trim_matches(|c: char| c == ' ' || c == '\t')
                .to_string();
            let source = match &pc.display {
                ChoiceDisplay::Fragment(idx) => {
                    flow.output.fragment_source(*idx, program, line_tables)
                }
                ChoiceDisplay::Text(_) => None,
            };
            Choice {
                text: display_text,
                index: i,
                tags: pc.tags.clone(),
                sticky: !pc.flags.once_only,
                source,
            }
        })
        .collect()
}

/// Build the terminal [`Step`] for a yield point (`WaitingForChoice`/
/// `Done`/`Ended`) based on the current story status.
///
/// Terminals carry no text (`docs/prose-dialect-spec.md` §7, RULED) —
/// `Line`'s old fused shape no longer exists. If `text`/`tags` is
/// non-empty, it's delivered first as its own `Step::Line`, and the bare
/// terminal is stashed on `flow.pending_terminal` for the very next
/// `advance` call to return with no further VM stepping. If there's
/// nothing to flush, the bare terminal is returned immediately.
#[expect(
    clippy::too_many_arguments,
    reason = "issue #2108's `element` param pushed this past 7; each param is \
              a distinct piece of the terminal/line it builds, not a natural group"
)]
fn yield_step(
    status: StoryStatus,
    text: String,
    tags: Vec<String>,
    element: BTreeMap<String, String>,
    source: Option<brink_format::SourceLocation>,
    flow: &mut Flow,
    program: &Program,
    line_tables: &[Vec<brink_format::LineEntry>],
    resolver: Option<&dyn brink_format::PluralResolver>,
) -> Step {
    let terminal = match status {
        StoryStatus::WaitingForChoice => {
            Step::Choices(collect_choices(flow, program, line_tables, resolver))
        }
        StoryStatus::Ended => Step::End,
        StoryStatus::Done => Step::Done,
        // Defensive fallback — `yield_step` is only ever called once
        // `status` has transitioned away from `Active` at a genuine yield
        // point (see call sites), so this arm is unreachable in practice.
        StoryStatus::Active => return make_output_line(flow, text, tags, element, source),
    };

    if text.is_empty() && tags.is_empty() {
        terminal
    } else {
        flow.pending_terminal.stash(flow.next_block_id, terminal);
        make_output_line(flow, text, tags, element, source)
    }
}
