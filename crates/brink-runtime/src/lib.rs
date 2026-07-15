//! Runtime/VM for executing compiled ink stories.
//!
//! The runtime takes a [`StoryData`](brink_format::StoryData) from the compiler,
//! links it into an immutable [`Program`], and executes it via [`Story`].
//!
//! ```ignore
//! use std::sync::Arc;
//!
//! let (program, line_tables) = brink_runtime::link(&story_data)?;
//! let mut story = brink_runtime::Story::new(Arc::new(program), line_tables);
//! loop {
//!     match story.continue_single()? {
//!         Line::Text { text, .. } => print!("{text}"),
//!         Line::Choices { text, choices, .. } => {
//!             print!("{text}");
//!             // pick a choice...
//!             story.choose(0)?;
//!         }
//!         Line::End { text, .. } => {
//!             print!("{text}");
//!             break;
//!         }
//!     }
//! }
//! ```
//!
//! `no_std` + `alloc`: this crate builds without the standard library when
//! the default `std` feature is disabled (see `docs/no-std-portability.md`).
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod collection_ops;
mod collections;
mod conversion_ops;
mod debug;
mod error;
mod external_policy;
mod linker;
mod list_ops;
mod locale;
mod output;
mod program;
mod proj_ops;
mod record_ops;
mod replay;
pub mod rng;
mod save;
mod session;
mod speculation;
mod state;
mod story;
pub mod transcript;
mod value_ops;
mod vm;
mod world;

pub use brink_format::{LoadReport, SAVE_FORMAT_VERSION, SaveState, VisitEntry};
pub use debug::{DebugChoice, DebugFrame, DebugGlobal, DebugRng, DebugSnapshot, DebugVisit};
pub use error::RuntimeError;
pub use external_policy::{EvalContext, ExternalsReport, KindTieredHandler, PolicyKind};
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
    Choice, DriveOutcome, ExternalFnHandler, ExternalResult, FallbackHandler, FlowInstance,
    FunctionEval, Line, Stats, StepOutcome, Story, StorySnapshot, StoryStatus,
};
pub use world::{
    CommitError, ContextView, FlowLocal, Mode, PolicyError, ResolvedPolicy, Scope, World,
    WorldPolicy, commit,
};
