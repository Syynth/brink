//! This crate's integration tests, compiled as one test binary (`--test
//! integration`) instead of one per file: each file under `tests/` used to
//! link its own copy of the crate's whole dependency graph. See
//! docs/decision-log.md, "Integration tests consolidate into one test
//! binary per crate". A file here that this list does not declare never
//! compiles; `scripts/check-integration-tests.mjs` fails on one.

mod assemble_analyzer_tables;
mod coalesce_types;
mod issue_1876_native_fn_value_typing;
mod issue_2083_call_site_local_shadows_global;
mod issue_2156_divert_arity;
mod law_unify_order_independence;
mod proptest_resolve;
mod reserved_names_cross_crate;
mod struct_shapes_public_api;
mod symbol_service;
mod ufcs_resolution;
