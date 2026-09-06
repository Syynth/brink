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
fn entry_file<'a>(entry: Option<&'a str>, files: &'a [String]) -> Option<&'a str> {
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
    fn entry_falls_back_to_the_lone_file_then_main() {
        assert_eq!(entry_file(Some("s.ink"), &["a.ink".into()]), Some("s.ink"));
        assert_eq!(entry_file(None, &["a.ink".into()]), Some("a.ink"));
        let two = ["a.ink".to_owned(), "main.ink".to_owned()];
        assert_eq!(entry_file(None, &two), Some("main.ink"));
        let none = ["a.ink".to_owned(), "b.ink".to_owned()];
        assert_eq!(entry_file(None, &none), None);
    }
}
