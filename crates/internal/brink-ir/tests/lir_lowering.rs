#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::items_after_statements
)]
// Issue #801: this crate's `clippy.toml` disallows bare `HashMap`/`HashSet`
// so an iteration-order leak into codegen output can't ship quietly. This
// file's one use (an always-empty `file_paths` map handed to
// `lower_to_program_with_type_mode` — this test harness never populates
// `SourceLocation`) has no order to leak; `crate::determinism::LookupMap`
// is `pub(crate)` and invisible to this external test-binary crate, so a
// file-level allow is the narrower fix (the `DefinitionId`-collision
// `seen` maps below are real `BTreeMap`s, not exempted).
#![allow(
    clippy::disallowed_types,
    reason = "always-empty file_paths map, no order to leak — see file doc"
)]

//! LIR lowering conformance tests, split by ink feature area (issue #689).
//!
//! This used to be one ~4,600-line file. Each `mod` below is a sibling file
//! under `tests/lir_lowering/` that was moved verbatim out of the original,
//! at the section boundaries the file was already organized by (the
//! `// ─── Section ─── ` dividers — still present at the top of each file).
//! The shared parse/lower/inspect harness lives in `support` and is
//! imported via `use crate::support::*;`. Per-section issue references
//! live in the section-divider comment inside each file, not here — a
//! trailing end-of-line comment on a `mod` item does not survive
//! `cargo fmt`'s alphabetical module reordering intact (it stays on the
//! line, not the item), so it is not a safe place to put annotations.

// `lir_lowering.rs` is itself an integration-test crate root, so plain
// `mod foo;` would resolve beside it (`tests/foo.rs`), not under
// `tests/lir_lowering/` — hence the explicit `#[path]` on every entry to
// keep the per-feature-area files out of the shared `tests/` directory.
#[path = "lir_lowering/support.rs"]
mod support;

#[path = "lir_lowering/assignments.rs"]
mod assignments;
#[path = "lir_lowering/author_warning_handling.rs"]
mod author_warning_handling;
#[path = "lir_lowering/bare_var_reference_nested_in_declaration_default.rs"]
mod bare_var_reference_nested_in_declaration_default;
#[path = "lir_lowering/basic_content.rs"]
mod basic_content;
#[path = "lir_lowering/block_scoped_temp_read_after_block_closes.rs"]
mod block_scoped_temp_read_after_block_closes;
#[path = "lir_lowering/branch_expansion.rs"]
mod branch_expansion;
#[path = "lir_lowering/break_continue_outside_loop.rs"]
mod break_continue_outside_loop;
#[path = "lir_lowering/builtin_functions.rs"]
mod builtin_functions;
#[path = "lir_lowering/call_through_variable.rs"]
mod call_through_variable;
#[path = "lir_lowering/choice_inline_divert_folding.rs"]
mod choice_inline_divert_folding;
#[path = "lir_lowering/choices.rs"]
mod choices;
#[path = "lir_lowering/collection_mutator_arity_mismatch.rs"]
mod collection_mutator_arity_mismatch;
#[path = "lir_lowering/collection_struct_literal_declaration_defaults.rs"]
mod collection_struct_literal_declaration_defaults;
#[path = "lir_lowering/complex_integration_scenarios.rs"]
mod complex_integration_scenarios;
#[path = "lir_lowering/conditionals.rs"]
mod conditionals;
#[path = "lir_lowering/const_folding_binary_expressions.rs"]
mod const_folding_binary_expressions;
#[path = "lir_lowering/container_counts_and_structure.rs"]
mod container_counts_and_structure;
#[path = "lir_lowering/container_definition_id_uniqueness.rs"]
mod container_definition_id_uniqueness;
#[path = "lir_lowering/counting_flags.rs"]
mod counting_flags;
#[path = "lir_lowering/diverts.rs"]
mod diverts;
#[path = "lir_lowering/expressions.rs"]
mod expressions;
#[path = "lir_lowering/externals.rs"]
mod externals;
#[path = "lir_lowering/glue_in_choice_body_before_gather.rs"]
mod glue_in_choice_body_before_gather;
#[path = "lir_lowering/glue_stripping_recognition_tests.rs"]
mod glue_stripping_recognition_tests;
#[path = "lir_lowering/inline_content_elements.rs"]
mod inline_content_elements;
#[path = "lir_lowering/knot_parameters.rs"]
mod knot_parameters;
#[path = "lir_lowering/knots.rs"]
mod knots;
#[path = "lir_lowering/lists.rs"]
mod lists;
#[path = "lir_lowering/logicblock_inside_unlifted_inline_conditional_sequence.rs"]
mod logicblock_inside_unlifted_inline_conditional_sequence;
#[path = "lir_lowering/nested_choice_inside_unlifted_inline_conditional.rs"]
mod nested_choice_inside_unlifted_inline_conditional;
#[path = "lir_lowering/nested_choices.rs"]
mod nested_choices;
#[path = "lir_lowering/pattern_recognizer_tests.rs"]
mod pattern_recognizer_tests;
#[path = "lir_lowering/rand_verb_surface.rs"]
mod rand_verb_surface;
#[path = "lir_lowering/range_values.rs"]
mod range_values;
#[path = "lir_lowering/read_count_builtin.rs"]
mod read_count_builtin;
#[path = "lir_lowering/return_statement.rs"]
mod return_statement;
#[path = "lir_lowering/root_content_definition_id_soundness.rs"]
mod root_content_definition_id_soundness;
#[path = "lir_lowering/root_final_gather.rs"]
mod root_final_gather;
#[path = "lir_lowering/sequences.rs"]
mod sequences;
#[path = "lir_lowering/stitches.rs"]
mod stitches;
#[path = "lir_lowering/string_interpolation_in_const_context.rs"]
mod string_interpolation_in_const_context;
#[path = "lir_lowering/structs_lir_and_codegen.rs"]
mod structs_lir_and_codegen;
#[path = "lir_lowering/t1b_2_logicblock_index_lower.rs"]
mod t1b_2_logicblock_index_lower;
#[path = "lir_lowering/t1e_1_path_projections.rs"]
mod t1e_1_path_projections;
#[path = "lir_lowering/t1e_2_path_projections.rs"]
mod t1e_2_path_projections;
#[path = "lir_lowering/tags.rs"]
mod tags;
#[path = "lir_lowering/temp_scoping_across_choice_gather_boundaries.rs"]
mod temp_scoping_across_choice_gather_boundaries;
#[path = "lir_lowering/temp_variables.rs"]
mod temp_variables;
#[path = "lir_lowering/template_recognition_slot_and_whitespace_only.rs"]
mod template_recognition_slot_and_whitespace_only;
#[path = "lir_lowering/thread_starts.rs"]
mod thread_starts;
#[path = "lir_lowering/tunnel_calls.rs"]
mod tunnel_calls;
#[path = "lir_lowering/variables_and_constants.rs"]
mod variables_and_constants;
