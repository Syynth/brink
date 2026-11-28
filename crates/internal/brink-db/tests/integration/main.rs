//! This crate's integration tests, compiled as one test binary (`--test
//! integration`) instead of one per file: each file under `tests/` used to
//! link its own copy of the crate's whole dependency graph. See
//! docs/decision-log.md, "Integration tests consolidate into one test
//! binary per crate". A file here that this list does not declare never
//! compiles; `scripts/check-integration-tests.mjs` fails on one.

mod fg1_dependency_edges;
mod fg2_scc_dependency_edges;
mod fg3_dependency_edges;
mod fg4a_dependency_edges;
mod fg4d_chunk_memos;
mod fg4e_prelude_decls;
mod issue_1680_lambda_effect_row_gap;
mod issue_1844_conventions_module_fence;
mod issue_1865_struct_shadows_builtin_type;
mod issue_1918_field_call_arg_check;
mod issue_1921_ufcs_external_arg_check;
mod issue_2083_fn_valued_const_global_call_site;
mod issue_2099_effects_family_dialect_gate;
mod issue_2111_conventions_projection;
mod issue_2114_harvest_index;
mod issue_2134_harvest_completion;
mod issue_2289_cross_file_claiming;
mod issue_2329_non_source_documents;
mod issue_3088_module_arbitration_diagnostics;
mod issue_460_shared_chunk_ctx;
mod issue_786_external_handle_check;
mod issue_805_external_scalar_inline_return_check;
mod issue_815_unreachable_file_narrowing;
mod lir_severity_partition;
mod local_signature;
mod native_project_grouping;
mod t1d_2b_handle_kind_inference;
mod t2_1_effect_rows;
mod t2_2_effects_assertions;
mod t2_3_effect_rows_emission;
mod tm3_strict;
