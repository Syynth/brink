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
    DebugChoice, DebugFrame, DebugGlobal, DebugPosition, DebugRng, DebugSnapshot,
    DebugSourceLocation, DebugVisit,
};
#[cfg(feature = "debug-hooks")]
pub use debug_control::{
    Breakpoint, BreakpointId, BreakpointSet, DEFAULT_DEBUG_BUDGET, DebugRunOutcome,
    DebugStopReason, StepMode, WatchHit, WatchpointObserver,
};
pub use error::{RanOutOfContentCause, RuntimeError};
pub use external_policy::{EvalContext, ExternalsReport, KindTieredHandler, PolicyKind};
pub use iter::ValueIter;
pub use linker::link;
pub use locale::{LocaleMode, apply_locale};
pub use output::{Fragment, OutputPart};
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
pub use story::{
    BlockId, Choice, DriveOutcome, Element, ExecMode, ExternalFnHandler, ExternalResult,
    FallbackHandler, FlowInstance, FunctionEval, OutputLine, Stats, Step, StepOutcome, Story,
    StorySnapshot, StoryStatus,
};
pub use world::{
    CommitError, ContextView, FlowLocal, FrameStartView, Mode, PolicyError, ResolvedPolicy, Scope,
    World, WorldPolicy, commit,
};
