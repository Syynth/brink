//! This crate's integration tests, compiled as one test binary (`--test
//! integration`) instead of one per file: each file under `tests/` used to
//! link its own copy of the crate's whole dependency graph. See
//! docs/decision-log.md, "Integration tests consolidate into one test
//! binary per crate". A file here that this list does not declare never
//! compiles; `scripts/check-integration-tests.mjs` fails on one.

// The bench's own scenario model, compiled in so both scenario tests exercise
// the real generator and frame-loop code (each user does `use crate::model;`).
#[path = "../../benches/scenario/model.rs"]
mod model;

mod manifest_field_convergence;
mod scenario_baseline_tripwire;
mod scenario_bench_model;
