//! Episode-based behavioral testing for brink ink runtime.
//!
//! Records full execution traces (episodes) including text output, choices,
//! tags, and state mutations. Supports branch exploration via DFS with
//! `Story` cloning and structural diffing of episodes.

pub mod corpus;
/// Scripted debug sessions (#3247) — needs the runtime's debug-hooks seam,
/// so it only exists in a build that enables it.
#[cfg(feature = "debug-hooks")]
pub mod debug_script;
pub mod diff;
pub mod episode;
pub mod explorer;
pub mod fence;
pub mod ground_truth;
pub mod oracle;
pub mod runner;
pub mod snapshot_fmt;
mod termination;

pub use diff::{EpisodeDiff, StepDiff, diff};
pub use episode::*;
pub use explorer::{ExploreConfig, explore};
pub use oracle::{OracleDiff, OracleEpisode, diff_oracle, load_oracle_episodes};
pub use runner::{RunConfig, record, run_text};
