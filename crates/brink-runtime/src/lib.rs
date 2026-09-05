//! Runtime/VM for executing compiled ink stories.
//!
//! The runtime takes a [`StoryData`](brink_format::StoryData) from the compiler,
//! links it into an immutable [`Program`], and executes it via [`Story`].
//!
//! ```no_run
//! # fn example(story_data: &brink_format::StoryData) -> Result<(), brink_runtime::RuntimeError> {
//! use std::sync::Arc;
//! use brink_runtime::Step;
//!
//! let (program, line_tables) = brink_runtime::link(story_data)?;
//! let mut story: brink_runtime::Story = brink_runtime::Story::new(Arc::new(program), line_tables);
//! loop {
//!     match story.continue_single()? {
//!         Step::Line(line) => print!("{}", line.text),
//!         Step::Done => {}
//!         Step::Choices(choices) => {
//!             let _ = choices;
//!             // pick a choice...
//!             story.choose(0)?;
//!         }
//!         Step::End => break,
//!         Step::Suspended => break,
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! `no_std` + `alloc`: this crate builds without the standard library when
//! the default `std` feature is disabled (see `docs/no-std-portability.md`).
#![cfg_attr(not(feature = "std"), no_std)]
// `Option::ok_or(RuntimeError::…)` builds the error eagerly and, on the
// `Some` path, drops it — and `RuntimeError`'s drop glue is an out-of-line
// call (the enum carries `String` payloads), paid even for a unit variant.
// The VM's per-step path does this two to three times per step: measured
// at 6.5% of all instructions on `crucible-8` (callgrind,
// `drop_glue::<RuntimeError>` called 2.3M times over 933K steps). Every
// such site uses `ok_or_else` so that no error value exists unless the
// `None` arm is taken. Clippy's heuristic that an enum constructor is free
// to build is wrong for this type, hence the crate-wide expectation; it
// goes stale (and CI says so) if the last `ok_or_else(|| RuntimeError::…)`
// disappears.
#![expect(
    clippy::unnecessary_lazy_evaluations,
    reason = "RuntimeError's drop glue is a real out-of-line cost on the Some path; see the note above"
)]

extern crate alloc;

#[cfg(feature = "bench-counters")]
pub mod bench_counters;
mod collection_ops;
mod collections;
mod conversion_ops;
mod debug;
#[cfg(feature = "debug-hooks")]
pub mod debug_control;
#[cfg(feature = "effect-trace")]
pub mod effect_trace;
mod error;
mod external_policy;
mod iter;
mod linker;
mod list_ops;
mod locale;
mod output;
mod program;
mod proj_ops;
mod rand_ops;
mod range_ops;
mod record_ops;
mod replay;
pub mod rng;
mod save;
mod session;
mod speculation;
mod state;
mod story;
mod string_ops;
mod tower_ops;
pub mod transcript;
mod value_ops;
mod vm;
mod world;

pub use brink_format::{LoadReport, SAVE_FORMAT_VERSION, SaveState, VisitEntry};
pub use debug::{
    DebugChoice, DebugFrame, DebugGlobal, DebugLocal, DebugPosition, DebugRng, DebugSnapshot,
    DebugSourceLocation, DebugValue, DebugVisit,
};
/// Scripted debug sessions (#3247/#3248): the shared verb set the test
/// harness, the CLI debugger and the studio all drive, so there is one
/// definition of "step over" rather than three.
#[cfg(feature = "debug-hooks")]
pub mod debug_session;

#[cfg(feature = "debug-hooks")]
pub use debug_control::{
    Breakpoint, BreakpointId, BreakpointSet, DEFAULT_DEBUG_BUDGET, DebugRunOutcome,
    DebugStopReason, StepMode, WatchHit, WatchpointObserver,
};
pub use error::{RUNTIME_WARNING_CAP, RanOutOfContentCause, RuntimeError, RuntimeWarning};
pub use external_policy::{EvalContext, ExternalsReport, KindTieredHandler, PolicyKind};
pub use iter::ValueIter;
pub use linker::link;
pub use locale::{LocaleMode, apply_locale};
pub use output::{Fragment, FragmentRef, Fragments, OutputPart};
pub use program::{ListMember, Program};
pub use replay::{
    RECORDING_CAP, RecordedExternal, RecordingHandler, ReplayHandler, ReplayMode, ReplayRecorder,
};
pub use rng::{DotNetRng, FastRng, StoryRng};
pub use save::{load_state, save_state};
pub use session::{
    DivergenceFound, EventKind, ExternalReplayMode, FailReason, JournalEvent, ListDelta,
    ReplayOutcome, ReplayWarning, SESSION_JOURNAL_CAP, SESSION_JOURNAL_VERSION, SessionError,
    SessionJournal, SnapshotFrame, SnapshotList, SnapshotStatus, StateDiff, StateSnapshot,
    StorySession, diff,
};
pub use speculation::{Budget, Speculation, SpeculationStep};
pub use state::{ContextAccess, ObservedContext, WriteObserver};
#[cfg(feature = "debug-hooks")]
pub use story::DrainedLine;
pub use story::{
    BlockId, Choice, DriveOutcome, Element, ExecMode, ExternalFnHandler, ExternalResult,
    FallbackHandler, FlowInstance, FunctionEval, OutputLine, Stats, Step, StepOutcome, Story,
    StorySnapshot, StoryStatus,
};
pub use world::{
    CommitError, ContextView, FlowLocal, FrameStartView, Mode, PolicyError, ResolvedPolicy, Scope,
    World, WorldPolicy, commit,
};
