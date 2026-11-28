//! This crate's integration tests, compiled as one test binary (`--test
//! integration`) instead of one per file: each file under `tests/` used to
//! link its own copy of the crate's whole dependency graph. See
//! docs/decision-log.md, "Integration tests consolidate into one test
//! binary per crate". A file here that this list does not declare never
//! compiles; `scripts/check-integration-tests.mjs` fails on one.

// Shared helpers, declared once here (each user does `use crate::analysis_fixture;`).
mod analysis_fixture;

mod b06_native_annotations;
mod b06_native_declarations;
mod b07_native_body;
mod b08_native_control_flow;
mod b08_native_wave_b_tail;
mod b09_native_accept_list;
mod b2_for_two_binding;
mod b5_native_construction;
mod branch_span_disjointness;
mod import_alias_resolution;
mod issue_1161_allow_annotation;
mod issue_1727_lambda_hir_minted_id;
mod issue_2215_lambda_stamping_inline_conditional_label;
mod issue_3183_lir_provenance;
mod issue_3184_debug_info;
mod issue_3185_locals_table;
mod issue_3365_empty_choice_diagnostic;
mod lir_lowering;
mod markup_wire_recognition;
mod native_lambdas;
mod native_pub_visibility;
mod native_use_dual_reading;
mod native_use_import_scope;
mod ng_d_native_array_literal;
mod prose_elements_are_loud_not_silent;
mod provenance_seam;
mod ufcs_auto_ref;
mod ufcs_field_call;
mod variant_enumeration;
