//! This crate's integration tests, compiled as one test binary (`--test
//! integration`) instead of one per file: each file under `tests/` used to
//! link its own copy of the crate's whole dependency graph. See
//! docs/decision-log.md, "Integration tests consolidate into one test
//! binary per crate". A file here that this list does not declare never
//! compiles; `scripts/check-integration-tests.mjs` fails on one.

mod benchmark_fixtures_compile;
mod block_id;
mod corpus;
mod current_path;
mod element;
mod flow_instance_visibility;
mod function_value_param_binding;
mod issue_2894_block_postfix;
mod issue_2903_index_postfix;
mod issue_3185_locals_debug_snapshot;
mod law_iterate;
mod law_transcript_roundtrip;
mod line_source;
mod locale;
mod renames;
mod run_story;
mod session;
mod shared_flow;
mod speculation;
mod terminal_classification;
mod variant_ops;
mod visibility;
