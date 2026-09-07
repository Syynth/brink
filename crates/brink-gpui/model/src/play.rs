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
use brink_runtime::{FastRng, Step, Story};

use crate::query::Location;

/// What the UI asks of the play session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayCommand {
    /// Compile and start. `at` is a knot or `knot.stitch` path to divert to
    /// before the first line — "Play from here". `None` plays from the
    /// entry.
    Start { at: Option<String> },
    /// Take the choice at `index` (a [`PlayChoice::index`]) and run on.
    Choose(usize),
    /// Drop the session.
    Stop,
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
    /// The runtime faulted. The session is dropped: a faulted VM has no
    /// state worth continuing from.
    Runtime(String),
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
            Self::Runtime(e) => write!(f, "runtime error: {e}"),
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
}

/// Run one command against the worker's play slot.
///
/// `entry` is the project's `[project] entry` as applied; `files` the
/// author's file keys, for the stand-in rule when there is none.
pub fn run(
    session: &mut IdeSession,
    entry: Option<&str>,
    files: &[String],
    play: &mut Option<Play>,
    command: PlayCommand,
) -> PlayOutcome {
    match command {
        PlayCommand::Start { at } => {
            *play = None;
            match start(session, entry, files, at.as_deref()) {
                Ok(started) => {
                    let outcome = advance(&mut *play.insert(started));
                    if outcome.error.is_some() {
                        *play = None;
                    }
                    outcome
                }
                Err(e) => PlayOutcome::failed(e),
            }
        }
        PlayCommand::Snapshot => PlayOutcome {
            state: play.as_ref().map(|running| snapshot(&running.story)),
            ..PlayOutcome::default()
        },
        PlayCommand::Choose(index) => {
            let Some(running) = play.as_mut() else {
                return PlayOutcome::failed(PlayError::NotStarted);
            };
            if let Err(e) = running.story.choose(index) {
                *play = None;
                return PlayOutcome::failed(PlayError::Runtime(e.to_string()));
            }
            let outcome = advance(running);
            if outcome.error.is_some() {
                *play = None;
            }
            outcome
        }
        PlayCommand::Stop => {
            *play = None;
            PlayOutcome::default()
        }
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
    let options = session.db().analysis_options().clone();
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
    let mut story = Story::<FastRng>::new(Arc::new(program), line_tables);
    if let Some(path) = at {
        // "Play from here" is a development affordance: a private stitch is
        // exactly the kind of place an author wants to jump into.
        story.set_visibility_enforcement(false);
        story
            .choose_path_string(path)
            .map_err(|e| PlayError::Runtime(e.to_string()))?;
    }
    Ok(Play { story })
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
    }
}

/// Run to the next yield point.
fn advance(play: &mut Play) -> PlayOutcome {
    let mut outcome = PlayOutcome::default();
    match play.story.continue_maximally() {
        Ok(steps) => {
            outcome.steps = steps.into_iter().map(convert).collect();
        }
        Err(e) => outcome.error = Some(PlayError::Runtime(e.to_string())),
    }
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

    #[test]
    fn a_snapshot_of_nothing_running_is_a_state_of_none_not_an_error() {
        // The State View has something to say about "no session" — it says
        // so — and an error would make the panel show a failure instead.
        let mut session = IdeSession::new();
        let mut play = None;
        let outcome = run(&mut session, None, &[], &mut play, PlayCommand::Snapshot);
        assert!(outcome.state.is_none());
        assert!(outcome.error.is_none(), "not running is not a failure");
        assert!(outcome.steps.is_empty(), "a snapshot advances nothing");
    }

    #[test]
    fn a_snapshot_reads_the_running_story_without_advancing_it() {
        let mut session = IdeSession::new();
        session.update_source(
            "main.ink",
            "VAR lamps = 2\n-> shore\n=== shore ===\nThe tide was out.\n* [Go] -> END\n".to_owned(),
        );
        let files = ["main.ink".to_owned()];
        let mut play = None;
        let started = run(
            &mut session,
            Some("main.ink"),
            &files,
            &mut play,
            PlayCommand::Start { at: None },
        );
        assert!(started.error.is_none(), "{:?}", started.error);

        let first = run(
            &mut session,
            Some("main.ink"),
            &files,
            &mut play,
            PlayCommand::Snapshot,
        );
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
        let again = run(
            &mut session,
            Some("main.ink"),
            &files,
            &mut play,
            PlayCommand::Snapshot,
        );
        assert_eq!(again.state.as_ref().map(|s| s.turn), Some(1));
        assert_eq!(again.state.map(|s| s.choices), Some(state.choices));
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
