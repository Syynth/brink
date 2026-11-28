//! This crate's integration tests, compiled as one test binary (`--test
//! integration`) instead of one per file: each file under `tests/` used to
//! link its own copy of the crate's whole dependency graph. See
//! docs/decision-log.md, "Integration tests consolidate into one test
//! binary per crate". A file here that this list does not declare never
//! compiles; `scripts/check-integration-tests.mjs` fails on one.

mod dialect_conformance;
mod issue_2318_std_collision_survives_config_document;
mod line_context_snapshots;
mod live_typing_db_divergence;
mod native_module_identity;
mod native_rule_selection;
mod session_option_propagation;
