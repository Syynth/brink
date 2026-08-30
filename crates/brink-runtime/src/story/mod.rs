//! Per-instance mutable story state.

use core::marker::PhantomData;
use core::ops::Range;

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

use brink_format::{DefinitionId, PluralResolver, Value};

use crate::collections::Map as HashMap;
use crate::error::RuntimeError;
use crate::program::Program;
use crate::rng::{FastRng, StoryRng};
use crate::state::{ContextAccess, WriteObserver};
#[cfg(any(feature = "testing", feature = "debug-hooks"))]
use crate::vm;
use crate::world::{ContextView, FlowLocal, World};

mod call_stack;
mod external;
mod flow_instance;
mod types;

pub use call_stack::ExecMode;
pub(crate) use call_stack::{
    CallFrame, CallFrameType, ChoiceDisplay, ContainerPosition, Flow, PendingChoice,
    PureCallbackState, classify_ran_out_of_content,
};
// Only test fixtures across the op-table modules construct a bare `Flow`
// literal (production code reaches `pending_terminal` only through
// `flow_instance.rs`, which imports `PendingTerminal` directly from
// `call_stack`) — gate the re-export the same way so a plain `cargo check`
// of the lib target (no `cfg(test)`) doesn't see it as unused.
#[cfg(test)]
pub(crate) use call_stack::PendingTerminal;
pub use external::{ExternalFnHandler, ExternalResult, FallbackHandler, FunctionEval};
pub use flow_instance::{DriveOutcome, FlowInstance};
pub use types::{BlockId, Choice, Element, OutputLine, Stats, Step, StepOutcome, StoryStatus};

// ── Story ───────────────────────────────────────────────────────────────────

/// Per-instance mutable state for executing stories.
///
/// Created from a [`Program`] via [`Story::new`]. Holds all mutable state
/// (stacks, globals, output buffer) while the immutable program data lives
/// in [`Program`].
///
/// Generic over `R: StoryRng` — defaults to [`FastRng`]. Use
/// [`DotNetRng`](crate::DotNetRng) for .NET-compatible deterministic output.
pub struct Story<R: StoryRng = FastRng> {
    program: Arc<Program>,
    pub(crate) default: FlowInstance,
    pub(crate) default_context: World,
    /// The default flow's per-flow override layer. Empty in F1.3 (F3 fills
    /// it in) — the routing view built from `(default_context, default_local)`
    /// is an all-`World` passthrough, so this contributes nothing yet.
    default_local: FlowLocal,
    line_tables: Vec<Vec<brink_format::LineEntry>>,
    instances: HashMap<String, (FlowInstance, World, FlowLocal)>,
    /// Named flows that **share** `default_context` (globals / visit counts /
    /// rng) — true ink concurrent-flow semantics, where one flow's writes are
    /// visible to the others. Each still has its own call stack + temps (those
    /// live in the [`FlowInstance`]). Distinct from `instances`, whose flows
    /// each own an isolated `World` (bevy-brink's per-entity model). Transient
    /// studio/host state — not persisted in a [`StorySnapshot`].
    shared_instances: HashMap<String, FlowInstance>,
    resolver: Option<Box<dyn PluralResolver>>,
    /// Whether host **semantic** access to `#@private` definitions is refused
    /// (M-2b, `docs/modules-spec.md` §4 boundary rule 2). `true` by default —
    /// production hosts respect visibility. Dev tooling (play-from-here) sets
    /// it `false` via [`set_visibility_enforcement`](Self::set_visibility_enforcement)
    /// to start flows at private knots. No effect on stories without any
    /// `#@private` definition (the fast path short-circuits on that).
    enforce_visibility: bool,
    /// The dev/prod execution mode (NS-A4, [`ExecMode`]). A host/build
    /// knob mirrored onto every owned [`FlowInstance`] — see
    /// [`set_exec_mode`](Self::set_exec_mode). Not persisted in a
    /// [`StorySnapshot`] (the mode is a property of the host/build, not of
    /// story state).
    exec_mode: ExecMode,
    _rng: PhantomData<R>,
}

impl<R: StoryRng> Clone for Story<R> {
    fn clone(&self) -> Self {
        Self {
            program: Arc::clone(&self.program),
            default: self.default.clone(),
            default_context: self.default_context.clone(),
            default_local: self.default_local.clone(),
            line_tables: self.line_tables.clone(),
            instances: self.instances.clone(),
            shared_instances: self.shared_instances.clone(),
            resolver: None,
            enforce_visibility: self.enforce_visibility,
            exec_mode: self.exec_mode,
            _rng: PhantomData,
        }
    }
}

/// Owned story state that can be detached from a `Program` and reattached later.
///
/// Created by [`Story::into_snapshot`], consumed by [`Story::from_snapshot`].
/// This enables locale hot-swapping: detach state, mutate the program's line
/// tables, then reattach.
pub struct StorySnapshot<R: StoryRng = FastRng> {
    default: FlowInstance,
    default_context: World,
    default_local: FlowLocal,
    instances: HashMap<String, (FlowInstance, World, FlowLocal)>,
    _rng: PhantomData<R>,
}

/// Which unit a `debug_step*` call advances by (#3264). Both are
/// first-class verbs, not a primitive and a wrapper — the studio shows the
/// disassembly beside the source and drives each directly.
#[cfg(feature = "debug-hooks")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepGranularity {
    Instruction,
    Line,
}

/// The mutable per-flow state a debug verb drives (#3223): the flow plus
/// the context pair its `ContextView` routes through — the default trio,
/// an isolated flow's own trio, or a shared flow paired with the default
/// context, exactly the routing every production `continue_flow*` method
/// performs. Selected by [`Story::debug_parts`] so all six debug verbs
/// share one selection rule.
#[cfg(feature = "debug-hooks")]
struct DebugTarget<'a> {
    flow: &'a mut FlowInstance,
    world: &'a mut World,
    local: &'a mut FlowLocal,
}

/// The flow-independent story state every debug verb reads (#3223) —
/// bundled so the verb impls stay under one signature as parameters grow.
#[cfg(feature = "debug-hooks")]
struct DebugEnv<'a> {
    program: &'a Program,
    line_tables: &'a [Vec<brink_format::LineEntry>],
    resolver: Option<&'a dyn PluralResolver>,
    /// External-function bindings (#3224). The debug loops resolve a
    /// `Stepped::ExternalCall` through [`flow_instance::resolve_external_call`]
    /// with this handler — the same function, same handler contract as
    /// production `advance()` — so a bound external steps through
    /// mid-session instead of erroring `UnresolvedExternalCall`.
    handler: &'a dyn ExternalFnHandler,
}

/// See [`Story::debug_handle_stepped`].
#[cfg(feature = "debug-hooks")]
enum SteppedDisposition {
    Continue,
    /// An invisible default was auto-selected and the flow resumed — the
    /// step loop skips its depth/line stop-check for this iteration
    /// (matching the production per-turn loop's treatment: the boundary
    /// is bookkeeping, not a place execution "is").
    Resumed,
    /// The step crossed (and synchronously resolved) an `External` call.
    CrossedExternal,
    Stop(crate::DebugRunOutcome),
}

/// One line handed out by [`Story::debug_drain_buffered_lines`]: text,
/// tags, and its source location (W7/#3300 provenance — same field the
/// production road's `OutputLine::source` carries).
#[cfg(feature = "debug-hooks")]
pub type DrainedLine = (
    String,
    alloc::vec::Vec<String>,
    Option<brink_format::SourceLocation>,
);

/// Whether a debug run stops when the output buffer commits a line —
/// [`Story::debug_run_to_line`]'s tier vs [`Story::debug_run`]'s free
/// run. An enum rather than a bool so call sites read as what they do.
#[cfg(feature = "debug-hooks")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum StopOnLine {
    No,
    Yes,
}

impl<R: StoryRng> Story<R> {
    /// Create a new story instance from a linked program and its line tables.
    pub fn new(program: Arc<Program>, line_tables: Vec<Vec<brink_format::LineEntry>>) -> Self {
        let (default, default_context) = FlowInstance::new_at_root(&program);
        Self {
            program,
            default,
            default_context,
            default_local: FlowLocal::new(),
            line_tables,
            instances: HashMap::new(),
            shared_instances: HashMap::new(),
            resolver: None,
            enforce_visibility: true,
            exec_mode: ExecMode::default(),
            _rng: PhantomData,
        }
    }

    /// Enable or disable host visibility enforcement (M-2b,
    /// `docs/modules-spec.md` §4 boundary rule 3). Enforcement is **on** by
    /// default: host semantic access (variable get/set, entry lookup,
    /// function eval) to a `#@private` definition returns
    /// [`RuntimeError::PrivateAccess`] (or `None`/`false` for the infallible
    /// get/set). Dev tooling — editors, debug hosts, the play-from-here
    /// affordance — calls this with `false` to start flows at private knots
    /// and inspect private state. This is a host capability, not a language
    /// switch; the compiled program is identical either way. Persistence
    /// (save/load/journal/replay) ignores this flag entirely.
    ///
    /// Propagates to every [`FlowInstance`] this `Story` currently owns
    /// (`default`, every named flow, every shared flow) — each carries its
    /// own copy of the flag (so `bevy-brink`/[`crate::Speculation`] can
    /// enforce it when driving a `FlowInstance` directly, without a
    /// `Story`), and `Story` keeps them synced so a `Story`-mediated dev
    /// override never diverges from the flows it delegates to. Flows
    /// spawned after this call ([`spawn_flow`](Self::spawn_flow)/
    /// [`spawn_flow_shared`](Self::spawn_flow_shared)) inherit the
    /// `Story`'s current setting at spawn time.
    pub fn set_visibility_enforcement(&mut self, enforce: bool) {
        self.enforce_visibility = enforce;
        self.default.set_visibility_enforcement(enforce);
        for (flow, _, _) in self.instances.values_mut() {
            flow.set_visibility_enforcement(enforce);
        }
        for flow in self.shared_instances.values_mut() {
            flow.set_visibility_enforcement(enforce);
        }
    }

    /// Whether host visibility enforcement is currently on (default `true`).
    #[must_use]
    pub fn visibility_enforced(&self) -> bool {
        self.enforce_visibility
    }

    /// Set the dev/prod execution mode (NS-A4, [`ExecMode`] — see its docs
    /// for the §4b ordering doctrine). **Dev** (the default) faults on a
    /// float NaN comparand in an ordering context; **Prod** keeps moving
    /// with the pinned non-fabricating total order. The knob's home is
    /// project config (`brink.toml` profile) with this host-API override
    /// (ruled 2026-07-19); the mode is never embedded in `.inkb` and never
    /// persisted in saves or snapshots.
    ///
    /// Propagates to every [`FlowInstance`] this `Story` currently owns
    /// (`default`, named, shared) — the same sync discipline as
    /// [`set_visibility_enforcement`](Self::set_visibility_enforcement).
    /// Flows spawned after this call inherit the `Story`'s current setting
    /// at spawn time.
    pub fn set_exec_mode(&mut self, mode: ExecMode) {
        self.exec_mode = mode;
        self.default.set_exec_mode(mode);
        for (flow, _, _) in self.instances.values_mut() {
            flow.set_exec_mode(mode);
        }
        for flow in self.shared_instances.values_mut() {
            flow.set_exec_mode(mode);
        }
    }

    /// The current dev/prod execution mode (default [`ExecMode::Dev`]).
    #[must_use]
    pub fn exec_mode(&self) -> ExecMode {
        self.exec_mode
    }

    /// Set the plural resolver for Select resolution in localized lines.
    pub fn set_plural_resolver(&mut self, resolver: Box<dyn PluralResolver>) {
        self.resolver = Some(resolver);
    }

    /// Replace the active line tables (e.g. for locale swapping).
    pub fn set_line_tables(&mut self, tables: Vec<Vec<brink_format::LineEntry>>) {
        self.line_tables = tables;
    }

    /// Read-only access to the current line tables.
    pub fn line_tables(&self) -> &[Vec<brink_format::LineEntry>] {
        &self.line_tables
    }

    /// The full append-only transcript of all output parts produced so far.
    pub fn transcript(&self) -> &[crate::output::OutputPart] {
        self.default.flow.output.transcript()
    }

    /// Number of parts in the transcript.
    pub fn transcript_len(&self) -> usize {
        self.default.flow.output.transcript_len()
    }

    /// Reset the transcript read cursor to the beginning (for re-rendering).
    pub fn reset_cursor(&mut self) {
        self.default.flow.output.reset_cursor();
    }

    /// Resolve a slice of the transcript against the current line tables.
    /// Returns `(text, tags)` tuples — one per line in the resolved output.
    pub fn resolve_transcript_slice(&self, range: Range<usize>) -> Vec<(String, Vec<String>)> {
        let transcript = self.default.flow.output.transcript();
        let end = range.end.min(transcript.len());
        let start = range.start.min(end);
        let slice = &transcript[start..end];
        let fragments = self.default.flow.output.fragments();
        // Element-attachment data (issue #2108) is dropped here — this
        // method's public contract is `(text, tags)`, unchanged; a caller
        // that needs per-line element data has no use for a locale-
        // re-rendering slice taken in isolation from the surrounding
        // `Step::Line` stream anyway.
        crate::output::resolve_lines(
            slice,
            &self.program,
            &self.line_tables,
            self.resolver.as_deref(),
            fragments,
        )
        .into_iter()
        .map(|(text, tags, _element, _source)| (text, tags))
        .collect()
    }

    /// Re-resolve all pending choices against the current line tables.
    /// Returns the same choices that would appear in `Step::Choices`,
    /// but freshly resolved (useful after locale switch).
    pub fn pending_choices(&self) -> Vec<Choice> {
        self.resolved_choices_for(&self.default.flow)
    }

    /// Resolve a given flow's pending choices against the current line tables.
    /// Shared by [`pending_choices`](Self::pending_choices) (default flow) and
    /// the per-flow debug snapshot (#200 shared flows).
    fn resolved_choices_for(&self, flow: &Flow) -> Vec<Choice> {
        flow.pending_choices
            .iter()
            .enumerate()
            .filter(|(_, pc)| !pc.flags.is_invisible_default)
            .map(|(i, pc)| {
                let display_text = match &pc.display {
                    ChoiceDisplay::Text(s) => s.clone(),
                    ChoiceDisplay::Fragment(idx) => flow.output.resolve_fragment(
                        *idx,
                        &self.program,
                        &self.line_tables,
                        self.resolver.as_deref(),
                    ),
                };
                let display_text = display_text
                    .trim_matches(|c: char| c == ' ' || c == '\t')
                    .to_string();
                Choice {
                    text: display_text,
                    index: i,
                    tags: pc.tags.clone(),
                }
            })
            .collect()
    }

    /// Resolve a fragment against the current line tables.
    pub fn resolve_fragment(&self, idx: u32) -> String {
        self.default.flow.output.resolve_fragment(
            idx,
            &self.program,
            &self.line_tables,
            self.resolver.as_deref(),
        )
    }

    /// Get the fragment index for a pending choice's display text, if any.
    pub fn choice_fragment_idx(&self, choice_index: usize) -> Option<u32> {
        self.default
            .flow
            .pending_choices
            .get(choice_index)
            .and_then(|pc| match &pc.display {
                ChoiceDisplay::Fragment(idx) => Some(*idx),
                ChoiceDisplay::Text(_) => None,
            })
    }

    /// Read-only access to the fragment store (for transcript serialization).
    pub fn fragments(&self) -> &[crate::output::Fragment] {
        self.default.flow.output.fragments()
    }

    /// Read-only access to the program.
    pub fn program(&self) -> &Program {
        &self.program
    }

    /// Cheap `Arc` clone of the program, for callers (e.g. [`crate::save`])
    /// that need a `&Program` alongside a disjoint mutable borrow of another
    /// field — `self.program()` ties its `&Program` to all of `&self`, which
    /// conflicts with a simultaneous `&mut self.default_context`.
    pub(crate) fn program_arc(&self) -> Arc<Program> {
        Arc::clone(&self.program)
    }

    // ── Variable access (host-facing) ───────────────────────────────

    /// Read a global variable's current value by name. `None` if no global
    /// with that name is declared. Reads the default flow's context.
    ///
    /// Returns `None` for a `#@private` variable while visibility enforcement
    /// is on (M-2b) — the host is outside every module, so a private name is
    /// not host-visible. Dev tooling opts out via
    /// [`set_visibility_enforcement`](Self::set_visibility_enforcement).
    pub fn variable(&self, name: &str) -> Option<&Value> {
        let idx = self.program.global_index(name)?;
        if self.enforce_visibility
            && self.program.has_private_defs()
            && self.program.global_is_private(idx)
        {
            return None;
        }
        Some(ContextAccess::global(&self.default_context, idx))
    }

    /// Set a global variable by name, returning `false` (no-op) if no global
    /// with that name is declared. Ink globals are dynamically typed, so the
    /// host is responsible for passing a sensibly-typed value.
    ///
    /// Returns `false` (no write) for a `#@private` variable while visibility
    /// enforcement is on (M-2b). Dev tooling opts out via
    /// [`set_visibility_enforcement`](Self::set_visibility_enforcement).
    pub fn set_variable(&mut self, name: &str, value: Value) -> bool {
        match self.program.global_index(name) {
            Some(idx) => {
                if self.enforce_visibility
                    && self.program.has_private_defs()
                    && self.program.global_is_private(idx)
                {
                    return false;
                }
                ContextAccess::set_global(&mut self.default_context, idx, value);
                true
            }
            None => false,
        }
    }

    /// Set the RNG seed for the default flow's context. Seeding makes
    /// `RANDOM`/shuffle output reproducible — set it before running (or after
    /// a reset) so two runs of the same story on different machines match.
    pub fn set_rng_seed(&mut self, seed: i32) {
        ContextAccess::set_rng_seed(&mut self.default_context, seed);
    }

    // ── Pausable stepping (async externals) ─────────────────────────

    /// Advance the default flow by one step with a custom handler, surfacing a
    /// deferred external as [`StepOutcome::AwaitingExternal`] rather than
    /// erroring (unlike [`continue_single_with`](Self::continue_single_with)).
    ///
    /// On `AwaitingExternal`, resolve the pending call
    /// ([`resolve_external`](Self::resolve_external), or
    /// [`invoke_fallback`](Self::invoke_fallback)) and call `advance_with` again
    /// to resume. Inspect the pending call via
    /// [`pending_external_name`](Self::pending_external_name) /
    /// [`pending_external_args`](Self::pending_external_args).
    pub fn advance_with(
        &mut self,
        handler: &dyn ExternalFnHandler,
    ) -> Result<StepOutcome, RuntimeError> {
        let resolver = self.resolver.as_deref();
        let mut view = ContextView::new(&mut self.default_context, &mut self.default_local);
        self.default.advance::<R>(
            &self.program,
            &self.line_tables,
            &mut view,
            handler,
            resolver,
        )
    }

    /// Name of the external the default flow is paused on, if any.
    #[must_use]
    pub fn pending_external_name(&self) -> Option<&str> {
        self.default.pending_external_name(&self.program)
    }

    /// Arguments of the external the default flow is paused on.
    #[must_use]
    pub fn pending_external_args(&self) -> &[Value] {
        self.default.pending_external_args()
    }

    /// Evaluate an ink function by name from engine code, returning its value.
    ///
    /// Runs out-of-band on the default flow: output is isolated (the visible
    /// story is untouched), and the call completes synchronously. Externals the
    /// function calls are resolved inline by `handler`; an external the handler
    /// defers ([`ExternalResult::Pending`]) can't be resolved in a synchronous
    /// call and yields [`RuntimeError::AsyncExternalInCall`] (the paused eval is
    /// cleaned up first).
    ///
    /// # Errors
    /// [`RuntimeError::FunctionNotFound`] for an unknown name;
    /// [`RuntimeError::AsyncExternalInCall`] if a called external defers; plus
    /// any runtime error raised during evaluation.
    pub fn call_function(
        &mut self,
        name: &str,
        args: &[Value],
        handler: &dyn ExternalFnHandler,
    ) -> Result<Value, RuntimeError> {
        // M-2b: refuse host-driven evaluation of a `#@private` function while
        // enforcement is on. Checked before resolution details so a private
        // name reports as private, not as "not found".
        if self.enforce_visibility
            && self.program.has_private_defs()
            && self.program.path_is_private(name)
        {
            return Err(RuntimeError::PrivateAccess {
                name: name.to_owned(),
            });
        }
        let container_idx = self
            .program
            .find_address(name)
            .ok_or_else(|| RuntimeError::FunctionNotFound(name.to_owned()))?
            .0;
        // Arity-check against the function's declared parameters (compiler-built
        // programs only; converter-built ones record 0 and so accept no args).
        let expected = self.program.container(container_idx).param_count;
        if args.len() != expected as usize {
            return Err(RuntimeError::ArgCountMismatch {
                target: name.to_owned(),
                expected,
                got: args.len(),
            });
        }
        let resolver = self.resolver.as_deref();
        let mut view = ContextView::new(&mut self.default_context, &mut self.default_local);
        let outcome = self.default.begin_function_eval::<R>(
            &self.program,
            &self.line_tables,
            &mut view,
            handler,
            container_idx,
            args,
            resolver,
        )?;
        match outcome {
            FunctionEval::Returned(value) => Ok(value),
            FunctionEval::AwaitingExternal => {
                let name = self
                    .default
                    .pending_external_name(&self.program)
                    .map_or_else(|| name.to_owned(), ToOwned::to_owned);
                self.default
                    .abort_eval(&self.program, &self.line_tables, resolver);
                Err(RuntimeError::AsyncExternalInCall(name))
            }
        }
    }

    /// Fork a [`Speculation`](crate::Speculation) — a sandboxed,
    /// side-effect-proof speculative run — from the default flow's
    /// current state.
    ///
    /// The speculation owns an independent snapshot: driving it (via its
    /// own `advance`/`choose`/`go_to_path`/`eval_function` verbs) never
    /// mutates this `Story`. Dropping it discards everything it did. See
    /// [`crate::Speculation`] for the full picture, and
    /// [`crate::Speculation::fork_from`] for forking a non-default flow
    /// (e.g. a named flow spawned via [`spawn_flow`](Self::spawn_flow)).
    #[must_use]
    pub fn speculate(&self) -> crate::Speculation<R> {
        crate::Speculation::fork_from(
            Arc::clone(&self.program),
            &self.default_context,
            &self.default_local,
            &self.default,
            &self.line_tables,
        )
    }

    /// Detach story state from the program, consuming the story.
    pub fn into_snapshot(self) -> (StorySnapshot<R>, Vec<Vec<brink_format::LineEntry>>) {
        let snapshot = StorySnapshot {
            default: self.default,
            default_context: self.default_context,
            default_local: self.default_local,
            instances: self.instances,
            _rng: PhantomData,
        };
        (snapshot, self.line_tables)
    }

    /// Reattach a snapshot to a program with line tables.
    pub fn from_snapshot(
        program: Arc<Program>,
        snapshot: StorySnapshot<R>,
        line_tables: Vec<Vec<brink_format::LineEntry>>,
    ) -> Self {
        let mut story = Self {
            program,
            default: snapshot.default,
            default_context: snapshot.default_context,
            default_local: snapshot.default_local,
            line_tables,
            instances: snapshot.instances,
            // Shared flows are transient (not persisted) — a reattached story
            // starts with none.
            shared_instances: HashMap::new(),
            resolver: None,
            // Enforcement is a host capability, not persisted state — a
            // reattached story defaults to enforcing; the host re-applies a
            // dev override if it wants one.
            enforce_visibility: true,
            // Same posture for the dev/prod mode (NS-A4): a host/build
            // knob, not persisted state — a reattached story defaults to
            // Dev; the host re-applies its own setting.
            exec_mode: ExecMode::default(),
            _rng: PhantomData,
        };
        // `snapshot.default`/`snapshot.instances` carry whatever
        // `FlowInstance`-level enforcement flag they had at detach time
        // (e.g. `false`, if `into_snapshot` ran while a play-from-here
        // session had enforcement off) — force every flow back to the
        // reattached story's own (enforcing) setting so the two can't
        // diverge.
        story.set_visibility_enforcement(true);
        // Same re-sync for the exec mode (the flows in the snapshot carry
        // whatever mode they had at detach time).
        story.set_exec_mode(ExecMode::default());
        story
    }

    // ── Execution API ──────────────────────────────────────────────

    /// Execute until one line of content (up to newline), or until a
    /// yield point (choices/end) if no newline occurs first.
    ///
    /// The returned [`Step`] variant tells you what to do next:
    /// - [`Step::Line`] — more output may follow, keep calling.
    /// - [`Step::Choices`] — call [`choose`](Self::choose) then resume.
    /// - [`Step::End`] — the story has permanently ended.
    pub fn continue_single(&mut self) -> Result<Step, RuntimeError> {
        let resolver = self.resolver.as_deref();
        let mut view = ContextView::new(&mut self.default_context, &mut self.default_local);
        self.default.step_single_line::<R>(
            &self.program,
            &self.line_tables,
            &mut view,
            &FallbackHandler,
            resolver,
        )
    }

    /// Like [`continue_single`](Self::continue_single) but with a
    /// [`WriteObserver`] that receives notifications for every state mutation.
    pub fn continue_single_observed(
        &mut self,
        observer: &mut dyn WriteObserver,
    ) -> Result<Step, RuntimeError> {
        use crate::state::ObservedContext;
        let mut view = ContextView::new(&mut self.default_context, &mut self.default_local);
        let mut obs_ctx = ObservedContext::new(&mut view, observer);
        let resolver = self.resolver.as_deref();
        self.default.step_single_line::<R>(
            &self.program,
            &self.line_tables,
            &mut obs_ctx,
            &FallbackHandler,
            resolver,
        )
    }

    /// Like [`continue_single`](Self::continue_single) but with a custom
    /// external function handler.
    pub fn continue_single_with(
        &mut self,
        handler: &dyn ExternalFnHandler,
    ) -> Result<Step, RuntimeError> {
        let resolver = self.resolver.as_deref();
        let mut view = ContextView::new(&mut self.default_context, &mut self.default_local);
        self.default.step_single_line::<R>(
            &self.program,
            &self.line_tables,
            &mut view,
            handler,
            resolver,
        )
    }

    /// Execute until the next yield point, collecting all lines.
    ///
    /// Returns a `Vec<Step>` where the last element is always
    /// [`Step::Choices`] or [`Step::End`], and all preceding elements
    /// are [`Step::Line`].
    pub fn continue_maximally(&mut self) -> Result<Vec<Step>, RuntimeError> {
        self.continue_maximally_impl(&FallbackHandler)
    }

    /// Like [`continue_maximally`](Self::continue_maximally) but with a
    /// custom external function handler.
    pub fn continue_maximally_with(
        &mut self,
        handler: &dyn ExternalFnHandler,
    ) -> Result<Vec<Step>, RuntimeError> {
        self.continue_maximally_impl(handler)
    }

    fn continue_maximally_impl(
        &mut self,
        handler: &dyn ExternalFnHandler,
    ) -> Result<Vec<Step>, RuntimeError> {
        let resolver = self.resolver.as_deref();
        let mut view = ContextView::new(&mut self.default_context, &mut self.default_local);
        self.default.drive_to_terminal::<R>(
            &self.program,
            &self.line_tables,
            &mut view,
            handler,
            resolver,
        )
    }

    /// Execute until the next yield point with a [`WriteObserver`] that
    /// receives notifications for every state mutation.
    pub fn continue_maximally_observed(
        &mut self,
        observer: &mut dyn WriteObserver,
    ) -> Result<Vec<Step>, RuntimeError> {
        use crate::state::ObservedContext;
        let mut view = ContextView::new(&mut self.default_context, &mut self.default_local);
        let mut obs_ctx = ObservedContext::new(&mut view, observer);
        let resolver = self.resolver.as_deref();
        self.default.drive_to_terminal::<R>(
            &self.program,
            &self.line_tables,
            &mut obs_ctx,
            &FallbackHandler,
            resolver,
        )
    }

    /// Select a choice by index, then resume with
    /// [`continue_single`](Self::continue_single) or
    /// [`continue_maximally`](Self::continue_maximally).
    pub fn choose(&mut self, index: usize) -> Result<(), RuntimeError> {
        let mut view = ContextView::new(&mut self.default_context, &mut self.default_local);
        self.default.choose(&mut view, index)
    }

    /// Move the default flow's play head to a named knot/stitch path — ink's
    /// `ChoosePathString` equivalent. The current flow is force-completed
    /// (callstack reset, pending choices cleared), the jump counts as a visit
    /// to the target exactly like a `-> path` divert, and subsequent
    /// [`continue_single`](Self::continue_single) /
    /// [`continue_maximally`](Self::continue_maximally) calls run from there.
    /// See [`FlowInstance::choose_path_string`] for full semantics.
    ///
    /// # Errors
    /// [`UnknownPath`](RuntimeError::UnknownPath) for an unknown path;
    /// [`JumpWhileAwaitingExternal`](RuntimeError::JumpWhileAwaitingExternal)
    /// if the flow is parked on an unresolved external call;
    /// [`AlreadyEvaluatingFunction`](RuntimeError::AlreadyEvaluatingFunction)
    /// if an engine→ink function evaluation is in progress.
    pub fn choose_path_string(&mut self, path: &str) -> Result<(), RuntimeError> {
        self.check_entry_visibility(path)?;
        let mut view = ContextView::new(&mut self.default_context, &mut self.default_local);
        self.default
            .choose_path_string(&self.program, &mut view, path)
    }

    /// M-2b: refuse a host-driven entry into a `#@private` knot/stitch while
    /// visibility enforcement is on. Shared by both `choose_path_string`
    /// entry points. Dev tooling (play-from-here) disables enforcement via
    /// [`set_visibility_enforcement`](Self::set_visibility_enforcement).
    fn check_entry_visibility(&self, path: &str) -> Result<(), RuntimeError> {
        if self.enforce_visibility
            && self.program.has_private_defs()
            && self.program.path_is_private(path)
        {
            return Err(RuntimeError::PrivateAccess {
                name: path.to_owned(),
            });
        }
        Ok(())
    }

    /// Move the default flow's play head to a parameterized knot/stitch,
    /// **binding its declared parameters** from `args` — ink's
    /// `ChoosePathString` with arguments. Otherwise identical to
    /// [`choose_path_string`](Self::choose_path_string). See
    /// [`FlowInstance::choose_path_string_with_args`] for full semantics.
    ///
    /// # Errors
    /// As [`choose_path_string`](Self::choose_path_string), plus
    /// [`ArgCountMismatch`](RuntimeError::ArgCountMismatch) when `args.len()`
    /// doesn't match the target's declared parameter count.
    pub fn choose_path_string_with_args(
        &mut self,
        path: &str,
        args: &[Value],
    ) -> Result<(), RuntimeError> {
        self.check_entry_visibility(path)?;
        let mut view = ContextView::new(&mut self.default_context, &mut self.default_local);
        self.default
            .choose_path_string_with_args(&self.program, &mut view, path, args)
    }

    /// Read-only access to the default flow's VM statistics.
    pub fn stats(&self) -> &Stats {
        &self.default.stats
    }

    /// Returns `true` if the default flow has a pending external call
    /// (an `External` frame on top of the call stack).
    pub fn has_pending_external(&self) -> bool {
        self.default.flow.external_fn_id().is_some()
    }

    /// Resolve a pending external call on the default flow by providing
    /// the return value. For fire-and-forget calls, pass `Value::Null`.
    ///
    /// After resolving, call [`continue_maximally`](Story::continue_maximally)
    /// to continue execution.
    pub fn resolve_external(&mut self, value: Value) {
        self.default.flow.resolve_external(value);
    }

    /// [`resolve_external`](Self::resolve_external) for a named flow —
    /// isolated or shared, the same unified namespace
    /// [`destroy_flow`](Self::destroy_flow) treats (#3224: the debug
    /// seam can park any flow on
    /// [`DebugStopReason::AwaitingExternal`](crate::DebugStopReason::AwaitingExternal),
    /// so any flow needs the out-of-band resolution counterpart).
    ///
    /// # Errors
    /// [`RuntimeError::UnknownFlow`] if `name` names no live flow.
    pub fn resolve_external_flow(&mut self, name: &str, value: Value) -> Result<(), RuntimeError> {
        if let Some((f, _, _)) = self.instances.get_mut(name) {
            f.flow.resolve_external(value);
            Ok(())
        } else if let Some(f) = self.shared_instances.get_mut(name) {
            f.flow.resolve_external(value);
            Ok(())
        } else {
            Err(RuntimeError::UnknownFlow(name.to_owned()))
        }
    }

    /// Resolve a pending external call on the default flow by invoking
    /// the ink-defined fallback body. The fallback is a function call
    /// whose output becomes the return value.
    ///
    /// After invoking, call [`continue_maximally`](Story::continue_maximally)
    /// to continue execution.
    pub fn invoke_fallback(&mut self) -> Result<(), RuntimeError> {
        let fn_id = self
            .default
            .flow
            .external_fn_id()
            .ok_or(RuntimeError::CallStackUnderflow)?;
        let entry = self.program.external_fn(fn_id);
        let fallback_id = entry
            .and_then(|e| e.fallback)
            .ok_or(RuntimeError::UnresolvedExternalCall(fn_id))?;
        let container_idx = self
            .program
            .resolve_target(fallback_id)
            .map(|(idx, _)| idx)
            .ok_or(RuntimeError::UnresolvedDefinition(fallback_id))?;
        self.default.flow.output.begin_capture();
        self.default.flow.invoke_fallback(container_idx);
        Ok(())
    }

    // ── Named flow API ──────────────────────────────────────────────

    /// Spawn a new flow instance starting at the given entry point.
    ///
    /// `entry_point` is the `DefinitionId` of the target container
    /// (e.g., a knot). Each flow instance gets its own globals, visit
    /// counts, and execution state.
    pub fn spawn_flow(
        &mut self,
        name: &str,
        entry_point: DefinitionId,
    ) -> Result<(), RuntimeError> {
        // M-2b: refuse host-driven entry into a `#@private` knot/stitch while
        // visibility enforcement is on (`docs/modules-spec.md` §4 boundary
        // rule 2). Mirrors `check_entry_visibility`'s refusal on the named
        // `choose_path_string` path — a host holding a `DefinitionId` (this
        // by-id entry point) must not be able to bypass it. Checked before
        // any other error path so a private target reports as private, not
        // as "already exists" or "unresolved" (#803).
        if self.enforce_visibility
            && self.program.has_private_defs()
            && self.program.is_private(entry_point)
        {
            return Err(RuntimeError::PrivateAccess {
                name: format!("{entry_point}"),
            });
        }
        if self.instances.contains_key(name) {
            return Err(RuntimeError::FlowAlreadyExists(name.to_owned()));
        }
        let container_idx = self
            .program
            .resolve_target(entry_point)
            .map(|(idx, _)| idx)
            .ok_or(RuntimeError::UnresolvedDefinition(entry_point))?;
        let (mut flow, ctx) = FlowInstance::new_at(&self.program, container_idx);
        // Inherit this `Story`'s current enforcement setting (a dev override
        // set before spawning must apply to newly spawned flows too, not
        // just the flows that existed at override time).
        flow.set_visibility_enforcement(self.enforce_visibility);
        flow.set_exec_mode(self.exec_mode);
        self.instances
            .insert(name.to_owned(), (flow, ctx, FlowLocal::new()));
        Ok(())
    }

    /// Run a named flow instance until the next yield point.
    pub fn continue_flow_maximally(&mut self, name: &str) -> Result<Vec<Step>, RuntimeError> {
        self.continue_flow_maximally_with(name, &FallbackHandler)
    }

    /// Run a named flow instance with an external function handler.
    pub fn continue_flow_maximally_with(
        &mut self,
        name: &str,
        handler: &dyn ExternalFnHandler,
    ) -> Result<Vec<Step>, RuntimeError> {
        let (instance, ctx, local) = self
            .instances
            .get_mut(name)
            .ok_or_else(|| RuntimeError::UnknownFlow(name.to_owned()))?;
        let mut view = ContextView::new(ctx, local);
        let resolver = self.resolver.as_deref();
        instance.drive_to_terminal::<R>(
            &self.program,
            &self.line_tables,
            &mut view,
            handler,
            resolver,
        )
    }

    /// Select a choice in a named flow.
    pub fn choose_flow(&mut self, name: &str, index: usize) -> Result<(), RuntimeError> {
        let (instance, ctx, local) = self
            .instances
            .get_mut(name)
            .ok_or_else(|| RuntimeError::UnknownFlow(name.to_owned()))?;
        let mut view = ContextView::new(ctx, local);
        instance.choose(&mut view, index)
    }

    /// Destroy a named flow instance — isolated or shared (#200).
    pub fn destroy_flow(&mut self, name: &str) -> Result<(), RuntimeError> {
        if self.shared_instances.remove(name).is_some() || self.instances.remove(name).is_some() {
            Ok(())
        } else {
            Err(RuntimeError::UnknownFlow(name.to_owned()))
        }
    }

    /// List active flow names (isolated + shared), sorted for determinism.
    pub fn flow_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .instances
            .keys()
            .chain(self.shared_instances.keys())
            .map(String::as_str)
            .collect();
        names.sort_unstable();
        names
    }

    /// Re-evaluate the wake conditions of parked flows and return the ids
    /// of the flows that woke, sorted for determinism
    /// (`docs/flow-suspension-spec.md` §10.2). Waking never auto-continues:
    /// the host drives a woken flow via [`Story::continue_flow_single`] when
    /// it wants output.
    ///
    /// **Returns an empty list until parks exist (FS-3r).** No flow can be
    /// parked in today's runtime — the E052 lowering fence keeps `await`
    /// from producing bytecode ([`Step::Suspended`] is unreachable), so
    /// there are no conditions to re-evaluate. The method ships now (FS-3w)
    /// so hosts wire the wake loop against a stable shape; FS-3r fills in
    /// real condition evaluation + dirty-tracking without changing this
    /// signature. Dirty-tracking is not built here — this is the free stub.
    #[must_use]
    pub fn wake_check(&mut self) -> Vec<String> {
        // FS-3r: iterate parked flows, re-evaluate each dirty condition in
        // the owning flow's context via the isolated function-eval
        // machinery, collect woken ids. No flow can be parked yet, so the
        // woken set is always empty.
        Vec::new()
    }

    // ── Shared flows (#200) ─────────────────────────────────────────
    // Spawn a flow that **shares** `default_context` (globals / visit counts /
    // rng) with the default flow — true ink concurrent-flow semantics — while
    // keeping its own call stack + temps. Distinct from `spawn_flow`, whose
    // flows each own an isolated context (bevy-brink's per-entity model).

    /// Spawn a shared-context flow at `container_idx` (or the root if `None`).
    pub fn spawn_flow_shared(
        &mut self,
        name: &str,
        container_idx: Option<u32>,
    ) -> Result<(), RuntimeError> {
        // M-2b: same by-id refusal as `spawn_flow` (#803) — a resolved
        // `container_idx` (e.g. from `Program::find_address`, as the wasm
        // `spawn_flow` binding in `brink-web` does) must not bypass the
        // named-lookup refusal either. `None` targets the root, which is
        // never private.
        if let Some(idx) = container_idx
            && self.enforce_visibility
            && self.program.has_private_defs()
            && self.program.container_is_private(idx)
        {
            return Err(RuntimeError::PrivateAccess {
                name: format!("{}", self.program.container(idx).id),
            });
        }
        if self.shared_instances.contains_key(name) || self.instances.contains_key(name) {
            return Err(RuntimeError::FlowAlreadyExists(name.to_owned()));
        }
        // The fresh context the constructor returns is discarded — a shared
        // flow runs against `default_context`.
        let (mut flow, _ctx) = match container_idx {
            Some(idx) => FlowInstance::new_at(&self.program, idx),
            None => FlowInstance::new_at_root(&self.program),
        };
        // Inherit this `Story`'s current enforcement setting — see
        // `spawn_flow`'s identical note.
        flow.set_visibility_enforcement(self.enforce_visibility);
        flow.set_exec_mode(self.exec_mode);
        self.shared_instances.insert(name.to_owned(), flow);
        Ok(())
    }

    /// Advance a shared flow one line (against the shared context).
    pub fn continue_flow_single(&mut self, name: &str) -> Result<Step, RuntimeError> {
        self.continue_flow_single_with(name, &FallbackHandler)
    }

    /// Advance a shared flow one line with an external-function handler.
    pub fn continue_flow_single_with(
        &mut self,
        name: &str,
        handler: &dyn ExternalFnHandler,
    ) -> Result<Step, RuntimeError> {
        let resolver = self.resolver.as_deref();
        let instance = self
            .shared_instances
            .get_mut(name)
            .ok_or_else(|| RuntimeError::UnknownFlow(name.to_owned()))?;
        let mut view = ContextView::new(&mut self.default_context, &mut self.default_local);
        instance.step_single_line::<R>(
            &self.program,
            &self.line_tables,
            &mut view,
            handler,
            resolver,
        )
    }

    /// Run a shared flow to its next terminal line (against the shared
    /// context) — the shared-flow analogue of [`Self::continue_flow_maximally`]
    /// (which drives an *isolated* flow instead). Bounded by
    /// [`FlowInstance::LINE_LIMIT`] via
    /// [`drive_to_terminal`](FlowInstance::drive_to_terminal): an
    /// infinite-emitting flow errors with [`RuntimeError::LineLimitExceeded`]
    /// rather than growing the returned `Vec` without bound (guard against
    /// unbounded growth).
    pub fn continue_flow_maximally_shared(
        &mut self,
        name: &str,
    ) -> Result<Vec<Step>, RuntimeError> {
        self.continue_flow_maximally_shared_with(name, &FallbackHandler)
    }

    /// Run a shared flow to its next terminal line with an external-function
    /// handler. See [`Self::continue_flow_maximally_shared`].
    pub fn continue_flow_maximally_shared_with(
        &mut self,
        name: &str,
        handler: &dyn ExternalFnHandler,
    ) -> Result<Vec<Step>, RuntimeError> {
        let resolver = self.resolver.as_deref();
        let instance = self
            .shared_instances
            .get_mut(name)
            .ok_or_else(|| RuntimeError::UnknownFlow(name.to_owned()))?;
        let mut view = ContextView::new(&mut self.default_context, &mut self.default_local);
        instance.drive_to_terminal::<R>(
            &self.program,
            &self.line_tables,
            &mut view,
            handler,
            resolver,
        )
    }

    /// Select a choice in a shared flow (against the shared context).
    pub fn choose_flow_shared(&mut self, name: &str, index: usize) -> Result<(), RuntimeError> {
        let instance = self
            .shared_instances
            .get_mut(name)
            .ok_or_else(|| RuntimeError::UnknownFlow(name.to_owned()))?;
        let mut view = ContextView::new(&mut self.default_context, &mut self.default_local);
        instance.choose(&mut view, index)
    }

    /// A structured, name-resolved snapshot of the current runtime state for
    /// the studio State View: status, current location, globals, call stack,
    /// visit counts, pending choices, and rng. Read-only; built on demand and
    /// not on any hot path. See [`DebugSnapshot`](crate::DebugSnapshot).
    #[must_use]
    pub fn debug_snapshot(&self) -> crate::DebugSnapshot {
        self.build_debug_snapshot(&self.default, &self.default_context)
    }

    /// A debug snapshot of a named shared flow (#200), built against the shared
    /// `default_context` — so its globals / visit counts match the default
    /// flow's, while its call stack + temps are the flow's own. Falls back to a
    /// named isolated flow's own context if `name` is one of those instead.
    pub fn debug_snapshot_flow(&self, name: &str) -> Result<crate::DebugSnapshot, RuntimeError> {
        if let Some(instance) = self.shared_instances.get(name) {
            Ok(self.build_debug_snapshot(instance, &self.default_context))
        } else if let Some((instance, ctx, _local)) = self.instances.get(name) {
            Ok(self.build_debug_snapshot(instance, ctx))
        } else {
            Err(RuntimeError::UnknownFlow(name.to_owned()))
        }
    }

    /// Build a debug snapshot from a specific flow instance + context. Backs
    /// both [`debug_snapshot`](Self::debug_snapshot) and the per-flow variant.
    #[expect(
        clippy::too_many_lines,
        reason = "single-purpose snapshot builder assembling one flat struct \
                  from several independent, already-small pieces (status, \
                  location/position, globals, call stack, visit counts, \
                  pending choices, rng) — splitting would scatter one \
                  coherent read into several private helpers with no other \
                  caller, per CLAUDE.md's `cargo fmt`/`clippy` convention \
                  for this shape"
    )]
    fn build_debug_snapshot(&self, instance: &FlowInstance, ctx: &World) -> crate::DebugSnapshot {
        use alloc::collections::BTreeMap;

        use crate::debug::{
            DebugChoice, DebugFrame, DebugGlobal, DebugLocal, DebugPosition, DebugRng,
            DebugSnapshot, DebugVisit, NameResolver,
        };

        let flow = &instance.flow;
        let resolver = NameResolver::new(&self.program);

        let status = match instance.status {
            StoryStatus::Active => "active",
            StoryStatus::WaitingForChoice => "waiting_for_choice",
            StoryStatus::Done => "done",
            StoryStatus::Ended => "ended",
        };

        let thread = flow.current_thread();

        // Nearest named container the cursor is currently in (innermost-first).
        let resolve_frame_location = |frame: &CallFrame| {
            frame
                .container_stack
                .iter()
                .rev()
                .find_map(|cp| resolver.container_path(cp.container_idx))
                .map(str::to_owned)
        };
        // Precise `(container_idx, offset)` for a frame: the top of its
        // container stack — the next instruction that frame will execute
        // (`vm::step` always advances/reads this exact slot; see
        // `vm.rs`'s `frame.container_stack.last()`). `None` for a frame
        // whose container stack is already empty.
        let frame_position = |frame: &CallFrame| {
            frame.container_stack.last().map(|cp| DebugPosition {
                container_idx: cp.container_idx,
                offset: cp.offset,
            })
        };

        let current_location = thread.call_stack.last().and_then(resolve_frame_location);
        let position = thread.call_stack.last().and_then(frame_position);

        // D7 (`docs/debugger-spec.md` §3, #3185): this frame's named
        // locals, resolved via `Program::scope_debug_locals` against the
        // frame's *current leaf* container — deliberately NOT by unioning
        // `frame.container_stack` (see that method's own doc for why a
        // per-container_stack union silently drops an enclosing
        // container's locals the moment the leaf moves into a sibling
        // child container, even though the call frame's `temps` haven't
        // changed at all). `BTreeMap` (not `HashMap`) keeps the merge
        // deterministic (`CLAUDE.md` "Determinism matters"), keyed by slot
        // so the only known collision case (a future codegen slot reuse —
        // see `DebugFrame::locals`'s own doc) resolves to *some* entry
        // rather than panicking or reordering nondeterministically. `None`
        // when this program carries no `DebugInfo` at all (release-
        // exported, or pre-D6) or the frame's container stack is empty
        // (nothing left to run in it — no leaf to resolve a scope from).
        let resolve_frame_locals = |frame: &CallFrame| -> Option<Vec<DebugLocal>> {
            self.program.debug_info.as_ref()?;
            let leaf = frame.container_stack.last()?;
            let mut by_slot: BTreeMap<u16, DebugLocal> = BTreeMap::new();
            for local in self.program.scope_debug_locals(leaf.container_idx) {
                if let Some(value) = frame.temps.get(local.slot as usize) {
                    by_slot.insert(
                        local.slot,
                        DebugLocal {
                            slot: local.slot,
                            name: local.name.clone(),
                            value: resolver.debug_value(value),
                        },
                    );
                }
            }
            Some(by_slot.into_values().collect())
        };

        // Globals, skipping unnamed slots.
        let globals = ctx
            .globals
            .iter()
            .enumerate()
            .filter_map(|(i, value)| {
                self.program.global_slot_name(i).map(|name| DebugGlobal {
                    name: name.to_owned(),
                    value: resolver.format_value(value),
                })
            })
            .collect();

        // Call stack, innermost (current) frame first.
        let depth = thread.call_stack.len();
        let mut call_stack = Vec::with_capacity(depth);
        for i in (0..depth).rev() {
            if let Some(frame) = thread.call_stack.get(i) {
                let kind = match frame.frame_type {
                    CallFrameType::Root => "root",
                    CallFrameType::Function => "function",
                    CallFrameType::Tunnel => "tunnel",
                    CallFrameType::Thread => "thread",
                    CallFrameType::External => "external",
                    CallFrameType::FunctionEvalFromGame => "eval",
                };
                call_stack.push(DebugFrame {
                    kind,
                    location: resolve_frame_location(frame),
                    position: frame_position(frame),
                    temps: frame.temps.len(),
                    locals: resolve_frame_locals(frame),
                });
            }
        }

        // Visit counts, resolved and sorted by path for determinism.
        let mut visit_counts: Vec<DebugVisit> = ctx
            .visit_counts
            .iter()
            .filter_map(|(id, &count)| {
                resolver.def_path(*id).map(|path| DebugVisit {
                    path: path.to_owned(),
                    count,
                })
            })
            .collect();
        visit_counts.sort_by(|a, b| a.path.cmp(&b.path));

        // Pending choices: visible texts (resolved) paired with target paths.
        let visible_targets: Vec<DefinitionId> = flow
            .pending_choices
            .iter()
            .filter(|pc| !pc.flags.is_invisible_default)
            .map(|pc| pc.target_id)
            .collect();
        let pending_choices = self
            .resolved_choices_for(flow)
            .into_iter()
            .enumerate()
            .map(|(i, ch)| DebugChoice {
                text: ch.text,
                target: visible_targets
                    .get(i)
                    .and_then(|id| resolver.def_path(*id))
                    .map(str::to_owned),
                // `ch.index` is the pre-filter `flow.pending_choices` position
                // (see `resolved_choices_for`) — the same index `choose()`
                // expects, not the post-filter enumeration position `i`.
                index: ch.index,
            })
            .collect();

        DebugSnapshot {
            status,
            current_location,
            position,
            turn_index: ctx.turn_index,
            globals,
            call_stack,
            visit_counts,
            pending_choices,
            rng: DebugRng {
                seed: ctx.rng_seed,
                previous: ctx.previous_random,
            },
        }
    }

    // ── Session support (crate-internal) ────────────────────────────

    /// Whether the default flow is in the `Active` status (mid-turn, more
    /// content pending). Used by [`StorySession`](crate::StorySession) for the
    /// turn-boundary mutation gate.
    pub(crate) fn status_is_active(&self) -> bool {
        self.default.status == StoryStatus::Active
    }

    /// Whether the default flow is waiting for a choice selection. Used by
    /// [`StorySession`](crate::StorySession) replay.
    pub(crate) fn status_is_waiting_for_choice(&self) -> bool {
        self.default.status == StoryStatus::WaitingForChoice
    }

    /// Build a typed [`StateSnapshot`](crate::StateSnapshot) of the default
    /// flow's game state — a NEW typed serialization path (globals with list
    /// membership, turn counts, callstack summary), distinct from the
    /// string-valued [`DebugSnapshot`](crate::DebugSnapshot).
    ///
    /// Known projection limit (deliberate, not a silent bug): visit/turn-count
    /// entries whose scope has no resolvable author path (anonymous counted
    /// containers — gathers, choice points — keyed only by hash id) are
    /// **omitted** from the snapshot's path-keyed maps. The full id-keyed
    /// counts remain available via [`Story::save_state`].
    pub(crate) fn state_snapshot(&self) -> crate::session::StateSnapshot {
        use alloc::collections::BTreeMap;

        use crate::debug::NameResolver;
        use crate::session::{SnapshotFrame, SnapshotList, StateSnapshot};

        let flow = &self.default.flow;
        let ctx = &self.default_context;
        let resolver = NameResolver::new(&self.program);

        // Typed globals + resolved list membership.
        let mut globals: BTreeMap<String, Value> = BTreeMap::new();
        let mut lists: BTreeMap<String, SnapshotList> = BTreeMap::new();
        for (i, value) in ctx.globals.iter().enumerate() {
            if let Some(name) = self.program.global_slot_name(i) {
                if let Value::List(list) = value {
                    let mut items: Vec<String> = list
                        .items
                        .iter()
                        .filter_map(|id| self.program.list_item_name(*id).map(str::to_owned))
                        .collect();
                    items.sort_unstable();
                    lists.insert(name.to_owned(), SnapshotList { items });
                }
                globals.insert(name.to_owned(), value.clone());
            }
        }

        // Visit / turn counts by resolved path (deterministic BTreeMap).
        let mut visit_counts: BTreeMap<String, u32> = BTreeMap::new();
        for (id, &count) in &ctx.visit_counts {
            if let Some(path) = resolver.def_path(*id) {
                visit_counts.insert(path.to_owned(), count);
            }
        }
        let mut turn_counts: BTreeMap<String, u32> = BTreeMap::new();
        for (id, &count) in &ctx.turn_counts {
            if let Some(path) = resolver.def_path(*id) {
                turn_counts.insert(path.to_owned(), count);
            }
        }

        // Callstack summary, innermost frame first.
        let resolve_frame_location = |frame: &CallFrame| {
            frame
                .container_stack
                .iter()
                .rev()
                .find_map(|cp| resolver.container_path(cp.container_idx))
                .map(str::to_owned)
        };
        let thread = flow.current_thread();
        let depth = thread.call_stack.len();
        let mut call_stack = Vec::with_capacity(depth);
        for i in (0..depth).rev() {
            if let Some(frame) = thread.call_stack.get(i) {
                let kind = match frame.frame_type {
                    CallFrameType::Root => "root",
                    CallFrameType::Function => "function",
                    CallFrameType::Tunnel => "tunnel",
                    CallFrameType::Thread => "thread",
                    CallFrameType::External => "external",
                    CallFrameType::FunctionEvalFromGame => "eval",
                };
                call_stack.push(SnapshotFrame {
                    kind: kind.to_owned(),
                    location: resolve_frame_location(frame),
                    temps: frame.temps.len(),
                });
            }
        }

        StateSnapshot {
            globals,
            lists,
            turn_index: ctx.turn_index,
            visit_counts,
            turn_counts,
            call_stack,
            status: self.default.status.into(),
        }
    }

    // ── Testing / instrumentation API ───────────────────────────────

    /// Dump the current execution state for debugging.
    ///
    /// Returns a human-readable summary of the call stack, current position,
    /// value stack, output buffer, globals, and pending choices.
    #[cfg(feature = "testing")]
    pub fn debug_state(&self) -> String {
        use core::fmt::Write;
        let mut out = String::new();
        let flow = &self.default.flow;
        let ctx = &self.default_context;

        let _ = writeln!(out, "=== Story Debug State ===");
        let _ = writeln!(out, "status: {:?}", self.default.status);

        // Current position
        let thread = flow.current_thread();
        if let Some(frame) = thread.call_stack.last()
            && let Some(cp) = frame.container_stack.last()
        {
            let id = self.program.container(cp.container_idx).id;
            let _ = writeln!(
                out,
                "position: container_idx={} id={id:?} offset={}",
                cp.container_idx, cp.offset,
            );
        }

        // Call stack
        let depth = thread.call_stack.len();
        let _ = writeln!(out, "\ncall stack ({depth} frames):");
        for i in 0..depth {
            if let Some(frame) = thread.call_stack.get(i) {
                let ret = frame
                    .return_address
                    .map(|r| format!("idx={} off={}", r.container_idx, r.offset));
                let _ = writeln!(
                    out,
                    "  [{i}] {:?} ret={} temps={} containers={}",
                    frame.frame_type,
                    ret.as_deref().unwrap_or("none"),
                    frame.temps.len(),
                    frame.container_stack.len(),
                );
                for (j, cp) in frame.container_stack.iter().enumerate() {
                    let id = self.program.container(cp.container_idx).id;
                    let _ = writeln!(
                        out,
                        "       container_stack[{j}]: idx={} id={id:?} off={}",
                        cp.container_idx, cp.offset,
                    );
                }
            }
        }

        // Value stack
        let _ = writeln!(out, "\nvalue stack ({}):", flow.value_stack.len());
        for (i, v) in flow.value_stack.iter().enumerate() {
            let _ = writeln!(out, "  [{i}] {v:?}");
        }

        // Output buffer (unread transcript)
        let unread_start = flow.output.cursor;
        let transcript = &flow.output.transcript[unread_start..];
        let _ = writeln!(
            out,
            "\noutput buffer (cursor={unread_start}, {} unread parts):",
            transcript.len(),
        );
        for (i, part) in transcript.iter().enumerate() {
            let _ = writeln!(out, "  [{i}] {part:?}");
        }

        // Globals
        let _ = writeln!(out, "\nglobals:");
        for (i, v) in ctx.globals.iter().enumerate() {
            #[expect(clippy::cast_possible_truncation, reason = "global count fits in u32")]
            if let Some(name) = self.program.global_name(i as u32) {
                let _ = writeln!(out, "  {name} = {v:?}");
            }
        }

        // Flow flags
        let _ = writeln!(out, "\nskipping_choice: {}", flow.skipping_choice);

        // Pending choices
        let _ = writeln!(out, "\npending choices ({}):", flow.pending_choices.len());
        for (i, c) in flow.pending_choices.iter().enumerate() {
            let _ = writeln!(out, "  [{i}] {:?} -> {:?}", c.display, c.target_id);
        }

        out
    }

    /// Returns whether the last execution cycle of the **default** flow
    /// ended with a safe exit (explicit `-> DONE` opcode). If false after a
    /// `Done` line, the story ran out of content — the next
    /// `continue_single` call will return [`RuntimeError::RanOutOfContent`]
    /// instead of more text. See [`FlowInstance::did_safe_exit`] for the
    /// full contract.
    ///
    /// This reads only `self.default` — for a named flow (spawned via
    /// [`spawn_flow`](Self::spawn_flow) or one of the isolated
    /// `instances`), use [`did_safe_exit_flow`](Self::did_safe_exit_flow)
    /// instead. Calling this after `continue_flow*` on a named flow
    /// silently returns the default flow's stale value.
    #[must_use]
    pub fn did_safe_exit(&self) -> bool {
        self.default.did_safe_exit()
    }

    /// Like [`did_safe_exit`](Self::did_safe_exit), but for a named flow
    /// (shared or isolated) rather than the default flow. Mirrors
    /// [`debug_snapshot_flow`](Self::debug_snapshot_flow)'s lookup shape:
    /// checks `shared_instances` first, then falls back to the isolated
    /// `instances`.
    ///
    /// # Errors
    /// [`UnknownFlow`](RuntimeError::UnknownFlow) if no flow named `name`
    /// exists (shared or isolated).
    pub fn did_safe_exit_flow(&self, name: &str) -> Result<bool, RuntimeError> {
        if let Some(instance) = self.shared_instances.get(name) {
            Ok(instance.did_safe_exit())
        } else if let Some((instance, _ctx, _local)) = self.instances.get(name) {
            Ok(instance.did_safe_exit())
        } else {
            Err(RuntimeError::UnknownFlow(name.to_owned()))
        }
    }

    /// Returns whether the last execution cycle passed through an empty
    /// choice set (a `Yield` opcode with no pending choices).
    #[cfg(feature = "testing")]
    pub fn did_unsafe_yield(&self) -> bool {
        self.default.flow.did_unsafe_yield
    }

    /// Execute a single VM step and return a debug trace of what happened.
    ///
    /// Returns `(opcode_description, container_idx, offset_before)` or None
    /// if the step didn't decode an opcode (frame exhaustion, thread completion, etc).
    #[cfg(feature = "testing")]
    pub fn step_once(&mut self) -> Result<Option<(String, u32, usize)>, RuntimeError> {
        use brink_format::Opcode;

        let flow = &self.default.flow;
        let thread = flow.current_thread();

        // Capture position before step
        let pre_info = thread.call_stack.last().and_then(|frame| {
            frame.container_stack.last().map(|pos| {
                let container = self.program.container(pos.container_idx);
                if pos.offset < container.bytecode.len() {
                    let mut off = pos.offset;
                    let op = Opcode::decode(&container.bytecode, &mut off).ok();
                    (pos.container_idx, pos.offset, op)
                } else {
                    (pos.container_idx, pos.offset, None)
                }
            })
        });

        // Execute one step
        let _result = vm::step::<R>(
            &mut self.default.flow,
            &self.program,
            &self.line_tables,
            &mut self.default_context,
            &mut self.default.stats,
            self.resolver.as_deref(),
        )?;

        match pre_info {
            Some((ci, off, Some(op))) => Ok(Some((format!("{op:?}"), ci, off))),
            Some((ci, off, None)) => Ok(Some(("(end of container)".to_string(), ci, off))),
            None => Ok(None),
        }
    }

    // ── D8 debugger control seam (issue #3186) ──────────────────────────
    //
    // Feature-gated per `debug_control`'s own module doc — with
    // `debug-hooks` off, none of this exists and nothing below is
    // compiled in. Every method here bypasses the buffered line-output
    // path (`continue_single` and friends) entirely, stepping `vm::step`
    // directly — the same primitive the `testing`-gated `step_once` probe
    // above already uses — so a caller sees every opcode boundary, not
    // just line boundaries. None of it changes `advance_with_limit` or
    // `vm::step_impl`; see `debug_control`'s module doc for the zero-cost
    // argument this depends on.

    /// Drain every COMPLETED-but-undelivered line from the default flow's
    /// output buffer — the exact cursor `continue_single` delivers from
    /// (`advance_with_limit` step 1's `take_first_line`). The wasm debug
    /// verbs call this before AND after stepping so the two drive roads
    /// share ONE delivery stream (W5/#3298): the production line-buffered
    /// path runs ahead of what it has handed out, so a line it already
    /// completed must surface in the debug outcome exactly once — and,
    /// because this consumes the same cursor, never again on a later
    /// journaled continue. Suppressed segments are skipped exactly as the
    /// production take does; partial (uncompleted) content stays buffered
    /// untouched.
    #[cfg(feature = "debug-hooks")]
    pub fn debug_drain_buffered_lines(&mut self) -> alloc::vec::Vec<DrainedLine> {
        let resolver = self.resolver.as_deref();
        let mut out = alloc::vec::Vec::new();
        while self.default.flow.output.has_completed_line() {
            let Some((text, tags, _element, source)) = self.default.flow.output.take_first_line(
                &self.program,
                &self.line_tables,
                resolver,
            ) else {
                break;
            };
            out.push((text, tags, source));
        }
        // Mirror `advance_with_limit`'s step 2: at a yield point no more
        // output is coming, so trailing (uncommitted-newline) content is
        // flushed — this is how the line before a choice point is
        // delivered on the production road, and it must be here too.
        if self.default.status != StoryStatus::Active && self.default.flow.output.has_unread() {
            for (text, tags, _element, source) in
                self.default
                    .flow
                    .output
                    .flush_lines(&self.program, &self.line_tables, resolver)
            {
                out.push((text, tags, source));
            }
        }
        out
    }

    /// The default flow's current execution position, or `None` when the
    /// innermost frame has an empty container stack — mirrors
    /// [`debug_snapshot`](Self::debug_snapshot)'s `position` field without
    /// building the rest of the snapshot.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub fn debug_position(&self) -> Option<crate::DebugPosition> {
        Self::position_of(&self.default.flow)
    }

    /// [`debug_position`](Self::debug_position) for any flow (#3223):
    /// `None` targets the default flow, `Some(name)` a named flow —
    /// isolated or shared, the same unified namespace
    /// [`destroy_flow`](Self::destroy_flow)/[`flow_names`](Self::flow_names)
    /// treat. `Ok(None)` is a real "no position" on a real flow, distinct
    /// from the unknown-name error.
    ///
    /// # Errors
    /// [`RuntimeError::UnknownFlow`] if `flow` names no live flow.
    #[cfg(feature = "debug-hooks")]
    pub fn debug_position_flow(
        &self,
        flow: Option<&str>,
    ) -> Result<Option<crate::DebugPosition>, RuntimeError> {
        Ok(Self::position_of(&self.debug_flow_ref(flow)?.flow))
    }

    /// The default flow's current thread's call-stack depth — the raw
    /// count [`debug_step`](Self::debug_step)'s step-over/out logic is
    /// derived from (`docs/debugger-spec.md` §4).
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub fn debug_call_stack_depth(&self) -> usize {
        Self::depth_of(&self.default.flow)
    }

    /// [`debug_call_stack_depth`](Self::debug_call_stack_depth) for any
    /// flow (#3223) — flow selection as in
    /// [`debug_position_flow`](Self::debug_position_flow).
    ///
    /// # Errors
    /// [`RuntimeError::UnknownFlow`] if `flow` names no live flow.
    #[cfg(feature = "debug-hooks")]
    pub fn debug_call_stack_depth_flow(&self, flow: Option<&str>) -> Result<usize, RuntimeError> {
        Ok(Self::depth_of(&self.debug_flow_ref(flow)?.flow))
    }

    /// Read-only flow selection for the debug getters (#3223): `None` is
    /// the default flow; a name resolves through the isolated map first,
    /// then the shared map — the same order [`debug_parts`](Self::debug_parts)
    /// uses, and unambiguous because [`spawn_flow`](Self::spawn_flow)/
    /// [`spawn_flow_shared`](Self::spawn_flow_shared) refuse a name live in
    /// either map.
    #[cfg(feature = "debug-hooks")]
    fn debug_flow_ref(&self, flow: Option<&str>) -> Result<&FlowInstance, RuntimeError> {
        match flow {
            None => Ok(&self.default),
            Some(name) => self
                .instances
                .get(name)
                .map(|(f, _, _)| f)
                .or_else(|| self.shared_instances.get(name))
                .ok_or_else(|| RuntimeError::UnknownFlow(name.to_owned())),
        }
    }

    /// Split this story into the selected flow's mutable [`DebugTarget`]
    /// plus the shared read-only [`DebugEnv`] (#3223). A shared flow pairs
    /// with `default_context`/`default_local` — its writes are visible to
    /// the default flow, true concurrent-flow semantics — while an
    /// isolated flow drives its own context, exactly as the production
    /// `continue_flow*` methods route each.
    #[cfg(feature = "debug-hooks")]
    fn debug_parts<'a>(
        &'a mut self,
        flow: Option<&str>,
        handler: &'a dyn ExternalFnHandler,
    ) -> Result<(DebugTarget<'a>, DebugEnv<'a>), RuntimeError> {
        let Self {
            program,
            default,
            default_context,
            default_local,
            line_tables,
            instances,
            shared_instances,
            resolver,
            ..
        } = self;
        let target = match flow {
            None => DebugTarget {
                flow: default,
                world: default_context,
                local: default_local,
            },
            Some(name) => {
                if let Some((f, w, l)) = instances.get_mut(name) {
                    DebugTarget {
                        flow: f,
                        world: w,
                        local: l,
                    }
                } else if let Some(f) = shared_instances.get_mut(name) {
                    DebugTarget {
                        flow: f,
                        world: default_context,
                        local: default_local,
                    }
                } else {
                    return Err(RuntimeError::UnknownFlow(name.to_owned()));
                }
            }
        };
        Ok((
            target,
            DebugEnv {
                program,
                line_tables,
                resolver: resolver.as_deref(),
                handler,
            },
        ))
    }

    /// Run the default flow forward one VM instruction at a time until an
    /// enabled breakpoint in `breakpoints` is reached — checked *before*
    /// the matching instruction executes, so execution halts BEFORE it
    /// runs, not after — or the flow reaches a stopping VM outcome (a
    /// choice point or a terminal `-> DONE`/`-> END`).
    ///
    /// The breakpoint check is skipped on this call's very first
    /// iteration, before any `vm::step` has run — otherwise a resumed
    /// `debug_run` called right after a previous `debug_run`/`debug_step`
    /// stopped exactly on an armed breakpoint would immediately re-report
    /// that same breakpoint without making any forward progress, forever
    /// (issue #3186 review: "resume is impossible"). At least one
    /// instruction always executes before a breakpoint at the position
    /// already stopped at is honored again.
    ///
    /// A choice point (`-> DONE`/exhaustion with pending choices) reports
    /// [`DebugStopReason::Choices`](crate::DebugStopReason::Choices), not
    /// [`DebugStopReason::Terminal`](crate::DebugStopReason::Terminal) —
    /// distinguishing the two matters because
    /// [`Story::choose`](Self::choose) only accepts the former. The same
    /// turn-index bump and invisible-default auto-select the production
    /// per-turn loop performs on this outcome are applied here too, via
    /// [`flow_instance::apply_done_bookkeeping`], so `status` and
    /// `turn_index` never diverge from what a production-path caller would
    /// see (issue #3186 review: "turn boundaries are mislabeled").
    ///
    /// Bounded by `budget_ceiling` VM steps — **not** the production step
    /// limit, and this never reads or writes `Stats::steps` (the counter
    /// `advance_with_limit`'s own step-limit check reads); the debug
    /// budget is tracked in a loop-local variable instead. See
    /// `debug_control`'s module doc for the full accounting argument
    /// (2026-08-28 step-limit ruling on issue #3186). Pass
    /// [`crate::DEFAULT_DEBUG_BUDGET`] unless the caller has a reason to
    /// override it.
    ///
    /// # Errors
    /// [`RuntimeError::DebugBudgetExceeded`] if `budget_ceiling` VM steps
    /// pass without hitting a breakpoint or a stopping outcome — never
    /// [`RuntimeError::StepLimitExceeded`], which is the *production*
    /// step-limit error and would misreport which budget fired. Any other
    /// error `vm::step` itself can produce.
    ///
    /// An `EXTERNAL` call crossed mid-run resolves exactly as production
    /// `advance()` resolves it (#3224): this method binds the
    /// [`FallbackHandler`], so the in-story fallback body runs; a host
    /// with real bindings passes its handler to
    /// [`debug_run_flow`](Self::debug_run_flow), and a handler that
    /// defers ([`ExternalResult::Pending`](crate::ExternalResult::Pending))
    /// parks the run with
    /// [`DebugStopReason::AwaitingExternal`](crate::DebugStopReason::AwaitingExternal)
    /// — frame intact, resolve out-of-band, then resume.
    #[cfg(feature = "debug-hooks")]
    pub fn debug_run(
        &mut self,
        breakpoints: &crate::debug_control::BreakpointSet,
        budget_ceiling: u64,
    ) -> Result<crate::DebugRunOutcome, RuntimeError> {
        self.debug_run_flow(None, &FallbackHandler, breakpoints, budget_ceiling)
    }

    /// Like [`debug_run`](Self::debug_run), but ALSO stops — with
    /// [`DebugStopReason::Step`](crate::DebugStopReason::Step) — the
    /// moment the flow's output buffer holds a **completed line**: the
    /// granularity ladder's top tier (2026-08-30 Continue ruling,
    /// `docs/decision-log.md`), "advance until the next content line is
    /// delivered". The stop lands strictly *past* the line's commit
    /// boundary (a line only completes once the following non-whitespace
    /// output begins, because glue may still legally join onto it — the
    /// same `has_completed_line` rule the production delivery cursor
    /// obeys), so a caller that drains
    /// [`debug_drain_buffered_lines`](Self::debug_drain_buffered_lines)
    /// after this verb receives the crossed line IN this stop's outcome —
    /// no one-advance delivery lag (#3321's felt half). Breakpoints,
    /// choice points, terminals, and deferred externals stop it early,
    /// exactly as in [`debug_run`](Self::debug_run); a line completed by
    /// the *flush* at a yield point surfaces through those stops' drain
    /// instead (the production road's own delivery for a line before a
    /// choice). Requires no debug line info — the stop condition is
    /// output-buffer state, not a `DebugInfo` entry.
    ///
    /// # Errors
    /// As [`debug_run`](Self::debug_run).
    #[cfg(feature = "debug-hooks")]
    pub fn debug_run_to_line(
        &mut self,
        breakpoints: &crate::debug_control::BreakpointSet,
        budget_ceiling: u64,
    ) -> Result<crate::DebugRunOutcome, RuntimeError> {
        self.debug_run_to_line_flow(None, &FallbackHandler, breakpoints, budget_ceiling)
    }

    /// [`debug_run_to_line`](Self::debug_run_to_line) for any flow —
    /// flow selection as in [`debug_run_flow`](Self::debug_run_flow).
    ///
    /// # Errors
    /// [`RuntimeError::UnknownFlow`] if `flow` names no live flow; then
    /// everything [`debug_run`](Self::debug_run) can raise.
    #[cfg(feature = "debug-hooks")]
    pub fn debug_run_to_line_flow(
        &mut self,
        flow: Option<&str>,
        handler: &dyn ExternalFnHandler,
        breakpoints: &crate::debug_control::BreakpointSet,
        budget_ceiling: u64,
    ) -> Result<crate::DebugRunOutcome, RuntimeError> {
        let (target, env) = self.debug_parts(flow, handler)?;
        Self::debug_run_impl(&env, target, breakpoints, budget_ceiling, StopOnLine::Yes)
    }

    /// [`debug_run`](Self::debug_run) for any flow (#3223): `None`
    /// targets the default flow (identical to `debug_run`), `Some(name)`
    /// a named flow — isolated or shared. A shared flow's writes land in
    /// the default context, so a debug session on one is observable from
    /// the others, exactly as in production.
    ///
    /// # Errors
    /// [`RuntimeError::UnknownFlow`] if `flow` names no live flow; then
    /// everything [`debug_run`](Self::debug_run) can raise.
    #[cfg(feature = "debug-hooks")]
    pub fn debug_run_flow(
        &mut self,
        flow: Option<&str>,
        handler: &dyn ExternalFnHandler,
        breakpoints: &crate::debug_control::BreakpointSet,
        budget_ceiling: u64,
    ) -> Result<crate::DebugRunOutcome, RuntimeError> {
        let (target, env) = self.debug_parts(flow, handler)?;
        Self::debug_run_impl(&env, target, breakpoints, budget_ceiling, StopOnLine::No)
    }

    /// What one `vm::step` outcome means for a debug loop (#3224):
    /// keep looping, keep looping but the step crossed (and resolved) an
    /// `External` call — `debug_step_impl` flips `Into` to `Over`
    /// semantics on that signal, spec §4 — or stop with an outcome.
    #[cfg(feature = "debug-hooks")]
    fn debug_handle_stepped(
        stepped: vm::Stepped,
        env: &DebugEnv<'_>,
        flow: &mut FlowInstance,
        view: &mut ContextView<'_>,
    ) -> Result<SteppedDisposition, RuntimeError> {
        use crate::debug_control::DebugStopReason;
        let stop = |flow: &FlowInstance, reason| {
            SteppedDisposition::Stop(crate::DebugRunOutcome {
                reason,
                position: Self::position_of(&flow.flow),
                depth: Self::depth_of(&flow.flow),
            })
        };
        Ok(match stepped {
            vm::Stepped::Done => match flow_instance::apply_done_bookkeeping(
                &mut flow.flow,
                view,
                &mut flow.status,
                &mut flow.stats,
            )? {
                flow_instance::DoneBookkeeping::AutoSelected => SteppedDisposition::Resumed,
                flow_instance::DoneBookkeeping::WaitingForChoice => {
                    stop(flow, DebugStopReason::Choices)
                }
                flow_instance::DoneBookkeeping::Terminal => stop(flow, DebugStopReason::Terminal),
            },
            vm::Stepped::Ended => {
                view.increment_turn_index();
                flow.status = StoryStatus::Ended;
                stop(flow, DebugStopReason::Terminal)
            }
            vm::Stepped::ExternalCall => {
                // #3224: resolve through the SAME function production
                // `advance()` uses, so debug and production stepping can
                // never disagree about binding semantics. An unresolved
                // (deferred) external parks with the frame intact for
                // out-of-band resolution.
                if flow_instance::resolve_external_call(&mut flow.flow, env.program, env.handler)? {
                    SteppedDisposition::CrossedExternal
                } else {
                    stop(flow, DebugStopReason::AwaitingExternal)
                }
            }
            vm::Stepped::Continue | vm::Stepped::ThreadCompleted => SteppedDisposition::Continue,
        })
    }

    #[cfg(feature = "debug-hooks")]
    fn debug_run_impl(
        env: &DebugEnv<'_>,
        target: DebugTarget<'_>,
        breakpoints: &crate::debug_control::BreakpointSet,
        budget_ceiling: u64,
        stop_on_line: StopOnLine,
    ) -> Result<crate::DebugRunOutcome, RuntimeError> {
        use crate::debug_control::DebugStopReason;

        let DebugTarget { flow, world, local } = target;
        let mut view = ContextView::new(world, local);
        let mut steps: u64 = 0;
        let mut past_entry = false;
        loop {
            if past_entry
                && let Some(pos) = Self::position_of(&flow.flow)
                && let Some(bp) = breakpoints.hit(pos)
            {
                return Ok(crate::DebugRunOutcome {
                    reason: DebugStopReason::Breakpoint {
                        id: bp.id,
                        name: bp.name.clone(),
                    },
                    position: Some(pos),
                    depth: Self::depth_of(&flow.flow),
                });
            }
            past_entry = true;
            steps += 1;
            if steps > budget_ceiling {
                return Err(RuntimeError::DebugBudgetExceeded {
                    breakpoint: "run".to_owned(),
                    ceiling: budget_ceiling,
                });
            }

            let stepped = vm::step::<R>(
                &mut flow.flow,
                env.program,
                env.line_tables,
                &mut view,
                &mut flow.stats,
                env.resolver,
            )?;

            match Self::debug_handle_stepped(stepped, env, flow, &mut view)? {
                SteppedDisposition::Continue
                | SteppedDisposition::Resumed
                | SteppedDisposition::CrossedExternal => {}
                SteppedDisposition::Stop(outcome) => return Ok(outcome),
            }

            // The run-to-line tier (2026-08-30 Continue ruling): a line
            // COMMITTING is the stop condition — checked after the step so
            // the stop lands past the commit boundary and the line is
            // drainable at the stop, not one advance later (#3321).
            if stop_on_line == StopOnLine::Yes && flow.flow.output.has_completed_line() {
                return Ok(crate::DebugRunOutcome {
                    reason: DebugStopReason::Step,
                    position: Self::position_of(&flow.flow),
                    depth: Self::depth_of(&flow.flow),
                });
            }
        }
    }

    /// Like [`debug_run`](Self::debug_run), but writes are routed through
    /// `watchpoints` (a [`crate::WatchpointObserver`]) via the existing
    /// [`ObservedContext`](crate::ObservedContext) seam — reusing
    /// [`WriteObserver`] rather than a second observer mechanism, exactly
    /// as `continue_single_observed` already does for the buffered
    /// production path. Also stops, with
    /// [`DebugStopReason::Watchpoint`](crate::DebugStopReason::Watchpoint),
    /// the moment a watched global is written, in addition to every
    /// `debug_run` stop condition.
    ///
    /// Drain contract (#3226): one stop per hit, each attributed to its
    /// own writing instruction. A hit already queued when this is called
    /// — the observer doubles as a non-pausing logger on the production
    /// path, so leftovers are a real state — reports immediately at the
    /// current position, before any stepping. (One VM step can queue at
    /// most one hit today — the VM's two `set_global` sites are
    /// single-write opcodes — but the loop-top drain makes that an
    /// optimization detail, not a correctness dependency.)
    ///
    /// # Errors
    /// Same as [`debug_run`](Self::debug_run).
    #[cfg(feature = "debug-hooks")]
    pub fn debug_run_watching(
        &mut self,
        breakpoints: &crate::debug_control::BreakpointSet,
        watchpoints: &mut crate::WatchpointObserver,
        budget_ceiling: u64,
    ) -> Result<crate::DebugRunOutcome, RuntimeError> {
        self.debug_run_watching_flow(
            None,
            &FallbackHandler,
            breakpoints,
            watchpoints,
            budget_ceiling,
        )
    }

    /// [`debug_run_watching`](Self::debug_run_watching) for any flow
    /// (#3223) — flow selection as in
    /// [`debug_run_flow`](Self::debug_run_flow). Note the watch surface is
    /// the *context* the flow routes through: on a shared flow the watched
    /// globals live in the default context, so a hit can be caused by the
    /// debugged flow only (this seam steps no other flow concurrently).
    ///
    /// # Errors
    /// [`RuntimeError::UnknownFlow`] if `flow` names no live flow; then
    /// everything [`debug_run`](Self::debug_run) can raise.
    #[cfg(feature = "debug-hooks")]
    pub fn debug_run_watching_flow(
        &mut self,
        flow: Option<&str>,
        handler: &dyn ExternalFnHandler,
        breakpoints: &crate::debug_control::BreakpointSet,
        watchpoints: &mut crate::WatchpointObserver,
        budget_ceiling: u64,
    ) -> Result<crate::DebugRunOutcome, RuntimeError> {
        let (target, env) = self.debug_parts(flow, handler)?;
        Self::debug_run_watching_impl(&env, target, breakpoints, watchpoints, budget_ceiling)
    }

    #[cfg(feature = "debug-hooks")]
    fn debug_run_watching_impl(
        env: &DebugEnv<'_>,
        target: DebugTarget<'_>,
        breakpoints: &crate::debug_control::BreakpointSet,
        watchpoints: &mut crate::WatchpointObserver,
        budget_ceiling: u64,
    ) -> Result<crate::DebugRunOutcome, RuntimeError> {
        use crate::debug_control::DebugStopReason;
        use crate::state::ObservedContext;

        let DebugTarget { flow, world, local } = target;
        let mut steps: u64 = 0;
        // Same resume fix as `debug_run` — see its doc: skip the
        // breakpoint check on this call's first iteration so a resumed
        // call doesn't immediately re-report the breakpoint it's already
        // stopped at.
        let mut past_entry = false;
        loop {
            // Leftover-hit drain (#3226), BEFORE any stepping: a hit
            // already queued in the observer — a second write from one
            // step if an opcode ever gains one (today none does: the VM's
            // two `set_global` sites are single-write opcodes, pinned by
            // `one_step_never_queues_a_second_watchpoint_hit`), or a
            // stale hit from the observer's non-pausing logging mode —
            // reports HERE, at the position it was queued at, instead of
            // being attributed to whatever instruction the next step
            // happens to execute. This is what makes the post-step drain
            // below safe by construction rather than by that invariant.
            if let Some(hit) = watchpoints.take_hit() {
                return Ok(crate::DebugRunOutcome {
                    reason: DebugStopReason::Watchpoint {
                        global_idx: hit.global_idx,
                    },
                    position: Self::position_of(&flow.flow),
                    depth: Self::depth_of(&flow.flow),
                });
            }
            if past_entry
                && let Some(pos) = Self::position_of(&flow.flow)
                && let Some(bp) = breakpoints.hit(pos)
            {
                return Ok(crate::DebugRunOutcome {
                    reason: DebugStopReason::Breakpoint {
                        id: bp.id,
                        name: bp.name.clone(),
                    },
                    position: Some(pos),
                    depth: Self::depth_of(&flow.flow),
                });
            }
            past_entry = true;
            steps += 1;
            if steps > budget_ceiling {
                return Err(RuntimeError::DebugBudgetExceeded {
                    breakpoint: "run".to_owned(),
                    ceiling: budget_ceiling,
                });
            }

            let stepped = {
                let mut view = ContextView::new(&mut *world, &mut *local);
                let mut obs_ctx = ObservedContext::new(&mut view, watchpoints);
                vm::step::<R>(
                    &mut flow.flow,
                    env.program,
                    env.line_tables,
                    &mut obs_ctx,
                    &mut flow.stats,
                    env.resolver,
                )?
            };

            if let Some(hit) = watchpoints.take_hit() {
                return Ok(crate::DebugRunOutcome {
                    reason: DebugStopReason::Watchpoint {
                        global_idx: hit.global_idx,
                    },
                    position: Self::position_of(&flow.flow),
                    depth: Self::depth_of(&flow.flow),
                });
            }

            // A fresh (unobserved) view for the bookkeeping half — the
            // observed one was consumed by the step above, and bookkeeping
            // writes are not watchable state.
            let mut view = ContextView::new(&mut *world, &mut *local);
            match Self::debug_handle_stepped(stepped, env, flow, &mut view)? {
                SteppedDisposition::Continue
                | SteppedDisposition::Resumed
                | SteppedDisposition::CrossedExternal => {}
                SteppedDisposition::Stop(outcome) => return Ok(outcome),
            }
        }
    }

    /// Step the default flow by one [`StepMode`](crate::StepMode) unit,
    /// derived from call-stack depth deltas (`docs/debugger-spec.md` §4):
    ///
    /// - [`StepMode::Into`](crate::StepMode::Into): execute exactly one
    ///   instruction, descending into any newly-entered frame.
    /// - [`StepMode::Over`](crate::StepMode::Over): execute instructions
    ///   until back at (or still at) the starting depth — runs through any
    ///   call the first instruction makes without stopping inside it.
    /// - [`StepMode::Out`](crate::StepMode::Out): execute instructions
    ///   until the current frame returns to its caller (depth strictly
    ///   less than the starting depth). Refused up front, with
    ///   [`DebugStopReason::NoStepOutTarget`](crate::DebugStopReason::NoStepOutTarget)
    ///   and no VM stepping at all, when the starting depth is the
    ///   outermost (`Root`) frame — §4: "The debugger must disable
    ///   step-out... exactly as GDB disables `finish` in the outermost
    ///   frame" — **or** when the innermost frame is a
    ///   [`CallFrameType::Thread`]: §4's ruled `Thread` row ("a thread is
    ///   not a frame you can return from... must not offer step out as if
    ///   it returns anywhere", decision-log D1 entry item 11) applies the
    ///   same refusal for the same reason — a thread exhausting just pops
    ///   it (`vm::step`'s `Opcode::Done`/`Yield` handling), which is not a
    ///   return to a caller and must not be reported as `Step`.
    ///
    /// `breakpoints` is checked on every iteration after the first (same
    /// "skip the entry position" rule [`debug_run`](Self::debug_run)
    /// documents) — an armed breakpoint reached partway through a
    /// `StepMode::Over`/`Out` run halts the step early, before the
    /// matching instruction executes, exactly as it would inside
    /// `debug_run`. A `StepMode::Into` step always stops after its own
    /// single instruction, so it never reaches a second iteration where a
    /// breakpoint could fire mid-step.
    ///
    /// A choice point reached mid-step reports
    /// [`DebugStopReason::Choices`](crate::DebugStopReason::Choices) (with
    /// the same turn-index/auto-select bookkeeping
    /// [`debug_run`](Self::debug_run) applies), taking priority over the
    /// requested step's own stop condition — see `debug_run`'s doc.
    ///
    /// Bounded by `budget_ceiling` VM steps on the same terms as
    /// [`debug_run`](Self::debug_run) — never touches `Stats::steps`.
    ///
    /// # Errors
    /// [`RuntimeError::DebugBudgetExceeded`] if the step target is never
    /// reached within `budget_ceiling` VM steps (a `StepMode::Over`/`Out`
    /// whose target frame never returns — e.g. a runaway loop between
    /// entering and leaving it). Any other error `vm::step` itself can
    /// produce.
    #[cfg(feature = "debug-hooks")]
    pub fn debug_step(
        &mut self,
        mode: crate::debug_control::StepMode,
        breakpoints: &crate::debug_control::BreakpointSet,
        budget_ceiling: u64,
    ) -> Result<crate::DebugRunOutcome, RuntimeError> {
        self.debug_step_flow(None, &FallbackHandler, mode, breakpoints, budget_ceiling)
    }

    /// [`debug_step`](Self::debug_step) for any flow (#3223) — flow
    /// selection as in [`debug_run_flow`](Self::debug_run_flow).
    ///
    /// # Errors
    /// [`RuntimeError::UnknownFlow`] if `flow` names no live flow; then
    /// everything [`debug_step`](Self::debug_step) can raise.
    #[cfg(feature = "debug-hooks")]
    pub fn debug_step_flow(
        &mut self,
        flow: Option<&str>,
        handler: &dyn ExternalFnHandler,
        mode: crate::debug_control::StepMode,
        breakpoints: &crate::debug_control::BreakpointSet,
        budget_ceiling: u64,
    ) -> Result<crate::DebugRunOutcome, RuntimeError> {
        let (target, env) = self.debug_parts(flow, handler)?;
        Self::debug_step_impl(
            &env,
            target,
            mode,
            StepGranularity::Instruction,
            breakpoints,
            budget_ceiling,
        )
    }

    /// Advance to the next **source line** (#3264), the granularity every
    /// GDB-style debugger means by `step`/`next`/`finish`.
    ///
    /// Both granularities are first-class (RULED 2026-08-28): the studio
    /// presents the `.inkt` disassembly beside the source, so an author can
    /// watch a line and the instructions it became at the same time. This
    /// is not a replacement for [`Self::debug_step`] — it is the other verb.
    ///
    /// Implemented as the *same* loop with one more stop condition, not as
    /// a loop calling `debug_step`. That matters for the budget: nesting
    /// would let each inner step spend the full ceiling, so the real
    /// worst-case cost would be the ceiling squared. Here one budget
    /// governs the whole line step, and exceeding it reports
    /// `DebugBudgetExceeded` exactly as instruction stepping does — a line
    /// that never changes (a tight loop) cannot hang.
    ///
    /// Per mode:
    /// - **Into** stops at the first instruction on a different line,
    ///   whatever the depth — descending into a call lands on the callee's
    ///   first line, which is what "step into" means.
    /// - **Over** additionally requires the depth to be back at or below
    ///   where it started, so a call runs to completion instead of stopping
    ///   inside it.
    /// - **Out** is identical to its instruction form and deliberately does
    ///   NOT wait for a line change: returning lands mid-line at the call
    ///   site, which is exactly where GDB's `finish` stops. Requiring a
    ///   line change here would overshoot into the following line.
    ///
    /// Returns [`DebugStopReason::NoLineInfo`] when the artifact cannot say
    /// which line execution is on, rather than quietly behaving like
    /// [`Self::debug_step`].
    #[cfg(feature = "debug-hooks")]
    pub fn debug_step_line(
        &mut self,
        mode: crate::debug_control::StepMode,
        breakpoints: &crate::debug_control::BreakpointSet,
        budget_ceiling: u64,
    ) -> Result<crate::DebugRunOutcome, RuntimeError> {
        self.debug_step_line_flow(None, &FallbackHandler, mode, breakpoints, budget_ceiling)
    }

    /// [`debug_step_line`](Self::debug_step_line) for any flow (#3223) —
    /// flow selection as in [`debug_run_flow`](Self::debug_run_flow).
    ///
    /// # Errors
    /// [`RuntimeError::UnknownFlow`] if `flow` names no live flow; then
    /// everything [`debug_step_line`](Self::debug_step_line) can raise.
    #[cfg(feature = "debug-hooks")]
    pub fn debug_step_line_flow(
        &mut self,
        flow: Option<&str>,
        handler: &dyn ExternalFnHandler,
        mode: crate::debug_control::StepMode,
        breakpoints: &crate::debug_control::BreakpointSet,
        budget_ceiling: u64,
    ) -> Result<crate::DebugRunOutcome, RuntimeError> {
        let (target, env) = self.debug_parts(flow, handler)?;
        Self::debug_step_impl(
            &env,
            target,
            mode,
            StepGranularity::Line,
            breakpoints,
            budget_ceiling,
        )
    }

    #[cfg(feature = "debug-hooks")]
    fn debug_step_impl(
        env: &DebugEnv<'_>,
        target: DebugTarget<'_>,
        mode: crate::debug_control::StepMode,
        granularity: StepGranularity,
        breakpoints: &crate::debug_control::BreakpointSet,
        budget_ceiling: u64,
    ) -> Result<crate::DebugRunOutcome, RuntimeError> {
        use crate::debug_control::{DebugStopReason, StepMode};

        let DebugTarget { flow, world, local } = target;
        let depth_before = Self::depth_of(&flow.flow);
        let line_before =
            match Self::line_before(env.program, &flow.flow, granularity, depth_before) {
                Ok(l) => l,
                Err(outcome) => return Ok(outcome),
            };
        if let Some(outcome) = Self::no_step_out_target(&flow.flow, mode, depth_before) {
            return Ok(outcome);
        }

        let mut view = ContextView::new(world, local);
        let mut steps: u64 = 0;
        let mut past_entry = false;
        // #3224, spec §4: a call that pushes an `External` frame is opaque
        // — no ink bytecode inside — so once one is crossed, an `Into`
        // step adopts `Over`'s stop conditions (run until back at the
        // starting depth) instead of stopping after its single
        // instruction, which would strand the debugger inside a frame it
        // cannot step through (or, with an in-story fallback, inside the
        // fallback body `Into` was never asked to enter).
        let mut crossed_external = false;
        loop {
            if past_entry
                && let Some(pos) = Self::position_of(&flow.flow)
                && let Some(bp) = breakpoints.hit(pos)
            {
                return Ok(crate::DebugRunOutcome {
                    reason: DebugStopReason::Breakpoint {
                        id: bp.id,
                        name: bp.name.clone(),
                    },
                    position: Some(pos),
                    depth: Self::depth_of(&flow.flow),
                });
            }
            past_entry = true;
            steps += 1;
            if steps > budget_ceiling {
                return Err(RuntimeError::DebugBudgetExceeded {
                    breakpoint: "step".to_owned(),
                    ceiling: budget_ceiling,
                });
            }

            let stepped = vm::step::<R>(
                &mut flow.flow,
                env.program,
                env.line_tables,
                &mut view,
                &mut flow.stats,
                env.resolver,
            )?;

            match Self::debug_handle_stepped(stepped, env, flow, &mut view)? {
                SteppedDisposition::Continue => {}
                SteppedDisposition::Resumed => continue,
                SteppedDisposition::CrossedExternal => crossed_external = true,
                SteppedDisposition::Stop(outcome) => return Ok(outcome),
            }

            let effective_mode = if crossed_external && mode == StepMode::Into {
                StepMode::Over
            } else {
                mode
            };
            let depth_after = Self::depth_of(&flow.flow);
            let depth_ok = match effective_mode {
                StepMode::Into => true,
                StepMode::Over => depth_after <= depth_before,
                StepMode::Out => depth_after < depth_before,
            };
            // `Out` is the same verb at both granularities: returning lands
            // mid-line at the call site, which is where `finish` stops.
            let line_ok = match (granularity, effective_mode) {
                (StepGranularity::Instruction, _) | (StepGranularity::Line, StepMode::Out) => true,
                (StepGranularity::Line, _) => Self::line_key_of(&flow.flow, env.program)
                    .is_some_and(|now| Some(now) != line_before),
            };
            let stop = depth_ok && line_ok;
            if stop {
                let position = Self::position_of(&flow.flow);
                // A step that LANDS on an armed breakpoint reports the
                // BREAKPOINT, not the step (GDB's own behavior). Reporting
                // `Step` here made the breakpoint silently unhittable by
                // line-stepping (found live in the W5 studio review): the
                // next advance resumes FROM this address, where the
                // past-entry rule rightly skips it — so no call ever got
                // to claim the hit.
                if let Some(pos) = position
                    && let Some(bp) = breakpoints.hit(pos)
                {
                    return Ok(crate::DebugRunOutcome {
                        reason: DebugStopReason::Breakpoint {
                            id: bp.id,
                            name: bp.name.clone(),
                        },
                        position,
                        depth: depth_after,
                    });
                }
                return Ok(crate::DebugRunOutcome {
                    reason: DebugStopReason::Step,
                    position,
                    depth: depth_after,
                });
            }
        }
    }

    /// The `NoStepOutTarget` outcome when `Out` has nowhere to go — the
    /// outermost frame, or a `Thread` frame, which is not returnable-from
    /// (`docs/debugger-spec.md` names threads as a genuine non-analogue to
    /// GDB's frames). `None` for every other mode, and for any frame that
    /// can actually return.
    #[cfg(feature = "debug-hooks")]
    fn no_step_out_target(
        flow: &Flow,
        mode: crate::debug_control::StepMode,
        depth_before: usize,
    ) -> Option<crate::DebugRunOutcome> {
        use crate::debug_control::{DebugStopReason, StepMode};
        if mode != StepMode::Out {
            return None;
        }
        let innermost_is_thread = flow
            .current_thread()
            .call_stack
            .last()
            .is_some_and(|frame| frame.frame_type == CallFrameType::Thread);
        (depth_before <= 1 || innermost_is_thread).then(|| crate::DebugRunOutcome {
            reason: DebugStopReason::NoStepOutTarget,
            position: Self::position_of(flow),
            depth: depth_before,
        })
    }

    /// The line a `Line`-granular step starts from, captured BEFORE any
    /// stepping so "a different line" is measured from where the user was
    /// rather than from wherever the first instruction landed.
    ///
    /// `Err` carries the `NoLineInfo` outcome for an artifact that cannot
    /// say which line execution is on — returned rather than silently
    /// degrading to instruction stepping, which would turn a missing line
    /// index into "why does step take four presses" instead of "this build
    /// has no line info".
    #[cfg(feature = "debug-hooks")]
    fn line_before(
        program: &crate::Program,
        flow: &Flow,
        granularity: StepGranularity,
        depth_before: usize,
    ) -> Result<Option<(u32, u32)>, crate::DebugRunOutcome> {
        match granularity {
            StepGranularity::Instruction => Ok(None),
            StepGranularity::Line => {
                Self::line_key_of(flow, program)
                    .map(Some)
                    .ok_or_else(|| crate::DebugRunOutcome {
                        reason: crate::debug_control::DebugStopReason::NoLineInfo,
                        position: Self::position_of(flow),
                        depth: depth_before,
                    })
            }
        }
    }

    #[cfg(feature = "debug-hooks")]
    fn line_key_of(flow: &Flow, program: &crate::Program) -> Option<(u32, u32)> {
        Self::position_of(flow).and_then(|pos| program.debug_line_key(pos))
    }

    /// The current `(container_idx, offset)` of a flow, or `None` when
    /// the innermost frame's container stack is empty — same read
    /// `build_debug_snapshot`'s own `frame_position` closure performs.
    #[cfg(feature = "debug-hooks")]
    fn position_of(flow: &Flow) -> Option<crate::DebugPosition> {
        flow.current_thread()
            .call_stack
            .last()
            .and_then(|frame| frame.container_stack.last())
            .map(|cp| crate::DebugPosition {
                container_idx: cp.container_idx,
                offset: cp.offset,
            })
    }

    /// A flow's current thread's call-stack depth.
    #[cfg(feature = "debug-hooks")]
    fn depth_of(flow: &Flow) -> usize {
        flow.current_thread().call_stack.len()
    }
}

#[cfg(test)]
#[expect(clippy::panic)]
mod tests {
    use super::*;
    use crate::link;

    fn load_i079_program() -> (crate::Program, Vec<Vec<brink_format::LineEntry>>) {
        let data = brink_compiler::compile_path(std::path::Path::new(
            "../../tests/tier1/choices/I079-once-only-choices-can-link-back-to-self/story.ink",
        ))
        .unwrap()
        .data;
        link(&data).unwrap()
    }

    /// Step a story until it yields choices, panicking if it ends first.
    fn step_until_choices(story: &mut Story) -> Vec<Choice> {
        loop {
            match story.continue_single().unwrap() {
                Step::Choices(choices) => return choices,
                Step::Line(_) => {}
                Step::Done => panic!("story hit Done before presenting choices"),
                Step::End => panic!("story ended before presenting choices"),
                Step::Suspended => panic!("story parked before presenting choices"),
            }
        }
    }

    /// Step a story, accumulating text, until it stops (choices, done, or
    /// end) — returns the accumulated text for content assertions. Terminals
    /// carry no text themselves; any trailing content already arrived as
    /// its own preceding `Step::Line`.
    fn step_until_choices_or_end(story: &mut Story) -> String {
        let mut text = String::new();
        loop {
            match story.continue_single().unwrap() {
                Step::Choices(_) | Step::Done | Step::End | Step::Suspended => return text,
                Step::Line(line) => text.push_str(&line.text),
            }
        }
    }

    /// After selecting a once-only choice, the visit count for its target
    /// container must be > 0. Without this, the once-only filter in
    /// `handle_begin_choice` can never fire.
    #[test]
    fn select_choice_increments_visit_count_for_target() {
        let (program, line_tables) = load_i079_program();
        let mut story = Story::new(Arc::new(program), line_tables);
        let choices = step_until_choices(&mut story);

        assert!(!choices.is_empty(), "expected at least one choice");

        // Record the target_id of the first pending choice BEFORE selecting.
        let target_id = story.default.flow.pending_choices[0].target_id;
        let visit_before = story
            .default_context
            .visit_counts
            .get(&target_id)
            .copied()
            .unwrap_or(0);

        story.choose(0).unwrap();

        // After selection, the visit count for this target must have increased.
        let visit_after = story
            .default_context
            .visit_counts
            .get(&target_id)
            .copied()
            .unwrap_or(0);
        assert!(
            visit_after > visit_before,
            "visit count for choice target should increment after selection: \
             before={visit_before}, after={visit_after}"
        );
    }

    /// Build a linked `Story` directly from `.ink` source (no fixture file),
    /// for cases that need a specific choice shape not already in `tests/`.
    fn story_from_source(src: &str) -> Story {
        let out = brink_compiler::compile("main.ink", |_p| Ok(src.to_owned())).expect("compiles");
        let mut bytes = Vec::new();
        brink_format::write_inkb(&out.data, &mut bytes);
        let data = brink_format::read_inkb(&bytes).expect("decode");
        let (prog, tables) = link(&data).expect("link");
        Story::new(Arc::new(prog), tables)
    }

    /// FS-3w guard (`docs/flow-suspension-spec.md` §10.1): `Step::Suspended`
    /// ships on the `Step` surface now but is **runtime-unreachable until
    /// FS-3r** — the E052 lowering fence keeps `await` from producing
    /// bytecode, so no `park`/`spill`/`resume` path exists to construct it.
    /// This pins both halves: the variant's terminal contract (terminals
    /// carry no payload — §7), and that driving a representative story
    /// (including one that spins up a shared flow) never yields a
    /// `Suspended` step, and that `wake_check` reports no woken flows
    /// because none can park.
    #[test]
    fn step_suspended_is_terminal_and_never_constructed_in_runtime() {
        // The variant behaves like any other terminal: no payload, reports
        // terminal.
        let parked = Step::Suspended;
        assert_eq!(parked.text(), "");
        assert!(parked.tags().is_empty());
        assert!(parked.is_terminal(), "a park is a turn boundary");

        // Drive a small story with a shared flow to a terminal; nothing the
        // runtime produces is ever `Suspended`.
        let src = "Hello -> knot\n== knot ==\nWorld\n-> DONE\n";
        let mut story = story_from_source(src);
        story
            .spawn_flow_shared("f", None)
            .expect("spawn shared flow");
        for _ in 0..64 {
            let step = story.continue_single().expect("continue");
            assert!(
                !matches!(step, Step::Suspended),
                "runtime must never construct Step::Suspended before FS-3r"
            );
            if step.is_terminal() {
                break;
            }
        }
        for _ in 0..64 {
            let step = story.continue_flow_single("f").expect("continue flow");
            assert!(
                !matches!(step, Step::Suspended),
                "a shared flow must never construct Step::Suspended before FS-3r"
            );
            if step.is_terminal() {
                break;
            }
        }

        // No flow can park, so `wake_check` reports an empty woken set.
        assert!(
            story.wake_check().is_empty(),
            "wake_check returns no woken flows until parks exist (FS-3r)"
        );
    }

    /// #999: a shared flow that emits text forever must error at
    /// `FlowInstance::LINE_LIMIT` rather than growing `continue_flow_maximally_shared`'s
    /// returned `Vec<Step>` without bound — the shared-flow analogue of
    /// `drive_to_terminal_errors_at_line_limit` above, exercised through the
    /// `Story`-level entry point the wasm leg (`brink-web`) actually calls.
    #[test]
    fn continue_flow_maximally_shared_errors_at_line_limit() {
        let src = "-> spam\n\n=== spam ===\nLine.\n-> spam\n";
        let mut story = story_from_source(src);
        story
            .spawn_flow_shared("f", None)
            .expect("spawn shared flow at the root (immediately diverts into `spam`)");
        let err = story
            .continue_flow_maximally_shared("f")
            .expect_err("infinite-emitting flow should hit the line limit rather than hang");
        match err {
            RuntimeError::LineLimitExceeded(n) => {
                assert_eq!(n, FlowInstance::LINE_LIMIT);
            }
            other => panic!("expected LineLimitExceeded, got {other:?}"),
        }
    }

    /// `Choice.index` (the live, visible choice list) must be the *raw*
    /// `pending_choices` position, not the post-filter enumeration position —
    /// an invisible-default fallback choice (`* ->`) mixed in with visible
    /// choices occupies a `pending_choices` slot but never appears in the
    /// visible list, so the visible indices can skip values. This is exactly
    /// what `select_choice`/`choose` expects (it indexes `pending_choices`
    /// directly) — a caller must never re-derive the index from array
    /// position over the visible list alone.
    #[test]
    fn choice_index_is_raw_pending_choices_position_with_invisible_default_mixed_in() {
        let src = "-(start)\n\
             * [First] -> a\n\
             * -> b\n\
             * [Third] -> c\n\
             -(a) Went A.\n-> DONE\n\
             -(b) Went B.\n-> DONE\n\
             -(c) Went C.\n-> DONE\n";
        let mut story = story_from_source(src);
        let choices = step_until_choices(&mut story);

        // The invisible-default fallback (raw index 1) is filtered out of the
        // visible list, so the visible choices' indices skip it: 0, then 2.
        assert_eq!(
            choices.iter().map(|c| c.index).collect::<Vec<_>>(),
            vec![0, 2],
            "visible choice indices must be the raw pending_choices positions, not 0,1,..: {choices:?}"
        );
        assert_eq!(story.default.flow.pending_choices.len(), 3);

        // Choosing the raw index of the second visible entry must select
        // the "Third" branch, not the invisible-default fallback.
        story.choose(choices[1].index).expect("choose by raw index");
        let text = step_until_choices_or_end(&mut story);
        assert!(text.contains("Went C"), "expected the Third branch: {text}");
    }

    /// `DebugSnapshot.pending_choices[].index` must agree with the live
    /// `Choice.index` — both derive from the same pre-filter pass over
    /// `pending_choices` (`resolved_choices_for`). A studio consumer restoring
    /// a Choice[] from a `DebugSnapshot` (rather than a live `Choice` list)
    /// depends on this to dispatch `choose()` correctly.
    #[test]
    fn debug_snapshot_choice_index_matches_live_choice_index() {
        let src = "-(start)\n\
             * [First] -> a\n\
             * -> b\n\
             * [Third] -> c\n\
             -(a) Went A.\n-> DONE\n\
             -(b) Went B.\n-> DONE\n\
             -(c) Went C.\n-> DONE\n";
        let mut story = story_from_source(src);
        let live_choices = step_until_choices(&mut story);
        let snap = story.debug_snapshot();

        assert_eq!(snap.pending_choices.len(), live_choices.len());
        for (live, dbg) in live_choices.iter().zip(snap.pending_choices.iter()) {
            assert_eq!(
                dbg.index, live.index,
                "DebugChoice.index must match the live Choice.index"
            );
        }
    }

    /// On the second pass through a choice set with once-only choices,
    /// a choice whose target has already been visited must NOT appear
    /// in `pending_choices`.
    #[test]
    fn once_only_choice_excluded_on_second_pass() {
        let (program, line_tables) = load_i079_program();
        let mut story = Story::new(Arc::new(program), line_tables);

        let first_choices = step_until_choices(&mut story);
        assert!(
            first_choices
                .iter()
                .any(|c| c.text.contains("First choice")),
            "first pass should contain 'First choice', got: {first_choices:?}"
        );

        story.choose(0).unwrap();

        let second_choices = step_until_choices(&mut story);
        assert!(
            !second_choices
                .iter()
                .any(|c| c.text.contains("First choice")),
            "second pass should NOT contain 'First choice' (once-only, already visited), \
             got: {second_choices:?}"
        );
    }

    // ── Choice thread forking ──────────────────────────────────────────

    fn load_i083_program() -> (crate::Program, Vec<Vec<brink_format::LineEntry>>) {
        let data = brink_compiler::compile_path(std::path::Path::new(
            "../../tests/tier1/choices/I083-choice-thread-forking/story.ink",
        ))
        .unwrap()
        .data;
        link(&data).unwrap()
    }

    /// When a choice is created inside a tunnel, the call stack at that
    /// moment (including the tunnel frame with its temps) must be captured.
    /// After the tunnel returns and the choice is presented, the snapshot
    /// should still reflect the tunnel-era call stack depth (>= 2 frames).
    #[test]
    fn pending_choice_captures_tunnel_call_stack() {
        let (program, line_tables) = load_i083_program();
        let mut story = Story::new(Arc::new(program), line_tables);
        let _choices = step_until_choices(&mut story);

        // At this point the tunnel has returned, so the live call_stack
        // has only the root frame.
        let current_thread = story.default.flow.current_thread();
        assert_eq!(
            current_thread.call_stack.len(),
            1,
            "live call stack should be 1 frame (root) after tunnel return"
        );

        // But the pending choice's fork should have captured the
        // call stack from inside the tunnel (root + tunnel = 2 frames).
        assert!(!story.default.flow.pending_choices.is_empty());
        let fork = &story.default.flow.pending_choices[0].thread_fork;
        assert!(
            fork.call_stack.len() >= 2,
            "choice fork should have >= 2 frames (root + tunnel), got {}",
            fork.call_stack.len()
        );
    }

    /// After selecting a choice that was created inside a tunnel,
    /// `select_choice` must restore the tunnel's call frame so that
    /// temp variables from the tunnel scope are accessible.
    #[test]
    fn select_choice_restores_tunnel_frame_with_temps() {
        let (program, line_tables) = load_i083_program();
        let mut story = Story::new(Arc::new(program), line_tables);
        let _choices = step_until_choices(&mut story);

        // Before choosing: only root frame, no tunnel temps.
        assert_eq!(story.default.flow.current_thread().call_stack.len(), 1);

        story.choose(0).unwrap();

        // After choosing: the tunnel frame should be restored.
        // The call stack should have at least 2 frames (root + tunnel).
        let call_stack = &story.default.flow.current_thread().call_stack;
        assert!(
            call_stack.len() >= 2,
            "call stack should be restored to tunnel depth after choice selection, \
             got {} frame(s)",
            call_stack.len()
        );

        // The tunnel frame (last frame) should have temp x = Int(1).
        let tunnel_frame = call_stack.last().unwrap();
        assert!(
            !tunnel_frame.temps.is_empty(),
            "tunnel frame should have temp variables"
        );
        assert_eq!(
            tunnel_frame.temps[0],
            Value::Int(1),
            "tunnel frame temps[0] should be Int(1) (the parameter x)"
        );
    }

    // ── Tags ──────────────────────────────────────────────────────────

    fn load_tags_program() -> (crate::Program, Vec<Vec<brink_format::LineEntry>>) {
        let data = brink_compiler::compile_path(std::path::Path::new(
            "../../tests/tier3/tags/tags/story.ink",
        ))
        .unwrap()
        .data;
        link(&data).unwrap()
    }

    fn load_tags_in_choice_program() -> (crate::Program, Vec<Vec<brink_format::LineEntry>>) {
        let data = brink_compiler::compile_path(std::path::Path::new(
            "../../tests/tier3/tags/tagsInChoice/story.ink",
        ))
        .unwrap()
        .data;
        link(&data).unwrap()
    }

    #[test]
    fn line_exposes_tags() {
        let (program, line_tables) = load_tags_program();
        let mut story = Story::<crate::FastRng>::new(Arc::new(program), line_tables);
        let lines = story.continue_maximally().unwrap();
        // The first line should have both tags.
        let first = lines.first().expect("expected at least one line");
        assert!(
            !matches!(first, Step::Choices(_)),
            "expected Text or End, got Choices"
        );
        assert_eq!(first.tags(), &["author: Joe", "title: My Great Story"],);
    }

    #[test]
    fn choice_exposes_tags() {
        let (program, line_tables) = load_tags_in_choice_program();
        let mut story = Story::new(Arc::new(program), line_tables);
        let choices = step_until_choices(&mut story);
        assert!(!choices.is_empty());
        // The choice in tagsInChoice has tags "one" and "two"
        assert!(
            !choices[0].tags.is_empty(),
            "choice should have tags, got: {choices:?}"
        );
    }

    // ── Thread support ──────────────────────────────────────────────────

    fn load_i091_program() -> (crate::Program, Vec<Vec<brink_format::LineEntry>>) {
        let data = brink_compiler::compile_path(std::path::Path::new(
            "../../tests/tier1/choices/I091-choice-count/story.ink",
        ))
        .unwrap()
        .data;
        link(&data).unwrap()
    }

    /// `<- choices` (thread) must create choices AND return to the main
    /// flow so that `CHOICE_COUNT()` can evaluate. The thread body
    /// should be called like a tunnel — when its container stack empties,
    /// execution returns to the caller. Non-root frames must always pop
    /// back to their caller, even when pending choices exist.
    #[test]
    fn thread_call_returns_to_main_flow() {
        let (program, line_tables) = load_i091_program();
        let mut story = Story::<crate::FastRng>::new(Arc::new(program), line_tables);

        let lines = story.continue_maximally().unwrap();
        // I091 should output "2\n" (CHOICE_COUNT) then present 2 choices.
        let full_text: String = lines.iter().map(Step::text).collect();
        assert!(
            full_text.starts_with('2'),
            "output should start with '2' from CHOICE_COUNT(), got: {full_text:?}"
        );
        let last = lines.last().expect("expected at least one line");
        match last {
            Step::Choices(choices) => {
                assert_eq!(choices.len(), 2, "expected 2 choices");
            }
            other => panic!("expected Choices, got {other:?}"),
        }
    }

    // ── FlowInstance::drive_to_terminal (F6.1a shared drive-to-terminal op) ──

    /// Compile `.ink` source directly into a linked `(Program, line_tables)`
    /// pair, bypassing `Story` so tests can drive a bare `FlowInstance`
    /// directly — the way a `Story`-free consumer (e.g. an engine
    /// integration) would.
    fn compile_source_for_flow(src: &str) -> (crate::Program, Vec<Vec<brink_format::LineEntry>>) {
        let out = brink_compiler::compile("main.ink", |_p| Ok(src.to_owned())).expect("compiles");
        let mut bytes = Vec::new();
        brink_format::write_inkb(&out.data, &mut bytes);
        let data = brink_format::read_inkb(&bytes).expect("decode");
        link(&data).expect("link")
    }

    #[test]
    fn drive_to_terminal_stops_at_done() {
        let (program, tables) = compile_source_for_flow("Hello.\n-> DONE\n");
        let (mut flow, mut world) = FlowInstance::new_at_root(&program);
        let mut local = FlowLocal::new();
        let mut view = ContextView::new(&mut world, &mut local);
        let lines = flow
            .drive_to_terminal::<FastRng>(&program, &tables, &mut view, &FallbackHandler, None)
            .expect("drive succeeds");
        let (last, rest) = lines.split_last().expect("at least one line");
        assert!(matches!(last, Step::Done), "expected Done, got {last:?}");
        assert!(
            rest.iter().all(|l| matches!(l, Step::Line(_))),
            "every line before the terminal one should be Text, got {rest:?}"
        );
    }

    #[test]
    fn drive_to_terminal_stops_at_choices() {
        let (program, tables) =
            compile_source_for_flow("Hello.\n* Pick me\n    Picked.\n    -> DONE\n");
        let (mut flow, mut world) = FlowInstance::new_at_root(&program);
        let mut local = FlowLocal::new();
        let mut view = ContextView::new(&mut world, &mut local);
        let lines = flow
            .drive_to_terminal::<FastRng>(&program, &tables, &mut view, &FallbackHandler, None)
            .expect("drive succeeds");
        let (last, rest) = lines.split_last().expect("at least one line");
        assert!(
            matches!(last, Step::Choices(_)),
            "expected Choices, got {last:?}"
        );
        assert!(
            rest.iter().all(|l| matches!(l, Step::Line(_))),
            "every line before the terminal one should be Text, got {rest:?}"
        );
    }

    #[test]
    fn drive_to_terminal_stops_at_end() {
        let (program, tables) = compile_source_for_flow("Hello.\n-> END\n");
        let (mut flow, mut world) = FlowInstance::new_at_root(&program);
        let mut local = FlowLocal::new();
        let mut view = ContextView::new(&mut world, &mut local);
        let lines = flow
            .drive_to_terminal::<FastRng>(&program, &tables, &mut view, &FallbackHandler, None)
            .expect("drive succeeds");
        let (last, rest) = lines.split_last().expect("at least one line");
        assert!(matches!(last, Step::End), "expected End, got {last:?}");
        assert!(
            rest.iter().all(|l| matches!(l, Step::Line(_))),
            "every line before the terminal one should be Text, got {rest:?}"
        );
    }

    /// A knot that prints and re-diverts into itself forever never reaches a
    /// terminal line, so `drive_to_terminal` must give up at
    /// `FlowInstance::LINE_LIMIT` rather than looping forever — proving the
    /// extracted op kept `Story::continue_maximally_impl`'s safety cap.
    #[test]
    fn drive_to_terminal_errors_at_line_limit() {
        let (program, tables) =
            compile_source_for_flow("-> spam\n\n=== spam ===\nLine.\n-> spam\n");
        let (mut flow, mut world) = FlowInstance::new_at_root(&program);
        let mut local = FlowLocal::new();
        let mut view = ContextView::new(&mut world, &mut local);
        let err = flow
            .drive_to_terminal::<FastRng>(&program, &tables, &mut view, &FallbackHandler, None)
            .expect_err("infinite content should hit the line limit rather than hang");
        match err {
            RuntimeError::LineLimitExceeded(n) => {
                assert_eq!(n, FlowInstance::LINE_LIMIT);
            }
            other => panic!("expected LineLimitExceeded, got {other:?}"),
        }
    }

    // ── FlowInstance::drive (F6.2 pausable Layer-2 drive op) ─────────────

    /// Defers (`Pending`) its first call, then resolves — mirrors the
    /// `DeferOnce` pattern used for the flow-level resume gap elsewhere in
    /// the runtime's test suite (`tests/session.rs`, `tests/speculation.rs`).
    struct DeferOnce {
        deferred: std::cell::Cell<bool>,
    }

    impl ExternalFnHandler for DeferOnce {
        fn call(&self, name: &str, _args: &[Value]) -> ExternalResult {
            if name == "pause_once" && !self.deferred.get() {
                self.deferred.set(true);
                ExternalResult::Pending
            } else {
                ExternalResult::Resolved(Value::Int(2))
            }
        }
    }

    /// `drive` pauses cleanly (no error) on a deferred external, and resuming
    /// after [`FlowInstance::resolve_external`] continues the *same* logical
    /// drive to its terminal line — the pausable sibling of
    /// `drive_to_terminal`, which instead errors on a deferred external.
    #[test]
    fn drive_pauses_on_awaiting_external_then_resumes() {
        let (program, tables) = compile_source_for_flow(
            "EXTERNAL pause_once(x)\nHello.\nWorld.\nValue: {pause_once(1)}.\n-> DONE\n",
        );
        let (mut flow, mut world) = FlowInstance::new_at_root(&program);
        let mut local = FlowLocal::new();
        let mut view = ContextView::new(&mut world, &mut local);
        let handler = DeferOnce {
            deferred: std::cell::Cell::new(false),
        };
        let mut budget = 10usize;

        let outcome = flow
            .drive::<FastRng>(&program, &tables, &mut view, &handler, None, &mut budget)
            .expect("first drive call succeeds");
        let paused_lines = match outcome {
            DriveOutcome::AwaitingExternal(lines) => lines,
            other @ DriveOutcome::Terminal(_) => panic!("expected AwaitingExternal, got {other:?}"),
        };
        let paused_text: String = paused_lines.iter().map(Step::text).collect();
        assert!(
            paused_text.contains("Hello"),
            "text produced before the pause should include 'Hello.'; got {paused_text:?}"
        );
        assert!(
            !paused_text.contains("Value"),
            "the line calling the deferred external should not have completed yet; got {paused_text:?}"
        );
        assert_eq!(
            budget,
            10 - paused_lines.len(),
            "budget should decrement by exactly the lines produced before the pause"
        );

        flow.resolve_external(Value::Int(2));
        let outcome = flow
            .drive::<FastRng>(&program, &tables, &mut view, &handler, None, &mut budget)
            .expect("second drive call resumes and completes");
        let resumed_lines = match outcome {
            DriveOutcome::Terminal(lines) => lines,
            other @ DriveOutcome::AwaitingExternal(_) => panic!("expected Terminal, got {other:?}"),
        };
        let resumed_text: String = resumed_lines.iter().map(Step::text).collect();
        assert!(
            resumed_text.contains("Value: 2"),
            "the resolved external's value should be inlined; got {resumed_text:?}"
        );
        assert!(
            matches!(resumed_lines.last(), Some(Step::Done)),
            "expected the drive to finish at Done, got {resumed_lines:?}"
        );
        assert_eq!(
            budget,
            10 - paused_lines.len() - resumed_lines.len(),
            "budget must keep decrementing across the resume — not reset to a fresh cap \
             (the whole point of the caller-owned budget: one bound per logical drive, not \
             per resume)"
        );
    }

    /// A knot that prints and re-diverts into itself forever never reaches a
    /// terminal line. `drive` must give up when the caller's `budget` is
    /// exhausted — which can be far smaller than
    /// [`FlowInstance::LINE_LIMIT`] — rather than looping until the much
    /// larger production default, proving the budget is a real per-call
    /// parameter and not just a relabeling of the constant.
    #[test]
    fn drive_errors_when_caller_budget_exhausted() {
        let (program, tables) =
            compile_source_for_flow("-> spam\n\n=== spam ===\nLine.\n-> spam\n");
        let (mut flow, mut world) = FlowInstance::new_at_root(&program);
        let mut local = FlowLocal::new();
        let mut view = ContextView::new(&mut world, &mut local);
        let mut budget = 3usize;
        let err = flow
            .drive::<FastRng>(
                &program,
                &tables,
                &mut view,
                &FallbackHandler,
                None,
                &mut budget,
            )
            .expect_err("infinite content should hit the caller's small budget");
        match err {
            RuntimeError::LineLimitExceeded(n) => assert_eq!(
                n, 3,
                "reported limit should be the caller's budget, not the unrelated LINE_LIMIT constant"
            ),
            other => panic!("expected LineLimitExceeded, got {other:?}"),
        }
        assert_eq!(budget, 0, "budget should be fully consumed, not partially");
    }

    // ── free `save_state`/`load_state` (F6.1b) ───────────────────────────

    /// A `Story`-free save/load roundtrip: drive a bare `FlowInstance` +
    /// `ContextView` (no `Story` anywhere), capture state via the lifted
    /// `save_state` free function, mutate the live context, then restore via
    /// `load_state` and confirm the mutation is undone. Proves the lifted
    /// functions work for a consumer (e.g. `bevy-brink`) that never
    /// constructs a `Story`.
    #[test]
    fn free_fn_save_load_roundtrip_without_story() {
        let (program, tables) = compile_source_for_flow(
            "VAR gold = 0\n\
             -> shrine\n\
             === shrine ===\n\
             ~ gold = 5\n\
             Shrine text.\n\
             -> DONE\n\
             === reader ===\n\
             {READ_COUNT(-> shrine)}\n\
             -> DONE\n",
            // `reader` is never entered — it exists only so the compiler's
            // counting-flags pass sees a visit-count read of `shrine` and
            // sets `CountingFlags::VISITS` on it (a knot with no read of its
            // own visit count anywhere in the program has counting disabled
            // entirely, an existing compiler optimization).
        );
        let (mut flow, mut world) = FlowInstance::new_at_root(&program);
        let mut local = FlowLocal::new();
        {
            let mut view = ContextView::new(&mut world, &mut local);
            flow.drive_to_terminal::<FastRng>(&program, &tables, &mut view, &FallbackHandler, None)
                .expect("drive succeeds");
        }

        let gold_slot = program.global_index("gold").expect("gold declared");
        let shrine_id = program.find_path_target("shrine").expect("shrine exists");

        let save = {
            let view = ContextView::new(&mut world, &mut local);
            crate::save_state(&program, &view)
        };
        assert_eq!(save.globals.get("gold"), Some(&Value::Int(5)));
        assert_eq!(
            save.visits
                .iter()
                .find(|e| e.id == shrine_id)
                .map(|e| e.count),
            Some(1),
            "shrine should have a captured visit entry"
        );

        // Mutate the live context directly through the trait.
        {
            let mut view = ContextView::new(&mut world, &mut local);
            view.set_global(gold_slot, Value::Int(999));
            view.set_visit_count(shrine_id, 42);
        }
        {
            let view = ContextView::new(&mut world, &mut local);
            assert_eq!(view.global(gold_slot), &Value::Int(999));
            assert_eq!(view.visit_count(shrine_id), 42);
        }

        // Restore via the lifted `load_state` and confirm the mutation is
        // undone.
        let report = {
            let mut view = ContextView::new(&mut world, &mut local);
            crate::load_state(&program, &mut view, &save)
        };
        assert!(report.unknown_globals.is_empty(), "clean load: {report:?}");

        let view = ContextView::new(&mut world, &mut local);
        assert_eq!(view.global(gold_slot), &Value::Int(5));
        assert_eq!(view.visit_count(shrine_id), 1);
    }
}
