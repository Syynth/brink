//! The observable-equivalence oracle (tier 0) —
//! `docs/observable-semantics-spec.md` §2/§3.
//!
//! A [`Trace`] is *exactly* what a host can observe from one run: the output
//! steps in order, the choices **by order**, the external calls with their
//! arguments, the host-readable global state at every turn boundary, the
//! results of host-invoked functions, and the terminal kind reached. Nothing
//! on the spec's "explicitly unobservable" list (bytecode layout, container
//! numbering, step/instruction counts, timing, diagnostics, runtime warnings,
//! temps, the value/call stacks, visit counts as internals, RNG state as such)
//! is captured, so it cannot be compared.
//!
//! [`trace_diff`] replays the *same runs* on two compiled programs — given as
//! `.inkb` bytes, so both compile roads and any future optimizer output plug
//! in unchanged — and reports the first divergence per run.
//!
//! ## Why this is not `Episode`
//!
//! [`crate::episode::Episode`] is the record the **C# oracle** comparison is
//! built on, and it deliberately carries things the spec calls unobservable —
//! [`crate::episode::StateWrite::IncrementVisit`],
//! [`crate::episode::StateWrite::SetRngSeed`],
//! [`crate::episode::StateWrite::SetPreviousRandom`] — because the C# oracle
//! episodes record them. Reusing it as the equivalence definition would smuggle
//! internals into the definition, and adding fields to it would change the
//! on-disk golden-episode schema the ratchet reads. So the trace is its own
//! type, and the two harnesses stay independent.
//!
//! ## Coverage, not completeness
//!
//! Bounded exploration cannot *prove* equivalence for an unbounded program
//! (spec §3). Every cap here is a coverage bound, not a claim.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use brink_format::{StoryData, Value};
use brink_runtime::{DotNetRng, ExternalFnHandler, ExternalResult, Program, Step, Story};

/// Everything that can go wrong before a run even starts.
#[derive(Debug, thiserror::Error)]
pub enum TraceError {
    /// The `.inkb` bytes could not be decoded.
    #[error("decode .inkb: {0}")]
    Decode(String),
    /// The decoded story could not be linked into a [`Program`].
    #[error("link: {0}")]
    Link(String),
    /// A run named a start path the program does not resolve.
    #[error("start path {path:?} not found")]
    StartPathNotFound {
        /// The unresolvable path, as the run named it.
        path: String,
    },
}

// ── The run definition ──────────────────────────────────────────────────────

/// One **run** in the sense of the spec §2: a start point, an RNG seed, a
/// choice sequence, and a fixed set of external-function results.
///
/// A run is program-independent by construction — it names choices by index
/// (hosts pick by index) and never by text — which is what lets the *same*
/// run be replayed against two programs.
#[derive(Debug, Clone, PartialEq)]
pub struct RunSpec {
    /// Knot/stitch path to jump to before the first step; `None` starts at
    /// the story root.
    pub start_path: Option<String>,
    /// RNG seed applied before the first step; `None` leaves the program's
    /// own default seeding alone.
    pub seed: Option<i32>,
    /// Choice indices, in the order they are taken.
    pub choices: Vec<usize>,
    /// Host-invoked functions evaluated at every turn boundary, in order.
    /// Empty for the corpus sweeps — a probe is a host call with real side
    /// effects (it can write globals and consume RNG draws), so it is opt-in.
    pub probes: Vec<FunctionProbe>,
}

impl RunSpec {
    /// The empty run: story root, no seeding, no choices, no probes.
    #[must_use]
    pub fn root() -> Self {
        Self {
            start_path: None,
            seed: None,
            choices: Vec::new(),
            probes: Vec::new(),
        }
    }

    /// This run with `choice` appended to its choice sequence.
    #[must_use]
    pub fn then(&self, choice: usize) -> Self {
        let mut next = self.clone();
        next.choices.push(choice);
        next
    }
}

/// A host-invoked ink function (`Story::call_function`) evaluated at every
/// turn boundary of a run — spec §2 item 4.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionProbe {
    /// The function's ink-visible name.
    pub name: String,
    /// Arguments, in declaration order.
    pub args: Vec<Value>,
}

/// How external calls are answered during a run. Part of the run's
/// definition: "a fixed set of external-function results" (spec §2).
#[derive(Debug, Clone, Default)]
pub enum ExternalStubs {
    /// Every external call declines to the ink fallback body — what every
    /// existing corpus fixture already relies on.
    #[default]
    Fallback,
    /// Named externals resolve to a fixed value; anything unnamed falls back.
    /// A [`BTreeMap`] so iteration (and therefore the stub answer) is
    /// order-independent.
    Fixed(BTreeMap<String, Value>),
}

/// Bounds on capture and on the run-set exploration. Every field is a
/// coverage bound, never a semantic claim.
#[derive(Debug, Clone)]
pub struct TraceConfig {
    /// Maximum `continue_single` calls per run before recording
    /// [`Terminal::StepLimit`].
    pub max_steps: usize,
    /// Maximum choices taken in any one run during exploration.
    pub max_depth: usize,
    /// Maximum runs [`explore_runs`] returns.
    pub max_runs: usize,
    /// Seeds to explore under. Empty means "the program's own default
    /// seeding only". Equivalence must hold under *every* seed, so listing
    /// more seeds widens coverage and never changes the definition.
    pub seeds: Vec<i32>,
    /// Start points to explore from. Empty means the story root only.
    pub start_paths: Vec<String>,
    /// Probes attached to every explored run.
    pub probes: Vec<FunctionProbe>,
    /// How external calls are answered.
    pub externals: ExternalStubs,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            max_steps: 10_000,
            max_depth: 6,
            max_runs: 64,
            seeds: Vec::new(),
            start_paths: Vec::new(),
            probes: Vec::new(),
            externals: ExternalStubs::Fallback,
        }
    }
}

// ── The trace ───────────────────────────────────────────────────────────────

/// One choice as the host sees it. Compared **by order** (spec §2.1) — the
/// runtime's own `index` is carried for replay, not for comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceChoice {
    /// The choice's presented text.
    pub text: String,
    /// The choice's tags.
    pub tags: Vec<String>,
}

/// What a host got back from one external call.
#[derive(Debug, Clone, PartialEq)]
pub enum ExternalOutcome {
    /// The stub answered with this value.
    Resolved(Value),
    /// The stub declined; the ink fallback body ran.
    Fallback,
}

/// What a host-invoked function evaluation produced.
#[derive(Debug, Clone, PartialEq)]
pub enum ProbeOutcome {
    /// The function returned this value.
    Returned(Value),
    /// The call failed. The message is **not** compared — spec §6 puts fault
    /// text on the diagnostics channel — only the fact of failure is.
    Failed(String),
}

/// How a run ended. Part of the trace (spec §2 item 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Terminal {
    /// ink `-> DONE`, or a flow that ran out of content. `safe_exit` is
    /// [`brink_runtime::Story::did_safe_exit`] — the host-visible difference
    /// between the two, and the difference between "call again for more" and
    /// "the next call errors".
    Done {
        /// Whether the flow exited safely rather than running dry.
        safe_exit: bool,
    },
    /// ink `-> END`.
    Ended,
    /// A flow parked at an `await` site.
    Suspended,
    /// The run's choice sequence ran out while choices were on offer — the
    /// bounded-exploration cut, not a story outcome.
    ChoicesExhausted,
    /// The run's next choice index was past the end of what the program
    /// offered. Only reachable when replaying a run recorded against a
    /// *different* program, and itself a divergence.
    ChoiceOutOfRange {
        /// The index the run asked for.
        wanted: usize,
        /// How many choices were actually on offer.
        offered: usize,
    },
    /// [`TraceConfig::max_steps`] was hit — a coverage bound, not a story
    /// outcome.
    StepLimit,
    /// The run faulted. Per spec §6 the *fact* of a fault is observable; the
    /// message is not, so it is kept out of this variant and reported through
    /// [`Trace::fault_detail`] instead.
    Faulted,
}

/// One observable event, in the order a host would see it.
#[derive(Debug, Clone, PartialEq)]
pub enum TraceEvent {
    /// A `Step::Line`: its text, tags and `element.data` (spec §2 item 1).
    /// `element.kind` is deliberately absent — it reports `narrative` on
    /// every line today (no claim handler classifies it yet), so capturing it
    /// would record a constant, not an observable.
    Line {
        /// The line's text.
        text: String,
        /// The line's tags.
        tags: Vec<String>,
        /// The line's `element.data` payload.
        data: BTreeMap<String, String>,
    },
    /// A `Step::Choices`, in presentation order.
    Choices(Vec<TraceChoice>),
    /// An external call the host saw, with its arguments and the stubbed
    /// answer (spec §2 item 2).
    External {
        /// The ink-declared external's name.
        name: String,
        /// Arguments, in declaration order.
        args: Vec<Value>,
        /// What the stub answered.
        outcome: ExternalOutcome,
    },
    /// Host-readable global state at a turn boundary (spec §2 item 3): every
    /// global the host can read by name, in slot order. A global the host
    /// cannot read (`#@private` under visibility enforcement) is absent — it
    /// is not host-readable, which is precisely the property being captured.
    Globals(Vec<(String, Value)>),
    /// A host-invoked function's result at a turn boundary (spec §2 item 4).
    /// Such a call's own output is isolated and discarded by the runtime, so
    /// there is none to record.
    Probe {
        /// The probed function's name.
        name: String,
        /// The arguments it was called with.
        args: Vec<Value>,
        /// What it produced.
        outcome: ProbeOutcome,
    },
    /// The run's terminal kind.
    Terminal(Terminal),
}

impl TraceEvent {
    /// A short label naming *which observable* this event is, for diff
    /// reporting.
    #[must_use]
    pub fn observable(&self) -> &'static str {
        match self {
            Self::Line { .. } => "line",
            Self::Choices(_) => "choices",
            Self::External { .. } => "external call",
            Self::Globals(_) => "host-readable globals",
            Self::Probe { .. } => "host-invoked function result",
            Self::Terminal(_) => "terminal kind",
        }
    }
}

/// The full observable trace of one run.
#[derive(Debug, Clone)]
pub struct Trace {
    /// The run this trace came from.
    pub run: RunSpec,
    /// The observable events, in order.
    pub events: Vec<TraceEvent>,
    /// Turn-boundary index of each event, parallel to `events` — which turn
    /// a divergence landed in, for reporting.
    pub turns: Vec<usize>,
    /// The fault message, when the run ended in [`Terminal::Faulted`]. Kept
    /// out of `events` on purpose: spec §6 makes fault *text* a
    /// diagnostics-channel matter, so it is reported but never compared.
    pub fault_detail: Option<String>,
}

impl Trace {
    /// The choices presented at the last `Step::Choices` of this trace, if
    /// the trace ended waiting on one.
    fn trailing_choices(&self) -> Option<&[TraceChoice]> {
        if !matches!(
            self.events.last(),
            Some(TraceEvent::Terminal(Terminal::ChoicesExhausted))
        ) {
            return None;
        }
        self.events.iter().rev().find_map(|e| match e {
            TraceEvent::Choices(cs) => Some(cs.as_slice()),
            _ => None,
        })
    }
}

// ── Capture ─────────────────────────────────────────────────────────────────

/// Records every external call and answers it from the run's stubs.
struct StubHandler {
    stubs: ExternalStubs,
    calls: RefCell<Vec<TraceEvent>>,
}

impl StubHandler {
    fn new(stubs: ExternalStubs) -> Self {
        Self {
            stubs,
            calls: RefCell::new(Vec::new()),
        }
    }

    fn drain(&self) -> Vec<TraceEvent> {
        core::mem::take(&mut self.calls.borrow_mut())
    }
}

impl ExternalFnHandler for StubHandler {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
        let answer = match &self.stubs {
            ExternalStubs::Fallback => None,
            ExternalStubs::Fixed(map) => map.get(name).cloned(),
        };
        let outcome = match &answer {
            Some(v) => ExternalOutcome::Resolved(v.clone()),
            None => ExternalOutcome::Fallback,
        };
        self.calls.borrow_mut().push(TraceEvent::External {
            name: name.to_owned(),
            args: args.to_vec(),
            outcome,
        });
        match answer {
            Some(v) => ExternalResult::Resolved(v),
            None => ExternalResult::Fallback,
        }
    }
}

/// A linked program plus its line tables — the unit both sides of a
/// [`trace_diff`] are loaded into once and replayed from many times.
pub struct LinkedProgram {
    program: Arc<Program>,
    line_tables: Vec<Vec<brink_format::LineEntry>>,
}

impl fmt::Debug for LinkedProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinkedProgram")
            .field("globals", &self.program.global_count())
            .field("line_tables", &self.line_tables.len())
            .finish()
    }
}

impl LinkedProgram {
    /// Link a compiled story so runs can be replayed against it.
    pub fn from_story_data(data: &StoryData) -> Result<Self, TraceError> {
        let (program, line_tables) =
            brink_runtime::link(data).map_err(|e| TraceError::Link(e.to_string()))?;
        Ok(Self {
            program: Arc::new(program),
            line_tables,
        })
    }

    /// Link a program from `.inkb` bytes — the form [`trace_diff`] takes, so
    /// both compile roads and any future optimizer output plug in unchanged.
    pub fn from_inkb(bytes: &[u8]) -> Result<Self, TraceError> {
        let data = brink_format::read_inkb(bytes).map_err(|e| TraceError::Decode(e.to_string()))?;
        Self::from_story_data(&data)
    }
}

/// Serialize a compiled story to `.inkb` bytes.
#[must_use]
pub fn to_inkb(data: &StoryData) -> Vec<u8> {
    let mut buf = Vec::new();
    brink_format::write_inkb(data, &mut buf);
    buf
}

/// Read every global the host can read, in slot order.
///
/// The read goes through [`brink_runtime::Story::variable`] — the host's own
/// road (`getVar`) — so what is captured is exactly what a host can see: a
/// `#@private` global under visibility enforcement returns `None` and is
/// therefore absent from the capture, which is the correct answer for
/// "host-readable state", not an omission.
fn host_readable_globals(story: &Story<DotNetRng>) -> Vec<(String, Value)> {
    let count = story.program().global_count();
    let mut out = Vec::new();
    for idx in 0..count {
        let Some(name) = story.program().global_name(idx) else {
            continue;
        };
        let name = name.to_owned();
        if let Some(value) = story.variable(&name) {
            out.push((name, value.clone()));
        }
    }
    out
}

/// Capture one run's observable trace against a linked program.
pub fn capture(
    linked: &LinkedProgram,
    run: &RunSpec,
    config: &TraceConfig,
) -> Result<Trace, TraceError> {
    let mut story =
        Story::<DotNetRng>::new(Arc::clone(&linked.program), linked.line_tables.clone());
    if let Some(seed) = run.seed {
        story.set_rng_seed(seed);
    }
    if let Some(path) = &run.start_path
        && story.choose_path_string(path).is_err()
    {
        return Err(TraceError::StartPathNotFound { path: path.clone() });
    }

    let handler = StubHandler::new(config.externals.clone());
    let mut trace = Trace {
        run: run.clone(),
        events: Vec::new(),
        turns: Vec::new(),
        fault_detail: None,
    };
    let mut turn = 0usize;

    // The story's start is a turn boundary the host can read from.
    turn_boundary(&mut trace, &mut story, &handler, run, turn);

    let mut next_choice = 0usize;
    for _ in 0..config.max_steps {
        let step = match story.continue_single_with(&handler) {
            Ok(step) => step,
            Err(e) => {
                trace.fault_detail = Some(e.to_string());
                push(&mut trace, TraceEvent::Terminal(Terminal::Faulted), turn);
                return Ok(trace);
            }
        };
        for call in handler.drain() {
            push(&mut trace, call, turn);
        }

        match step {
            Step::Line(line) => push(
                &mut trace,
                TraceEvent::Line {
                    text: line.text,
                    tags: line.tags,
                    data: line.element.data,
                },
                turn,
            ),
            Step::Done => {
                let safe_exit = story.did_safe_exit();
                finish(
                    &mut trace,
                    &mut story,
                    &handler,
                    run,
                    turn,
                    Terminal::Done { safe_exit },
                );
                return Ok(trace);
            }
            Step::End => {
                finish(&mut trace, &mut story, &handler, run, turn, Terminal::Ended);
                return Ok(trace);
            }
            Step::Suspended => {
                finish(
                    &mut trace,
                    &mut story,
                    &handler,
                    run,
                    turn,
                    Terminal::Suspended,
                );
                return Ok(trace);
            }
            Step::Choices(choices) => {
                let presented: Vec<TraceChoice> = choices
                    .iter()
                    .map(|c| TraceChoice {
                        text: c.text.clone(),
                        tags: c.tags.clone(),
                    })
                    .collect();
                push(&mut trace, TraceEvent::Choices(presented), turn);
                turn_boundary(&mut trace, &mut story, &handler, run, turn);
                turn += 1;
                if take_choice(
                    &mut trace,
                    &mut story,
                    &choices,
                    run,
                    &mut next_choice,
                    turn,
                ) == Advance::Stop
                {
                    return Ok(trace);
                }
            }
        }
    }

    push(&mut trace, TraceEvent::Terminal(Terminal::StepLimit), turn);
    Ok(trace)
}

/// Whether [`take_choice`] left the run able to continue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Advance {
    /// The run selected a choice and can keep stepping.
    Continue,
    /// The run ended here; its terminal event is already recorded.
    Stop,
}

/// Take the run's next choice, recording the terminal that ends the run when
/// the run has no next choice, when it names one the program does not offer,
/// or when selecting it faults.
fn take_choice(
    trace: &mut Trace,
    story: &mut Story<DotNetRng>,
    choices: &[brink_runtime::Choice],
    run: &RunSpec,
    next_choice: &mut usize,
    turn: usize,
) -> Advance {
    let Some(&wanted) = run.choices.get(*next_choice) else {
        push(
            trace,
            TraceEvent::Terminal(Terminal::ChoicesExhausted),
            turn,
        );
        return Advance::Stop;
    };
    *next_choice += 1;
    let Some(choice) = choices.get(wanted) else {
        push(
            trace,
            TraceEvent::Terminal(Terminal::ChoiceOutOfRange {
                wanted,
                offered: choices.len(),
            }),
            turn,
        );
        return Advance::Stop;
    };
    if let Err(e) = story.choose(choice.index) {
        trace.fault_detail = Some(e.to_string());
        push(trace, TraceEvent::Terminal(Terminal::Faulted), turn);
        return Advance::Stop;
    }
    Advance::Continue
}

/// Record a run's terminal kind, then the turn boundary that follows it.
fn finish(
    trace: &mut Trace,
    story: &mut Story<DotNetRng>,
    handler: &StubHandler,
    run: &RunSpec,
    turn: usize,
    terminal: Terminal,
) {
    push(trace, TraceEvent::Terminal(terminal), turn);
    turn_boundary(trace, story, handler, run, turn);
}

fn push(trace: &mut Trace, event: TraceEvent, turn: usize) {
    trace.events.push(event);
    trace.turns.push(turn);
}

/// Record host-readable state and every function probe at a turn boundary.
fn turn_boundary(
    trace: &mut Trace,
    story: &mut Story<DotNetRng>,
    handler: &StubHandler,
    run: &RunSpec,
    turn: usize,
) {
    push(
        trace,
        TraceEvent::Globals(host_readable_globals(story)),
        turn,
    );
    for probe in &run.probes {
        let outcome = match story.call_function(&probe.name, &probe.args, handler) {
            Ok(value) => ProbeOutcome::Returned(value),
            Err(e) => ProbeOutcome::Failed(e.to_string()),
        };
        for call in handler.drain() {
            push(trace, call, turn);
        }
        push(
            trace,
            TraceEvent::Probe {
                name: probe.name.clone(),
                args: probe.args.clone(),
                outcome,
            },
            turn,
        );
    }
}

// ── Run-set exploration ─────────────────────────────────────────────────────

/// Explore a program's choice tree and return the runs it reaches.
///
/// The result is the *run set* [`trace_diff`] replays on both sides. Runs are
/// enumerated depth-first with choice 0 first, so the set is deterministic.
pub fn explore_runs(
    linked: &LinkedProgram,
    config: &TraceConfig,
) -> Result<Vec<RunSpec>, TraceError> {
    let mut seeds: Vec<Option<i32>> = config.seeds.iter().copied().map(Some).collect();
    if seeds.is_empty() {
        seeds.push(None);
    }
    let mut starts: Vec<Option<String>> = config.start_paths.iter().cloned().map(Some).collect();
    if starts.is_empty() {
        starts.push(None);
    }

    let mut runs = Vec::new();
    for start in &starts {
        for seed in &seeds {
            let root = RunSpec {
                start_path: start.clone(),
                seed: *seed,
                choices: Vec::new(),
                probes: config.probes.clone(),
            };
            let mut stack = vec![root];
            while let Some(run) = stack.pop() {
                if runs.len() >= config.max_runs {
                    return Ok(runs);
                }
                let trace = capture(linked, &run, config)?;
                let branches = trace.trailing_choices().map_or(0, <[TraceChoice]>::len);
                if branches == 0 || run.choices.len() >= config.max_depth {
                    runs.push(run);
                    continue;
                }
                // Push in reverse so choice 0 is explored first.
                for i in (0..branches).rev() {
                    stack.push(run.then(i));
                }
            }
        }
    }
    Ok(runs)
}

/// Explore a program's run set and capture each run's trace.
///
/// The mutation study (`docs/observable-semantics-spec.md` §4 tier 3a) needs
/// the traces themselves, not just the runs: a mutant is only *grounded* when
/// the site it edits is something the baseline trace demonstrably exercised.
pub fn explore_traces(
    linked: &LinkedProgram,
    config: &TraceConfig,
) -> Result<Vec<Trace>, TraceError> {
    explore_runs(linked, config)?
        .iter()
        .map(|run| capture(linked, run, config))
        .collect()
}

/// Explore a program from `.inkb` bytes and return its run set.
pub fn explore_runs_from_inkb(
    program: &[u8],
    config: &TraceConfig,
) -> Result<Vec<RunSpec>, TraceError> {
    explore_runs(&LinkedProgram::from_inkb(program)?, config)
}

// ── The diff ────────────────────────────────────────────────────────────────

/// Which side of a [`trace_diff`] something happened on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The left-hand program, `P`.
    Left,
    /// The right-hand program, `Q`.
    Right,
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Left => f.write_str("P"),
            Self::Right => f.write_str("Q"),
        }
    }
}

/// What diverged.
#[derive(Debug, Clone)]
pub enum DivergenceKind {
    /// Both sides had an event here and they differ.
    Differs {
        /// `P`'s event.
        left: Box<TraceEvent>,
        /// `Q`'s event.
        right: Box<TraceEvent>,
    },
    /// One side produced an event the other did not reach.
    Missing {
        /// The side that ran out of events.
        side: Side,
        /// The event the other side had.
        event: Box<TraceEvent>,
    },
    /// Exactly one side faulted. Fault *text* is not compared (spec §6); the
    /// messages are carried for reporting only.
    FaultAsymmetry {
        /// The side that faulted.
        side: Side,
        /// That side's fault message.
        detail: Option<String>,
    },
}

/// One run's first divergence.
#[derive(Debug, Clone)]
pub struct Divergence {
    /// Index of the run in the run set handed to [`trace_diff`].
    pub run_index: usize,
    /// The run itself.
    pub run: RunSpec,
    /// The turn boundary the divergence landed in.
    pub turn: usize,
    /// The event index within the trace.
    pub event_index: usize,
    /// What diverged.
    pub kind: DivergenceKind,
}

/// The structured result of [`trace_diff`].
#[derive(Debug, Clone)]
pub struct TraceDiff {
    /// How many runs were replayed on both sides.
    pub total_runs: usize,
    /// The first divergence of every diverging run, in run order.
    pub divergences: Vec<Divergence>,
}

impl TraceDiff {
    /// Whether the two programs agreed on every replayed run.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.divergences.is_empty()
    }

    /// The first divergence across all runs, if any.
    #[must_use]
    pub fn first(&self) -> Option<&Divergence> {
        self.divergences.first()
    }
}

impl fmt::Display for TraceDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.divergences.is_empty() {
            return write!(f, "observably equivalent over {} run(s)", self.total_runs);
        }
        writeln!(
            f,
            "{} of {} run(s) diverged:",
            self.divergences.len(),
            self.total_runs
        )?;
        for d in &self.divergences {
            writeln!(
                f,
                "  run {} (choices {:?}, seed {:?}, start {:?}) turn {} event {}:",
                d.run_index, d.run.choices, d.run.seed, d.run.start_path, d.turn, d.event_index
            )?;
            match &d.kind {
                DivergenceKind::Differs { left, right } => {
                    writeln!(f, "    observable: {}", left.observable())?;
                    writeln!(f, "    P: {left:?}")?;
                    writeln!(f, "    Q: {right:?}")?;
                }
                DivergenceKind::Missing { side, event } => {
                    writeln!(
                        f,
                        "    {side} ran out of events; the other had {} {event:?}",
                        event.observable()
                    )?;
                }
                DivergenceKind::FaultAsymmetry { side, detail } => {
                    writeln!(f, "    {side} faulted and the other did not: {detail:?}")?;
                }
            }
        }
        Ok(())
    }
}

/// Replay `runs` on both programs and report the first divergence per run.
///
/// `p` and `q` are `.inkb` bytes. This is the executable form of the
/// equivalence definition (`docs/observable-semantics-spec.md` §3): an empty
/// result means the two programs are observably equivalent **over these
/// runs** — coverage, not proof.
pub fn trace_diff(p: &[u8], q: &[u8], runs: &[RunSpec]) -> Result<TraceDiff, TraceError> {
    let config = TraceConfig::default();
    trace_diff_with(p, q, runs, &config)
}

/// The tier-0 corpus-differential entry point: explore `pre`'s runs, then
/// replay exactly those runs on `post`.
///
/// This is the shape every consumer of the oracle wants — the corpus
/// self-check (`pre` and `post` being two compiles of the same source), the
/// mutation study (`post` being a mutant), and, when it exists, an optimizer
/// (`post` being `opt(pre)`). Runs come from `pre` because `pre` is the
/// program whose behaviour is being preserved.
pub fn differential(
    pre: &[u8],
    post: &[u8],
    config: &TraceConfig,
) -> Result<TraceDiff, TraceError> {
    let linked = LinkedProgram::from_inkb(pre)?;
    let runs = explore_runs(&linked, config)?;
    trace_diff_with(pre, post, &runs, config)
}

/// [`trace_diff`] with explicit bounds and stubs.
pub fn trace_diff_with(
    p: &[u8],
    q: &[u8],
    runs: &[RunSpec],
    config: &TraceConfig,
) -> Result<TraceDiff, TraceError> {
    let left = LinkedProgram::from_inkb(p)?;
    let right = LinkedProgram::from_inkb(q)?;
    let mut divergences = Vec::new();
    for (run_index, run) in runs.iter().enumerate() {
        let lt = capture(&left, run, config)?;
        let rt = capture(&right, run, config)?;
        if let Some(kind_at) = first_divergence(&lt, &rt) {
            let (event_index, kind) = kind_at;
            divergences.push(Divergence {
                run_index,
                run: run.clone(),
                turn: lt
                    .turns
                    .get(event_index)
                    .copied()
                    .or_else(|| rt.turns.get(event_index).copied())
                    .unwrap_or(0),
                event_index,
                kind,
            });
        }
    }
    Ok(TraceDiff {
        total_runs: runs.len(),
        divergences,
    })
}

fn first_divergence(left: &Trace, right: &Trace) -> Option<(usize, DivergenceKind)> {
    let max = left.events.len().max(right.events.len());
    for i in 0..max {
        match (left.events.get(i), right.events.get(i)) {
            (Some(a), Some(b)) => {
                if a != b {
                    return Some((
                        i,
                        DivergenceKind::Differs {
                            left: Box::new(a.clone()),
                            right: Box::new(b.clone()),
                        },
                    ));
                }
            }
            (Some(a), None) => {
                return Some((
                    i,
                    DivergenceKind::Missing {
                        side: Side::Right,
                        event: Box::new(a.clone()),
                    },
                ));
            }
            (None, Some(b)) => {
                return Some((
                    i,
                    DivergenceKind::Missing {
                        side: Side::Left,
                        event: Box::new(b.clone()),
                    },
                ));
            }
            (None, None) => break,
        }
    }
    // Event sequences agree. A fault on one side only would already have
    // shown up as a `Terminal::Faulted` mismatch above — fault *text* is
    // never compared (spec §6), so there is nothing left to check.
    None
}

// ── Translation identity (spec §2.2) ────────────────────────────────────────

/// One line whose translation identity moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineIdentityChange {
    /// A scope present on one side only.
    ScopeOnlyIn {
        /// Which side has it.
        side: Side,
        /// The scope's id, as `export_lines` renders it.
        scope_id: String,
    },
    /// A line index present on one side only.
    LineOnlyIn {
        /// Which side has it.
        side: Side,
        /// The owning scope's id.
        scope_id: String,
        /// The line's index within the scope.
        index: u16,
    },
    /// A line whose text hash changed.
    HashChanged {
        /// The owning scope's id.
        scope_id: String,
        /// The line's index within the scope.
        index: u16,
        /// `P`'s hash.
        before: String,
        /// `Q`'s hash.
        after: String,
    },
}

/// The result of [`line_identity_diff`].
#[derive(Debug, Clone, Default)]
pub struct LineIdentityDiff {
    /// Every identity change, in scope-id then line-index order.
    pub changes: Vec<LineIdentityChange>,
}

impl LineIdentityDiff {
    /// Whether every line kept its identity.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

impl fmt::Display for LineIdentityDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.changes.is_empty() {
            return f.write_str("translation identity unchanged");
        }
        writeln!(f, "{} line identity change(s):", self.changes.len())?;
        for change in &self.changes {
            writeln!(f, "  {change:?}")?;
        }
        Ok(())
    }
}

/// Diff two programs' exported line tables by identity — scope id plus per-line
/// text hash (`docs/observable-semantics-spec.md` §2.2).
///
/// This is a **separate obligation** from [`trace_diff`]: it is not
/// runtime-observable, it is what XLIFF units and `.inkl` overlays bind to. A
/// transformation can pass the trace diff and still orphan every translation.
pub fn line_identity_diff(p: &StoryData, q: &StoryData) -> LineIdentityDiff {
    let left = line_identity_index(p);
    let right = line_identity_index(q);
    let mut changes = Vec::new();

    for (scope_id, lines) in &left {
        let Some(other) = right.get(scope_id) else {
            changes.push(LineIdentityChange::ScopeOnlyIn {
                side: Side::Left,
                scope_id: scope_id.clone(),
            });
            continue;
        };
        for (index, hash) in lines {
            match other.get(index) {
                Some(other_hash) if other_hash == hash => {}
                Some(other_hash) => changes.push(LineIdentityChange::HashChanged {
                    scope_id: scope_id.clone(),
                    index: *index,
                    before: hash.clone(),
                    after: other_hash.clone(),
                }),
                None => changes.push(LineIdentityChange::LineOnlyIn {
                    side: Side::Left,
                    scope_id: scope_id.clone(),
                    index: *index,
                }),
            }
        }
    }

    for (scope_id, lines) in &right {
        let Some(other) = left.get(scope_id) else {
            changes.push(LineIdentityChange::ScopeOnlyIn {
                side: Side::Right,
                scope_id: scope_id.clone(),
            });
            continue;
        };
        for index in lines.keys() {
            if !other.contains_key(index) {
                changes.push(LineIdentityChange::LineOnlyIn {
                    side: Side::Right,
                    scope_id: scope_id.clone(),
                    index: *index,
                });
            }
        }
    }

    LineIdentityDiff { changes }
}

/// `scope id -> line index -> text hash`, from the real exporter
/// (`brink_intl::export_lines`) so the identity compared here is the identity
/// XLIFF and `.inkl` overlays actually bind to. [`BTreeMap`] throughout, so
/// the reported order is deterministic.
fn line_identity_index(data: &StoryData) -> BTreeMap<String, BTreeMap<u16, String>> {
    let exported = brink_intl::export_lines(data, 0);
    let mut index: BTreeMap<String, BTreeMap<u16, String>> = BTreeMap::new();
    for scope in exported.scopes {
        let entry = index.entry(scope.id).or_default();
        for line in scope.lines {
            entry.insert(line.index, line.hash);
        }
    }
    index
}
