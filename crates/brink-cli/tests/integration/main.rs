//! This crate's integration tests, compiled as one test binary (`--test
//! integration`) instead of one per file: each file under `tests/` used to
//! link its own copy of the crate's whole dependency graph. See
//! docs/decision-log.md, "Integration tests consolidate into one test
//! binary per crate". A file here that this list does not declare never
//! compiles; `scripts/check-integration-tests.mjs` fails on one.

mod debug_cli;
mod debug_info_flag_cli;
mod directory_prune_escape_hatch_cli;
mod fatal_diagnostics_rendered_cli;
mod fix_cli;
mod ide_cli;
mod include_above_root_cli;
mod native_play_cli;
mod project_config_cli;
mod unrelated_malformed_file_cli;
