//! Episode-based behavioral testing for brink ink runtime.
//!
//! Records full execution traces (episodes) including text output, choices,
//! tags, state mutations, and external function calls. Supports branch
//! exploration via DFS with `Story` cloning and structural diffing of episodes.

pub mod corpus;
pub mod diff;
pub mod episode;
pub mod explorer;
pub mod oracle;
pub mod runner;
pub mod snapshot_fmt;

pub use diff::{EpisodeDiff, StepDiff, diff};
pub use episode::*;
pub use explorer::{ExploreConfig, explore};
pub use oracle::{OracleDiff, OracleEpisode, diff_oracle, load_oracle_episodes};
pub use runner::{RunConfig, record, record_from_ink_json, run_text, run_text_from_ink_json};
