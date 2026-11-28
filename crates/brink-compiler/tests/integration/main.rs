//! This crate's integration tests, compiled as one test binary (`--test
//! integration`) instead of one per file: each file under `tests/` used to
//! link its own copy of the crate's whole dependency graph. See
//! docs/decision-log.md, "Integration tests consolidate into one test
//! binary per crate". A file here that this list does not declare never
//! compiles; `scripts/check-integration-tests.mjs` fails on one.

mod driver;
mod e0xx_diagnostics;
mod fs2_await;
mod golden_i078;
mod issue_2856_builtin_shadow;
mod issue_2872_e063_policy_audit;
mod issue_2960_choice_midline_comment;
mod issue_2976_alternative_midline_comment;
mod issue_3181_flattened_content_source_location;
mod issue_3213_source_location_golden;
mod issue_3354_temp_dominance;
mod issue_3363_builtin_argument_faults;
mod issue_3365_empty_choice_warning;
mod issue_3373_compat_deny;
mod issue_3383_nested_gather_divert;
mod issue_3386_inline_conditional_ids;
mod issue_3395_lift_order;
mod issue_3401_shared_sequence_state;
mod issue_3413_shuffle_once_exhausted;
mod issue_3480_blank_choice_warning;
mod issue_3507_glue_spring;
mod issue_3508_choice_text_whitespace;
mod issue_3510_empty_then_branch_else;
mod issue_3519_function_leading_newline;
mod issue_3522_function_end_glue;
mod issue_3523_branch_leading_newline;
mod issue_3525_call_in_slot_expression;
mod issue_3527_visible_choice_index;
mod issue_3528_temp_after_thread_spawn;
mod issue_3530_elseless_newline;
mod issue_3531_list_contains_empty;
mod issue_3532_list_origins;
mod issue_3533_blank_line_before_boundary;
mod issue_3534_tag_only_line;
mod issue_3535_glue_across_blank_line;
mod issue_3536_function_end_empty_value;
mod issue_3538_shuffle_unpicked_order;
mod issue_3556_function_end_trim_cursor;
mod issue_3695_glue_mid_line_after_interpolation;
mod issue_3698_choice_line_diverts;
mod ns_a4_ordering;
mod seq_verbs;
mod t1b_dialect_gate;
mod tm3_strict_policy;
mod tm4c_structs_codegen;
mod visibility;
