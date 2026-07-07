//! [`Speculation`] — a composable, self-contained, side-effect-proof
//! speculative run over a story's current state.
//!
//! This is the F4.1 stage of the speculative-eval work: the runtime
//! primitive celeris's watch/eval and bevy's inspector compose on. A
//! `Speculation` is a [`Mode::Sandbox`] fork (see `crate::world`) of the
//! current story state that the caller drives with the ordinary verbs
//! (`advance`, `choose`, `go_to_path`, `eval_function`, …) and then
//! discards. It reads current state (an owned snapshot, taken at fork
//! time); every write it makes is diverted into its own sandboxed
//! [`FlowLocal`] overrides and is discarded on drop — the live [`World`]
//! and [`FlowInstance`] it was forked from are never mutated.
//!
//! `Speculation` exposes verbs, not a pre-composed runner: driving it to a
//! terminal line, probing multiple branches, or bailing out early is the
//! caller's loop to write. The only thing `Speculation` adds beyond what
//! [`FlowInstance`] already offers is a caller-supplied [`Budget`] in place
//! of the runtime's hardcoded step/line ceilings, so a speculative probe
//! over possibly-malformed or adversarial bytecode fails fast instead of
//! silently burning the full production budget (1,000,000 steps) before
//! giving up.
//!
//! No existing construction path creates a `Speculation` — it is reached
//! only via [`Speculation::fork_from`] or [`crate::Story::speculate`],
//! neither of which any pre-F4.1 code calls. This keeps the change purely
//! additive: the oracle corpus, which never constructs a `Speculation`,
//! is unaffected.

use std::marker::PhantomData;
use std::sync::Arc;

use brink_format::Value;

use crate::error::RuntimeError;
use crate::program::Program;
use crate::rng::{FastRng, StoryRng};
use crate::story::{ExternalFnHandler, FlowInstance, FunctionEval, Line, StepOutcome};
use crate::world::{ContextView, FlowLocal, Mode, World};

/// Caller-supplied cap on a [`Speculation`]'s VM stepping, in place of the
/// runtime's hardcoded `STEP_LIMIT`/`LINE_LIMIT` ceilings.
///
/// - `steps` bounds a single [`Speculation::advance`] call's inner VM step
///   loop (mirrors [`FlowInstance`]'s private `STEP_LIMIT`, 1,000,000 in
///   production).
/// - `lines` bounds the total number of visible lines a `Speculation` may
///   produce over its lifetime, across however many `advance` calls the
///   caller makes (mirrors [`crate::Story`]'s private `LINE_LIMIT`, 10,000
///   in production).
///
/// The [`Default`] is well under both production ceilings — a speculative
/// probe is expected to be short-lived, so a runaway one should fail fast
/// rather than approach the live budgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Max VM steps for a single `advance` call.
    pub steps: u64,
    /// Max visible lines this `Speculation` may ever produce.
    pub lines: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            steps: 100_000,
            lines: 1_000,
        }
    }
}

/// Outcome of a single [`Speculation::advance`] call.
///
/// Mirrors [`StepOutcome`], the [`FlowInstance::advance`] equivalent: a
/// [`Line`] (including a terminal `Done`/`Choices`/`End` variant), or a
/// cleanly-surfaced pending external for the caller to resolve (see
/// [`Speculation::resolve_external`]) before calling `advance` again. A
/// budget-exhausted call is an `Err`, not a variant here — see
/// [`RuntimeError::StepLimitExceeded`]/[`RuntimeError::LineLimitExceeded`].
#[derive(Debug, Clone)]
pub enum SpeculationStep {
    /// A line of output, or a yield point (`Done`/`Choices`/`End`).
    Line(Line),
    /// The speculation paused on a deferred external; resolve it and
    /// `advance` again.
    AwaitingExternal,
}

/// A sandboxed, self-contained speculative run over a story's current
/// state. See the module docs for the full picture.
///
/// Owns everything it needs to drive itself: an `Arc`-shared [`Program`]
/// (cheap to bump, never mutated), an owned clone of the source [`World`]
/// (read through, never written back), a [`Mode::Sandbox`] fork of the
/// source [`FlowLocal`] (where every write lands, and which is simply
/// dropped to discard them), and a clone of the source [`FlowInstance`]'s
/// current execution position. Dropping a `Speculation` is the entire
/// discard operation — nothing it did ever reached the state it was
/// forked from.
pub struct Speculation<R: StoryRng = FastRng> {
    program: Arc<Program>,
    line_tables: Vec<Vec<brink_format::LineEntry>>,
    world: World,
    local: FlowLocal,
    flow: FlowInstance,
    /// Running count of lines produced across every `advance` call so far
    /// on this speculation, checked against each call's `budget.lines`.
    lines_advanced: usize,
    _rng: PhantomData<R>,
}

impl<R: StoryRng> Speculation<R> {
    /// Fork a `Speculation` from an arbitrary flow's current state.
    ///
    /// This is the composable, flow-level constructor — bevy-brink (or any
    /// other orchestration layer juggling multiple [`FlowInstance`]s over
    /// possibly-distinct [`World`]s) can call this directly for any flow,
    /// not just a [`crate::Story`]'s default one. [`crate::Story::speculate`]
    /// is a convenience wrapper over this for the common case.
    ///
    /// `world` is cloned (an owned snapshot — the source `World` is never
    /// touched again by this speculation), `local` is forked in
    /// [`Mode::Sandbox`] (every unit routes `Local`; writes never reach
    /// `world` or `local`'s own future writes), and `flow` is cloned to
    /// capture the exact execution position to speculate from.
    #[must_use]
    pub fn fork_from(
        program: Arc<Program>,
        world: &World,
        local: &FlowLocal,
        flow: &FlowInstance,
        line_tables: &[Vec<brink_format::LineEntry>],
    ) -> Self {
        Self {
            program,
            line_tables: line_tables.to_vec(),
            world: world.clone(),
            local: local.fork(Mode::Sandbox),
            flow: flow.clone(),
            lines_advanced: 0,
            _rng: PhantomData,
        }
    }

    /// Move this speculation's play head to a named knot/stitch path —
    /// the speculative equivalent of [`FlowInstance::choose_path_string`].
    /// Only this speculation's own (sandboxed) position moves; the flow it
    /// was forked from is untouched.
    ///
    /// # Errors
    /// Same as [`FlowInstance::choose_path_string`]: an unknown path, a
    /// jump while parked on an unresolved external, or a jump while a
    /// function evaluation is in progress.
    pub fn go_to_path(&mut self, path: &str) -> Result<(), RuntimeError> {
        let mut view = ContextView::new(&mut self.world, &mut self.local);
        self.flow.choose_path_string(&self.program, &mut view, path)
    }

    /// Select a pending choice by index — the speculative equivalent of
    /// [`FlowInstance::choose`].
    ///
    /// # Errors
    /// [`RuntimeError::NotWaitingForChoice`] if this speculation isn't
    /// waiting on a choice; [`RuntimeError::InvalidChoiceIndex`] for an
    /// out-of-range index.
    pub fn choose(&mut self, index: usize) -> Result<(), RuntimeError> {
        let mut view = ContextView::new(&mut self.world, &mut self.local);
        self.flow.choose(&mut view, index)
    }

    /// Drive this speculation forward by one visible line, honoring
    /// `budget` in place of the runtime's hardcoded step/line ceilings.
    ///
    /// Checks `budget.lines` against the running total of lines this
    /// speculation has already produced *before* stepping — a speculation
    /// that has already hit its line budget errors immediately rather
    /// than stepping further. Otherwise drives exactly one visible line
    /// (or a yield point) via [`FlowInstance::advance`]'s machinery,
    /// capped at `budget.steps` VM steps instead of the production
    /// 1,000,000.
    ///
    /// A deferred external ([`crate::ExternalResult::Pending`]) surfaces
    /// as [`SpeculationStep::AwaitingExternal`] — resolve it via
    /// [`resolve_external`](Self::resolve_external) and call `advance`
    /// again, exactly as [`FlowInstance::advance`]'s callers do.
    ///
    /// # Errors
    /// [`RuntimeError::LineLimitExceeded`] if `budget.lines` has already
    /// been reached; [`RuntimeError::StepLimitExceeded`] if `budget.steps`
    /// is exhausted before a line completes; any other error `advance`
    /// itself can produce (e.g. [`RuntimeError::StoryEnded`]).
    pub fn advance(
        &mut self,
        budget: Budget,
        handler: &dyn ExternalFnHandler,
    ) -> Result<SpeculationStep, RuntimeError> {
        if self.lines_advanced >= budget.lines {
            return Err(RuntimeError::LineLimitExceeded(budget.lines));
        }

        let mut view = ContextView::new(&mut self.world, &mut self.local);
        let outcome = self.flow.advance_with_limit::<R>(
            &self.program,
            &self.line_tables,
            &mut view,
            handler,
            None,
            budget.steps,
        )?;

        if let StepOutcome::Line(_) = &outcome {
            self.lines_advanced += 1;
        }

        Ok(match outcome {
            StepOutcome::Line(line) => SpeculationStep::Line(line),
            StepOutcome::AwaitingExternal => SpeculationStep::AwaitingExternal,
        })
    }

    /// Evaluate an ink function on this speculation, returning its value —
    /// the speculative equivalent of [`crate::Story::call_function`] /
    /// [`FlowInstance::begin_function_eval`]. Output is isolated (as for
    /// any engine→ink call) and, being on the sandboxed fork, can never
    /// escape to the live story either way.
    ///
    /// # Errors
    /// [`RuntimeError::FunctionNotFound`] for an unknown name;
    /// [`RuntimeError::ArgCountMismatch`] if `args.len()` doesn't match the
    /// function's declared parameter count; any error
    /// [`FlowInstance::begin_function_eval`] itself can produce.
    pub fn eval_function(
        &mut self,
        name: &str,
        args: &[Value],
        handler: &dyn ExternalFnHandler,
    ) -> Result<FunctionEval, RuntimeError> {
        let container_idx = self
            .program
            .find_address(name)
            .ok_or_else(|| RuntimeError::FunctionNotFound(name.to_owned()))?
            .0;
        let expected = self.program.container(container_idx).param_count;
        if args.len() != expected as usize {
            return Err(RuntimeError::ArgCountMismatch {
                target: name.to_owned(),
                expected,
                got: args.len(),
            });
        }
        let mut view = ContextView::new(&mut self.world, &mut self.local);
        self.flow.begin_function_eval::<R>(
            &self.program,
            &self.line_tables,
            &mut view,
            handler,
            container_idx,
            args,
            None,
        )
    }

    /// Resume a function evaluation on this speculation that paused on
    /// [`FunctionEval::AwaitingExternal`], after the pending external has
    /// been resolved via [`resolve_external`](Self::resolve_external) —
    /// the speculative equivalent of
    /// [`FlowInstance::resume_function_eval`].
    ///
    /// Closes the F4.1 gap: [`eval_function`](Self::eval_function) could
    /// pause on a deferred external with no way to continue. The full
    /// cycle is `eval_function` → `AwaitingExternal` →
    /// `resolve_external(value)` → `resume_function_eval` →
    /// `Returned(value)`.
    ///
    /// # Errors
    /// [`RuntimeError::NotEvaluatingFunction`] if no evaluation is in
    /// progress on this speculation; any error
    /// [`FlowInstance::resume_function_eval`] itself can produce.
    pub fn resume_function_eval(
        &mut self,
        handler: &dyn ExternalFnHandler,
    ) -> Result<FunctionEval, RuntimeError> {
        let mut view = ContextView::new(&mut self.world, &mut self.local);
        self.flow.resume_function_eval::<R>(
            &self.program,
            &self.line_tables,
            &mut view,
            handler,
            None,
        )
    }

    /// Resolve a pending external call on this speculation by supplying
    /// its return value. No-op if none is pending. See
    /// [`FlowInstance::resolve_external`].
    pub fn resolve_external(&mut self, value: Value) {
        self.flow.resolve_external(value);
    }

    /// The ink-declared name of the external this speculation is paused
    /// on, if any. See [`FlowInstance::pending_external_name`].
    #[must_use]
    pub fn pending_external_name(&self) -> Option<&str> {
        self.flow.pending_external_name(&self.program)
    }

    /// The append-only transcript of output parts produced by this
    /// speculation so far — structural references, resolved against
    /// whatever line tables the caller has (see
    /// [`FlowInstance::transcript`]). Empty for a freshly-forked
    /// speculation that hasn't been driven yet.
    #[must_use]
    pub fn transcript(&self) -> &[crate::output::OutputPart] {
        self.flow.transcript()
    }
}
