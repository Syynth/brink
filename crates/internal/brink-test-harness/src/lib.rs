//! Episode-based behavioral testing for brink ink runtime.
//!
//! Records full execution traces (episodes) including text output, choices,
//! tags, and state mutations. Supports branch exploration via DFS with
//! `Story` cloning and structural diffing of episodes.

pub mod corpus;
pub mod diff;
pub mod episode;
pub mod explorer;
pub mod fence;
pub mod fix;
pub mod ground_truth;
pub mod mutate;
pub mod oracle;
pub mod runner;
pub mod snapshot_fmt;
mod termination;
pub mod trace;

pub use diff::{EpisodeDiff, StepDiff, diff};
pub use episode::*;
pub use explorer::{ExploreConfig, explore};
pub use fix::{
    FixFixture, FixFixtureError, SafeFixConfig, SafeFixReport, SafeVerdict, assert_safe_fix,
    check_safe_fix, load_fix_fixture,
};
pub use oracle::{OracleDiff, OracleEpisode, diff_oracle, load_oracle_episodes};
pub use runner::{RunConfig, record, run_text};
pub use trace::{
    LineIdentityDiff, RunSpec, Trace, TraceConfig, TraceDiff, TraceError, differential,
    line_identity_diff, trace_diff, trace_diff_with,
};
