//! This crate's integration tests, compiled as one test binary (`--test
//! integration`) instead of one per file: each file under `tests/` used to
//! link its own copy of the crate's whole dependency graph. See
//! docs/decision-log.md, "Integration tests consolidate into one test
//! binary per crate". A file here that this list does not declare never
//! compiles; `scripts/check-integration-tests.mjs` fails on one.

// Shared helpers, declared once here (each user does `use crate::law_support;`).
#[expect(
    clippy::panic,
    reason = "`compile` and `run_to_completion` panic with the generated program attached, as \
              their users allowed crate-wide when each law file was its own binary"
)]
mod law_support;

mod addressing;
mod algorithms_corpus;
mod algorithms_ground_truth;
mod b03_admission_corpus;
mod b04_manifest_burn_in;
mod b3a_ufcs_e2e;
mod b5_construction_e2e;
mod block_local_ids;
mod book_fences;
mod book_function_values_examples;
mod book_path_projections_examples;
mod corpus_report;
mod db_memo_retention;
#[cfg(feature = "debug-hooks")]
mod debug_control_3186;
#[cfg(feature = "debug-hooks")]
mod debug_entry_hold_3679;
mod debug_hooks_production_path_unaffected_3186;
mod debug_position_3182;
#[cfg(feature = "debug-hooks")]
mod debug_seam_3223_3224_3226;
#[cfg(feature = "debug-hooks")]
mod debug_sessions;
mod debug_source_line_3261;
mod debug_source_resolve_3187;
mod debug_source_resolve_3246;
mod diagnostic_docs_fences;
mod diagnostic_docs_validation;
mod dump_episode;
mod editor_session_bench_typed_probe;
mod environment_parallel_gate;
mod f34_comparator_write_guard;
mod fg5_memory_ceilings;
mod field_mutator_take_rmw;
mod first_light;
mod fix_safe_obligations;
mod function_eval;
mod incremental_fuzz;
mod inkjs_sanction;
mod issue_2045_tag_escape_stripping;
mod issue_2092_scene_entered_extern;
mod issue_2197_std_mount_module_qualification;
mod issue_2229_synthesized_choice_segment;
mod jump_to_path;
mod law_cow_sharing_programs;
mod law_determinism_repetition;
mod law_rmw_equivalence;
mod law_strict_gradual_equivalence;
mod newline_diagnostic;
mod ns_a4_exec_mode;
mod ns_a5_range;
mod ns_a6_rand;
mod ns_a7_weighted_heap;
mod opt_corpus_fence;
mod opt_corpus_stats;
mod opt_negative_control;
mod oracle_snapshots;
mod param_binding_at_entry;
mod proptest_t1b;
mod proptest_t1c;
mod resolved_ref_range_contract;
mod screenplay_preset_std_module;
mod segmenter_corpus_agreement;
mod stamp_road_parity;
mod t1e_projections;
mod t2_effects_corpus;
mod take_rmw;
mod thread_frame_growth_3561;
mod tier1_brink;
mod tier1_native;
mod tier1_native_strict;
mod tier4_generated;
mod trace_corpus_selfcheck;
mod trace_mutation_study;
mod trace_oracle;
mod variant_flip;
mod variant_groups_e2e;
