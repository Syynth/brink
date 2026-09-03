pub mod argument_widgets;
pub mod arity_trim_fix;
pub mod auto_import;
pub mod code_actions;
pub mod color;
mod completion;
pub mod creation_site_fix;
pub mod dialect_config;
pub mod dir_rename;
pub mod document;
pub mod effects;
pub mod extract;
pub mod file_rename;
pub mod fix;
pub mod fn_value_hover;
pub mod folding;
pub mod formatting;
pub mod hir_projection;
/// Re-export shim (#3064 B3): the line-context core moved to
/// `brink_ir::hir::line_context` so `brink-db`'s per-segment queries can
/// call it; every existing `brink_ide::line_context::…` path keeps
/// working through this module.
pub mod line_context {
    pub use brink_ir::hir::line_context::*;
}
/// Re-export shim (#3064 B3) — see [`line_context`].
pub mod trivia {
    pub use brink_ir::trivia::*;
}
pub mod hover;
pub mod import_block;
pub mod import_fix;
pub mod include_block;
mod inferred_types;
pub mod inlay_hints;
pub mod line_convert;
pub mod navigation;
pub mod passage;
pub mod redundant_visibility_fix;
pub mod rename;
pub mod rename_detection;
pub mod semantic_tokens;
pub mod session;
pub mod signature;
pub mod stale_was_fix;
pub mod stdlib;
pub mod story_graph;
pub mod structural_delete;
pub mod structural_move;
pub mod structural_result;
pub mod style_hover;
mod text;
pub mod ufcs_hover;
pub mod value_call_fix;

pub use completion::{
    CompletionContext, CursorScope, cursor_scope, detect_completion_context, is_visible_in_context,
    ref_arg_root_prefix, stdlib_completion_context, stdlib_completions,
};

/// Author-time host value cache (Tier 3, #174): `host`-source semantic types →
/// the labelled values the attached host pushed in (`set_host_values`). Keyed
/// by semantic-type name. Consumed at query time by the argument picker +
/// value-label inlay hints; never part of analysis (so a push needs no
/// re-analyze). Empty when no host is attached.
pub type HostValues = std::collections::HashMap<String, Vec<brink_ir::ValueItem>>;
pub use brink_ir::LineIndex;
pub use formatting::{format_region, sort_knots_in_source, sort_stitches_in_knot};
pub use text::{
    builtin_hover_text, diff_to_edits, doc_extended_start, find_call_context, stdlib_hover_text,
    word_at_offset, word_range_at_offset,
};
