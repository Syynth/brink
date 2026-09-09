//! The play session: the runtime, owned by the worker beside the analysis
//! session (`docs/gpui-studio-spec.md` §3 — nothing of the engine on the
//! main thread), driven by plain-data commands and answering in plain-data
//! steps.
//!
//! One session at a time. Starting compiles the project fresh from the
//! session's current text — the db memoizes, so an unchanged project costs
//! nothing — links it, and runs to the first yield point. A choice resumes
//! it. Edits made after a start are not folded in: the story keeps running
//! on what it was compiled from, and the UI says so; a restart picks them
//! up. That is the web studio's rule too, and the honest one — hot-swapping
//! a program under a running VM is its own project.

use std::sync::Arc;

use brink_ide::session::IdeSession;
use brink_runtime::debug_control::{
    BreakpointSet, DEFAULT_DEBUG_BUDGET, DebugStopReason, StepMode,
};
use brink_runtime::{FastRng, Step, Story};

use crate::query::Location;

/// What the UI asks of the play session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayCommand {
    /// Compile and start. `at` is a knot or `knot.stitch` path to divert to
    /// before the first line — "Play from here". `None` plays from the
    /// entry.
    Start {
        at: Option<String>,
    },
    /// Take the choice at `index` (a [`PlayChoice::index`]) and run on.
    Choose(usize),
    /// Drop the session.
    Stop,
    /// Replace the whole breakpoint set. Whole-set rather than
    /// add/remove: the editor owns the gutter marks, and reconciling two
    /// copies of a set is how they drift apart. Answered with a
    /// [`PlayOutcome`] whose `unbound` names every line that bound to
    /// nothing — a comment, a blank, or code that folded away — so the
    /// studio can say so instead of arming something that can never hit.
    SetBreakpoints(Vec<(String, u32)>),
    /// Run until a breakpoint, a choice point, or the story ends.
    Continue,
    /// One source line (`step`), or one VM instruction (`stepi`). Both
    /// are first-class (RULED 2026-08-28) — the Program Explorer shows
    /// the disassembly beside the source, so an author watches a line and
    /// the instructions it became at once.
    StepLine,
    StepInstruction,
    /// Read the running story's state without advancing it — what the
    /// State View shows. Answered with a [`PlayOutcome`] carrying no
    /// steps and a `state`; a session that is not running answers with
    /// `state: None` rather than an error, since "nothing is running" is
    /// a state the panel has something to say about.
    Snapshot,
}

/// The running story's state, as the State View reads it.
///
/// A flattened `brink_runtime::DebugSnapshot`: the runtime already
/// assembles all of this (status, position, globals, call stack, visit
/// counts, pending choices, RNG), so the panel needs no engine work — it
/// needed a way to ASK, which is [`PlayCommand::Snapshot`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayState {
    /// `active` / `waiting_for_choice` / `done` / `ended`.
    pub status: String,
    /// The nearest named knot or stitch the cursor is in.
    pub location: Option<String>,
    pub turn: u32,
    /// Globals as `(name, value)`, in the runtime's order.
    pub globals: Vec<(String, String)>,
    /// Call frames, innermost first: `(kind, location)`.
    pub call_stack: Vec<(String, Option<String>)>,
    /// Visit counts by path, sorted by path — anonymous containers are
    /// left out, as the runtime's own path-resolved list does.
    pub visits: Vec<(String, u32)>,
    /// The choices on offer, as the reader sees them.
    pub choices: Vec<String>,
    /// The story RNG, as the runtime reports it: `(seed, previous)`.
    pub rng: (i32, i32),
    /// The instruction about to run: `(container_idx, offset)`, keyed the
    /// way the Program Explorer's disassembly rows are (D9/#3187, which
    /// put `container_idx` on the model for exactly this). `None` when
    /// the innermost frame has no open container — an exhausted flow, or
    /// one parked on a deferred external.
    pub position: Option<(u32, usize)>,
    /// Set when this is the state a RUNTIME FAULT left behind rather than
    /// a live story's. The VM is dropped either way; what the author needs
    /// is the state it died in — which globals held what, where the flow
    /// was — so it is read off the story before the drop and answered here
    /// until the next Start. Panels must say so rather than draw a corpse
    /// as a running session.
    pub faulted: Option<Fault>,
}

/// One step of story output, the runtime's [`Step`] with only what a
/// transcript needs, and locations in the compiler's file keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayStep {
    Line {
        text: String,
        tags: Vec<String>,
        /// Where the line came from, when the line table knows.
        source: Option<Location>,
    },
    Choices(Vec<PlayChoice>),
    /// This turn's output is complete; nothing more runs until a choice —
    /// and there is none, so the flow is at rest.
    Done,
    /// `-> END`.
    End,
    /// Parked at an `await` site; the studio has no wake affordance yet.
    Suspended,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayChoice {
    pub text: String,
    /// The value [`PlayCommand::Choose`] takes.
    pub index: usize,
    pub tags: Vec<String>,
    /// Written `+` (offered again) rather than `*`.
    pub sticky: bool,
    /// Where the choice's text came from, when known.
    pub source: Option<Location>,
}

/// A runtime fault: what the engine said, and where it happened.
///
/// ink's own runtime names the site — `RUNTIME ERROR: 'story.ink' line 7:
/// Can not call use == operation on Int and List` — and an author handed
/// the message without one has to hunt the project for it. So this carries
/// the site too, resolved exactly the way a breakpoint stop's is
/// ([`current_line`]), off the debug info the play compile asks for.
///
/// `at` is `None` when nothing resolves: a fault raised before the story
/// ran (a bad "play from here" path), or a program built without debug
/// info.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub message: String,
    /// `(file, 1-based line)`, keyed the way [`PlayStop::at`] is.
    pub at: Option<(String, u32)>,
}

/// Why a command produced no steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayError {
    /// Nothing names the story's start: no `[project] entry`, and no lone
    /// or `main.*` file to stand in for one.
    NoEntry,
    /// The project has errors, so there is no program to run. Each entry
    /// is `code: message`; the Problems panel has the positions.
    Compile(Vec<String>),
    /// The compiler produced no story and no error — should not happen.
    NoStory,
    /// The bytecode failed to link.
    Link(String),
    /// A `Choose` with no session running.
    NotStarted,
    /// The runtime faulted. The session is dropped — a faulted VM has no
    /// state worth continuing from — but the state it died in is parked on
    /// the slot first, and answered to the next `Snapshot`.
    Runtime(Fault),
    /// The worker has no usable project.
    Unavailable,
}

impl std::fmt::Display for PlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoEntry => {
                f.write_str("nothing names the story's start — set `[project] entry` in brink.toml")
            }
            Self::Compile(errors) => {
                write!(f, "the story has {} error(s)", errors.len())
            }
            Self::NoStory => f.write_str("the compiler produced no story"),
            Self::Link(e) => write!(f, "link failed: {e}"),
            Self::NotStarted => f.write_str("no story is running"),
            Self::Runtime(Fault {
                message,
                at: Some((path, line)),
            }) => write!(f, "runtime error at {path}:{line}: {message}"),
            Self::Runtime(Fault { message, at: None }) => {
                write!(f, "runtime error: {message}")
            }
            Self::Unavailable => f.write_str("no project is open"),
        }
    }
}

/// What one command produced.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayOutcome {
    /// The steps up to the next yield point, in order. Empty on an error
    /// and on `Stop`.
    pub steps: Vec<PlayStep>,
    /// Runtime warnings drained after the run, already rendered.
    pub warnings: Vec<String>,
    pub error: Option<PlayError>,
    /// The session's state, on a [`PlayCommand::Snapshot`] and nowhere
    /// else. `None` means no story is running.
    pub state: Option<PlayState>,
    /// Where a debug command stopped, on the four debug commands and
    /// nowhere else.
    pub stop: Option<PlayStop>,
    /// Lines from `SetBreakpoints` that bound to nothing, so the studio
    /// can report them rather than leave a mark that will never hit.
    pub unbound: Vec<(String, u32)>,
}

/// Where a debug command came to rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayStop {
    /// `breakpoint` / `step` / `choices` / `terminal` / `watchpoint` /
    /// `awaiting external`, plus a breakpoint's own name.
    pub reason: String,
    /// The source position the flow is stopped ON, 1-based — what the
    /// editor marks. `None` at a terminal, or with no debug info.
    pub at: Option<(String, u32)>,
}

impl PlayOutcome {
    fn failed(error: PlayError) -> Self {
        Self {
            error: Some(error),
            ..Self::default()
        }
    }

    /// The worker has no usable project.
    #[must_use]
    pub fn unavailable() -> Self {
        Self::failed(PlayError::Unavailable)
    }

    /// Whether the last step is one nothing can continue from.
    #[must_use]
    pub fn is_over(&self) -> bool {
        matches!(
            self.steps.last(),
            Some(PlayStep::Done | PlayStep::End | PlayStep::Suspended)
        ) || self.error.is_some()
    }
}

/// The running story. Lives in the worker loop beside the session.
pub struct Play {
    story: Story<FastRng>,
    /// Held for `resolve_source_line` (arming a breakpoint) and
    /// `resolve_debug_position` (reporting where a stop landed) — the
    /// story owns its own copy, but neither is reachable through it.
    program: Arc<brink_runtime::Program>,
    breakpoints: BreakpointSet,
}

/// The worker's play slot: the running story, the marks that outlive it,
/// and what a fault left behind.
///
/// One value rather than three loose locals because the three are one
/// thing — a Start replaces the story, re-arms the marks against the
/// program it just compiled, and clears the last corpse, and keeping that
/// invariant in one place is what stops a stale fault outliving the run
/// that superseded it.
#[derive(Default)]
pub struct PlaySlot {
    play: Option<Play>,
    /// The breakpoint lines as the editor holds them, kept across
    /// sessions because the gutter marks are: a mark set before Play is
    /// pressed must be armed by the Start that follows.
    wanted: Vec<(String, u32)>,
    /// The state the last fault died in, answered to `Snapshot` while
    /// nothing is running. Cleared by the next Start or Stop.
    fault: Option<PlayState>,
}

impl PlaySlot {
    /// Drop a faulted session, keeping the state it died in.
    ///
    /// The drop is the old rule and stays: there is nothing worth
    /// continuing from. What changes is that the state is read off the
    /// story FIRST — dropping it unread is what left the State View
    /// saying "no story is running" over a transcript that had just
    /// faulted, with the values that explain the fault already gone.
    fn park(&mut self, state: Option<PlayState>) {
        self.play = None;
        self.fault = state;
    }
}

/// Run one command against the worker's play slot.
///
/// `entry` is the project's `[project] entry` as applied; `files` the
/// author's file keys, for the stand-in rule when there is none.
pub fn run(
    session: &mut IdeSession,
    entry: Option<&str>,
    files: &[String],
    slot: &mut PlaySlot,
    command: PlayCommand,
) -> PlayOutcome {
    match command {
        PlayCommand::Start { at } => {
            slot.play = None;
            slot.fault = None;
            match start(session, entry, files, at.as_deref()) {
                Ok(started) => {
                    let running = slot.play.insert(started);
                    // The breakpoints outlive the session, because the
                    // marks in the gutter do: they are armed against the
                    // program this Start just compiled, and a start with
                    // any of them armed runs on the DEBUG road, so the
                    // first one hits instead of the story running past it.
                    let unbound = arm(running, &slot.wanted);
                    let mut outcome = if slot.wanted.is_empty() {
                        advance(running)
                    } else {
                        debug_command(slot, DebugVerb::Continue)
                    };
                    outcome.unbound = unbound;
                    if outcome.error.is_some() {
                        slot.park(outcome.state.clone());
                    }
                    outcome
                }
                Err(e) => PlayOutcome::failed(e),
            }
        }
        // A fault answers here too, for as long as nothing has superseded
        // it: the panel that asks this is the one an author opens to find
        // out WHY the story died.
        PlayCommand::Snapshot => PlayOutcome {
            state: slot
                .play
                .as_ref()
                .map(|running| snapshot(&running.story))
                .or_else(|| slot.fault.clone()),
            ..PlayOutcome::default()
        },
        PlayCommand::Choose(index) => {
            let Some(running) = slot.play.as_mut() else {
                return PlayOutcome::failed(PlayError::NotStarted);
            };
            if let Err(e) = running.story.choose(index) {
                let outcome = faulted(running, e.to_string());
                slot.park(outcome.state.clone());
                return outcome;
            }
            // With breakpoints armed, taking a choice continues on the
            // DEBUG road: the production `continue_maximally` knows
            // nothing about them, so a breakpoint past a choice could
            // never hit and the mark in the gutter would be a lie.
            if running.breakpoints.iter().next().is_some() {
                return debug_command(slot, DebugVerb::Continue);
            }
            let outcome = advance(running);
            if outcome.error.is_some() {
                slot.park(outcome.state.clone());
            }
            outcome
        }
        PlayCommand::Stop => {
            slot.play = None;
            slot.fault = None;
            PlayOutcome::default()
        }
        PlayCommand::SetBreakpoints(lines) => {
            // Kept whether or not a story is running: the marks are the
            // editor's, and the next Start arms them. A set with nothing
            // running is not an error — it is the ordinary case of
            // marking a line before pressing Play.
            slot.wanted = lines;
            let wanted = &slot.wanted;
            let unbound = slot.play.as_mut().map(|running| arm(running, wanted));
            PlayOutcome {
                unbound: unbound.unwrap_or_default(),
                ..PlayOutcome::default()
            }
        }
        PlayCommand::Continue => debug_command(slot, DebugVerb::Continue),
        PlayCommand::StepLine => debug_command(slot, DebugVerb::StepLine),
        PlayCommand::StepInstruction => debug_command(slot, DebugVerb::StepInstruction),
    }
}

/// The outcome a runtime fault produces: the engine's message, the site
/// the debug info resolves for it, and the state the story died in.
///
/// Read off the story while it is still alive — every caller drops it
/// immediately after (see [`PlaySlot::park`]).
///
/// ## The turn's own output is NOT here, and cannot be
///
/// ink delivers the lines a faulting turn had already produced and then
/// reports the fault; brink delivers nothing. That is not this function's
/// choice: `continue_maximally` returns `Result<Vec<Step>, _>` and
/// `drive_to_terminal`'s `?` drops the steps it had accumulated, so the
/// output is gone before any of it reaches here — and gone from the
/// buffer too, having already been taken out of it. Recovering it needs
/// the runtime to hand back partial output with the error, which is a
/// change to an API `bevy-brink`, the CLI and the wasm bindings all share
/// (#3587).
/// Pinned by `a_runtime_fault_names_its_site_and_leaves_its_state_readable`.
fn faulted(play: &Play, message: String) -> PlayOutcome {
    let fault = Fault {
        message,
        at: current_line(play),
    };
    let mut state = snapshot(&play.story);
    state.faulted = Some(fault.clone());
    PlayOutcome {
        error: Some(PlayError::Runtime(fault)),
        state: Some(state),
        ..PlayOutcome::default()
    }
}

/// Which debug verb a command runs. The three share everything but the
/// one call, so they share the body rather than three copies of the
/// drain/convert/stop bookkeeping.
#[derive(Clone, Copy)]
enum DebugVerb {
    Continue,
    StepLine,
    StepInstruction,
}

/// Replace the breakpoint set, returning the lines that bound to nothing.
///
/// A line binds to a program address or it does not: a comment, a blank,
/// or code that folded away has none. Reported rather than armed —
/// "a breakpoint that can never hit is worse than none".
fn arm(play: &mut Play, lines: &[(String, u32)]) -> Vec<(String, u32)> {
    play.breakpoints = BreakpointSet::new();
    let mut unbound = Vec::new();
    for (file, line) in lines {
        // The studio counts lines from 1, as every editor does; the
        // engine counts from 0. This is the one edge that converts.
        match play
            .program
            .resolve_source_line(file, line.saturating_sub(1))
        {
            Some(position) => {
                play.breakpoints.insert(
                    position.container_idx,
                    position.offset,
                    format!("{file}:{line}"),
                );
            }
            None => unbound.push((file.clone(), *line)),
        }
    }
    unbound
}

fn debug_command(slot: &mut PlaySlot, verb: DebugVerb) -> PlayOutcome {
    let Some(running) = slot.play.as_mut() else {
        return PlayOutcome::failed(PlayError::NotStarted);
    };
    // Before AND after, so the two drive roads share one delivery stream:
    // the production path runs ahead of what it has handed out, and a line
    // it already completed must surface here exactly once (W5/#3298).
    let mut lines = running.story.debug_drain_buffered_lines();
    let result = match verb {
        DebugVerb::Continue => running
            .story
            .debug_run(&running.breakpoints, DEFAULT_DEBUG_BUDGET),
        DebugVerb::StepLine => running.story.debug_step_line(
            StepMode::Into,
            &running.breakpoints,
            DEFAULT_DEBUG_BUDGET,
        ),
        DebugVerb::StepInstruction => {
            running
                .story
                .debug_step(StepMode::Into, &running.breakpoints, DEFAULT_DEBUG_BUDGET)
        }
    };
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(e) => {
            let mut outcome = faulted(running, format!("{e:?}"));
            // The lines drained BEFORE the verb ran were already
            // completed by an earlier call; a fault must not swallow
            // those either. They come first — they are older.
            let mut earlier: Vec<PlayStep> = lines
                .into_iter()
                .map(|(text, tags, source)| PlayStep::Line {
                    text,
                    tags,
                    source: location(source),
                })
                .collect();
            earlier.append(&mut outcome.steps);
            outcome.steps = earlier;
            slot.park(outcome.state.clone());
            return outcome;
        }
    };
    let Some(running) = slot.play.as_mut() else {
        return PlayOutcome::failed(PlayError::NotStarted);
    };
    lines.extend(running.story.debug_drain_buffered_lines());
    let mut steps: Vec<PlayStep> = lines
        .into_iter()
        .map(|(text, tags, source)| PlayStep::Line {
            text,
            tags,
            source: location(source),
        })
        .collect();
    // A stop at a choice point offers choices; the transcript needs them
    // the same way a production advance delivers them.
    if matches!(outcome.reason, DebugStopReason::Choices) {
        let snap = running.story.debug_snapshot();
        steps.push(PlayStep::Choices(
            snap.pending_choices
                .into_iter()
                .enumerate()
                .map(|(i, c)| PlayChoice {
                    text: c.text,
                    index: i,
                    tags: Vec::new(),
                    sticky: false,
                    source: None,
                })
                .collect(),
        ));
    }
    PlayOutcome {
        steps,
        warnings: running
            .story
            .take_runtime_warnings()
            .iter()
            .map(ToString::to_string)
            .collect(),
        stop: Some(PlayStop {
            reason: describe(&outcome.reason),
            at: current_line(running),
        }),
        ..PlayOutcome::default()
    }
}

/// The file and 1-based line the flow is stopped on.
fn current_line(play: &Play) -> Option<(String, u32)> {
    let position = play.story.debug_snapshot().position?;
    let loc = play.program.resolve_debug_position(position)?;
    let file = loc.file?;
    let line0 = play.program.line_at(&file, loc.range_start)?;
    Some((file, line0 + 1))
}

fn describe(reason: &DebugStopReason) -> String {
    match reason {
        DebugStopReason::Breakpoint { name, .. } => format!("breakpoint {name}"),
        DebugStopReason::Watchpoint { global_idx } => format!("watchpoint on global {global_idx}"),
        DebugStopReason::Choices => "a choice point".to_owned(),
        DebugStopReason::Step => "step".to_owned(),
        other => format!("{other:?}").to_lowercase(),
    }
}

/// The file to compile from: the applied entry, else the project's one
/// file, else a root-level `main.*`.
pub(crate) fn entry_file<'a>(entry: Option<&'a str>, files: &'a [String]) -> Option<&'a str> {
    if let Some(entry) = entry {
        return Some(entry);
    }
    if let [only] = files {
        return Some(only.as_str());
    }
    files
        .iter()
        .find(|f| matches!(f.as_str(), "main.ink" | "main.brink"))
        .map(String::as_str)
}

fn start(
    session: &mut IdeSession,
    entry: Option<&str>,
    files: &[String],
    at: Option<&str>,
) -> Result<Play, PlayError> {
    let entry = entry_file(entry, files).ok_or(PlayError::NoEntry)?;
    // The debugger needs the `DebugInfo` section: without it a breakpoint
    // binds to nothing and a stop has no source position. The session
    // wants it on (`IdeSession::emit_debug_info`, default ON since
    // 2026-08-29) but the db's own options carry `AnalysisOptions`'s
    // release default, and this compile reads those — so it is asked for
    // here, at the one call that runs a story.
    let options = brink_analyzer::AnalysisOptions {
        emit_debug_info: session.emit_debug_info(),
        ..session.db().analysis_options().clone()
    };
    let product = session
        .compile(entry, &options)
        .map_err(|e| PlayError::Compile(vec![e.to_string()]))?;
    if !product.errors.is_empty() {
        return Err(PlayError::Compile(
            product
                .errors
                .iter()
                .map(|d| format!("{}: {}", d.code.as_str(), d.message))
                .collect(),
        ));
    }
    let data = product.story.ok_or(PlayError::NoStory)?;
    let (program, line_tables) =
        brink_runtime::link(&data).map_err(|e| PlayError::Link(e.to_string()))?;
    let program = Arc::new(program);
    let mut story = Story::<FastRng>::new(Arc::clone(&program), line_tables);
    if let Some(path) = at {
        // "Play from here" is a development affordance: a private stitch is
        // exactly the kind of place an author wants to jump into.
        story.set_visibility_enforcement(false);
        story.choose_path_string(path).map_err(|e| {
            PlayError::Runtime(Fault {
                message: e.to_string(),
                // Nothing has run, so there is no position to resolve.
                at: None,
            })
        })?;
    }
    Ok(Play {
        story,
        program,
        breakpoints: BreakpointSet::new(),
    })
}

/// The running story's state, flattened for the UI.
///
/// The runtime assembles the snapshot; this only drops what the panel has
/// no use for (the `DefinitionId`-keyed visit ids, the per-frame bytecode
/// positions) and turns the rest into plain data, since nothing of the
/// engine crosses to the main thread.
fn snapshot(story: &Story<FastRng>) -> PlayState {
    let snap = story.debug_snapshot();
    PlayState {
        status: snap.status.to_owned(),
        location: snap.current_location,
        turn: snap.turn_index,
        globals: snap
            .globals
            .into_iter()
            .map(|g| (g.name, g.value))
            .collect(),
        call_stack: snap
            .call_stack
            .into_iter()
            .map(|f| (f.kind.to_owned(), f.location))
            .collect(),
        visits: snap
            .visit_counts
            .into_iter()
            .map(|v| (v.path, v.count))
            .collect(),
        choices: snap.pending_choices.into_iter().map(|c| c.text).collect(),
        rng: (snap.rng.seed, snap.rng.previous),
        position: snap.position.map(|p| (p.container_idx, p.offset)),
        // A live read. `faulted` is stamped on by [`faulted`] alone.
        faulted: None,
    }
}

/// Run to the next yield point.
fn advance(play: &mut Play) -> PlayOutcome {
    let mut outcome = match play.story.continue_maximally() {
        Ok(steps) => PlayOutcome {
            steps: steps.into_iter().map(convert).collect(),
            ..PlayOutcome::default()
        },
        // The site and the state are read here, while the story is still
        // alive; the caller drops it immediately after.
        Err(e) => faulted(play, e.to_string()),
    };
    outcome.warnings = play
        .story
        .take_runtime_warnings()
        .iter()
        .map(ToString::to_string)
        .collect();
    outcome
}

fn location(source: Option<brink_format::SourceLocation>) -> Option<Location> {
    source.map(|s| Location {
        path: s.file,
        start: s.range_start,
        end: s.range_end,
    })
}

fn convert(step: Step) -> PlayStep {
    match step {
        Step::Line(line) => PlayStep::Line {
            text: line.text,
            tags: line.tags,
            source: location(line.source),
        },
        Step::Choices(choices) => PlayStep::Choices(
            choices
                .into_iter()
                .map(|c| PlayChoice {
                    text: c.text,
                    index: c.index,
                    tags: c.tags,
                    sticky: c.sticky,
                    source: location(c.source),
                })
                .collect(),
        ),
        Step::Done => PlayStep::Done,
        Step::End => PlayStep::End,
        Step::Suspended => PlayStep::Suspended,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A driver that carries the play slot the worker owns, so a test
    /// reads as a sequence of commands rather than as plumbing.
    struct Driver {
        session: IdeSession,
        files: Vec<String>,
        slot: PlaySlot,
    }

    impl Driver {
        fn new(source: &str) -> Self {
            let mut session = IdeSession::new();
            session.update_source("main.ink", source.to_owned());
            Self {
                session,
                files: vec!["main.ink".to_owned()],
                slot: PlaySlot::default(),
            }
        }

        /// A driver over no project at all.
        fn empty() -> Self {
            Self {
                session: IdeSession::new(),
                files: Vec::new(),
                slot: PlaySlot::default(),
            }
        }

        fn go(&mut self, command: PlayCommand) -> PlayOutcome {
            let entry = (!self.files.is_empty()).then_some("main.ink");
            run(
                &mut self.session,
                entry,
                &self.files,
                &mut self.slot,
                command,
            )
        }
    }

    #[test]
    fn a_snapshot_of_nothing_running_is_a_state_of_none_not_an_error() {
        // The State View has something to say about "no session" — it says
        // so — and an error would make the panel show a failure instead.
        let outcome = Driver::empty().go(PlayCommand::Snapshot);
        assert!(outcome.state.is_none());
        assert!(outcome.error.is_none(), "not running is not a failure");
        assert!(outcome.steps.is_empty(), "a snapshot advances nothing");
    }

    #[test]
    fn a_snapshot_reads_the_running_story_without_advancing_it() {
        let mut d = Driver::new(
            "VAR lamps = 2\n-> shore\n=== shore ===\nThe tide was out.\n* [Go] -> END\n",
        );
        let started = d.go(PlayCommand::Start { at: None });
        assert!(started.error.is_none(), "{:?}", started.error);

        let first = d.go(PlayCommand::Snapshot);
        let state = first.state.expect("a story is running");
        assert_eq!(state.status, "waiting_for_choice");
        assert_eq!(state.location.as_deref(), Some("shore"));
        assert_eq!(state.turn, 1);
        assert_eq!(
            state.globals,
            vec![("lamps".to_owned(), "2".to_owned())],
            "globals come through with their values"
        );
        assert_eq!(state.choices, vec!["Go".to_owned()]);
        assert!(!state.call_stack.is_empty(), "a running story has a stack");

        // Reading twice reads the same: a snapshot must not be a step.
        let again = d.go(PlayCommand::Snapshot);
        assert_eq!(again.state.as_ref().map(|s| s.turn), Some(1));
        assert_eq!(again.state.map(|s| s.choices), Some(state.choices));
    }

    #[test]
    fn a_breakpoint_set_before_play_is_armed_by_the_start_that_follows() {
        // Marking a line and pressing Play is the ordinary way to reach a
        // breakpoint, so the set outlives any one session: with nothing
        // running this is not an error, and Start arms what it holds.
        let mut d =
            Driver::new("-> shore\n=== shore ===\nThe tide was out.\nThe lamp was lit.\n-> END\n");
        let set = d.go(PlayCommand::SetBreakpoints(vec![(
            "main.ink".to_owned(),
            4,
        )]));
        assert!(set.error.is_none(), "marking a line needs no session");
        assert!(set.unbound.is_empty(), "nothing is bound yet either");

        let started = d.go(PlayCommand::Start { at: None });
        let stop = started
            .stop
            .expect("a start with breakpoints is a debug run");
        assert!(stop.reason.starts_with("breakpoint"), "{stop:?}");
        assert_eq!(stop.at, Some(("main.ink".to_owned(), 4)));
        // Nothing has been delivered yet, and that is the engine's own
        // rule rather than a gap: a line completes only once the
        // following non-whitespace output begins, because glue may still
        // legally join onto it. Continuing past the breakpoint is what
        // commits it, and then both lines arrive.
        assert!(started.steps.is_empty(), "{:?}", started.steps);
        let on = d.go(PlayCommand::Continue);
        let texts: Vec<&str> = on
            .steps
            .iter()
            .filter_map(|s| match s {
                PlayStep::Line { text, .. } => Some(text.trim()),
                _ => None,
            })
            .filter(|t| !t.is_empty())
            .collect();
        assert_eq!(
            texts,
            ["The tide was out.", "The lamp was lit."],
            "{:?}",
            on.steps
        );
    }

    #[test]
    fn a_line_with_no_code_is_reported_rather_than_armed() {
        // "A breakpoint that can never hit is worse than none": a comment
        // compiles to nothing, so it is named back rather than marked.
        let mut d = Driver::new("-> shore\n=== shore ===\n// a note\nThe tide.\n-> END\n");
        let _ = d.go(PlayCommand::SetBreakpoints(vec![(
            "main.ink".to_owned(),
            3,
        )]));
        let started = d.go(PlayCommand::Start { at: None });
        assert_eq!(started.unbound, vec![("main.ink".to_owned(), 3)]);
    }

    #[test]
    fn a_breakpoint_past_a_choice_still_hits() {
        // Taking a choice continues on the debug road when anything is
        // armed — the production continue knows nothing about
        // breakpoints, so this one could never hit.
        let mut d = Driver::new(
            "-> shore\n=== shore ===\nThe tide was out.\n* [Go] -> after\n\
             === after ===\nThe lamp was lit.\n-> END\n",
        );
        let _ = d.go(PlayCommand::SetBreakpoints(vec![(
            "main.ink".to_owned(),
            6,
        )]));
        let started = d.go(PlayCommand::Start { at: None });
        assert!(started.stop.is_some(), "the start ran on the debug road");
        let out = d.go(PlayCommand::Choose(0));
        let stop = out.stop.expect("the choice continued on the debug road");
        assert!(stop.reason.starts_with("breakpoint"), "{stop:?}");
        assert_eq!(stop.at, Some(("main.ink".to_owned(), 6)));
    }

    #[test]
    fn stepping_says_where_it_landed() {
        let mut d = Driver::new("-> shore\n=== shore ===\nOne.\nTwo.\nThree.\n-> END\n");
        // Stop on the first line, so there is somewhere to step FROM.
        let _ = d.go(PlayCommand::SetBreakpoints(vec![(
            "main.ink".to_owned(),
            3,
        )]));
        let started = d.go(PlayCommand::Start { at: None });
        assert_eq!(
            started.stop.and_then(|s| s.at),
            Some(("main.ink".to_owned(), 3))
        );
        let stepped = d.go(PlayCommand::StepLine);
        let stop = stepped.stop.expect("a stop reason");
        assert_eq!(
            stop.at,
            Some(("main.ink".to_owned(), 4)),
            "one source line on: {stop:?}"
        );
        // An instruction step is the other verb, not a wrapper: it moves
        // within the line it is on.
        let inner = d.go(PlayCommand::StepInstruction);
        assert!(inner.stop.is_some(), "{inner:?}");
    }

    #[test]
    fn a_debug_command_with_nothing_running_is_not_started_rather_than_a_panic() {
        for command in [
            PlayCommand::Continue,
            PlayCommand::StepLine,
            PlayCommand::StepInstruction,
        ] {
            let out = Driver::empty().go(command);
            assert!(matches!(out.error, Some(PlayError::NotStarted)), "{out:?}");
        }
    }

    #[test]
    fn entry_falls_back_to_the_lone_file_then_main() {
        assert_eq!(entry_file(Some("s.ink"), &["a.ink".into()]), Some("s.ink"));
        assert_eq!(entry_file(None, &["a.ink".into()]), Some("a.ink"));
        let two = ["a.ink".to_owned(), "main.ink".to_owned()];
        assert_eq!(entry_file(None, &two), Some("main.ink"));
        let none = ["a.ink".to_owned(), "b.ink".to_owned()];
        assert_eq!(entry_file(None, &none), None);
    }
}
