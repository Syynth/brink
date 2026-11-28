//! This crate's integration tests, compiled as one test binary (`--test
//! integration`) instead of one per file: each file under `tests/` used to
//! link its own copy of the crate's whole dependency graph. See
//! docs/decision-log.md, "Integration tests consolidate into one test
//! binary per crate". A file here that this list does not declare never
//! compiles; `scripts/check-integration-tests.mjs` fails on one.

// Shared helpers, declared once here (each user does `use crate::law_support;`).
mod law_support;

mod inkb;
mod inkl;
mod inkt_opcode_family_roundtrip;
mod law_cow_sharing;
mod law_deep_equality;
mod law_savestate_roundtrip;
mod proptest_inkb;
mod proptest_inkt;
mod ts_mirror_field_parity;
mod write_inkt;
