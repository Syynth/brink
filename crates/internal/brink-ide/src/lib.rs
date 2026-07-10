pub mod argument_widgets;
pub mod auto_import;
pub mod code_actions;
pub mod color;
mod completion;
pub mod dir_rename;
pub mod document;
pub mod extract;
pub mod file_rename;
pub mod folding;
pub mod formatting;
pub mod hir_projection;
pub mod hover;
pub mod include_block;
pub mod inlay_hints;
pub mod line_context;
pub mod line_convert;
mod line_index;
pub mod navigation;
pub mod rename;
pub mod semantic_tokens;
pub mod session;
pub mod signature;
pub mod story_graph;
pub mod structural_delete;
pub mod structural_move;
pub mod structural_result;
mod text;
pub mod trivia;

pub use completion::{
    CompletionContext, CursorScope, cursor_scope, detect_completion_context, is_visible_in_context,
};

/// Author-time host value cache (Tier 3, #174): `host`-source semantic types →
/// the labelled values the attached host pushed in (`set_host_values`). Keyed
/// by semantic-type name. Consumed at query time by the argument picker +
/// value-label inlay hints; never part of analysis (so a push needs no
/// re-analyze). Empty when no host is attached.
pub type HostValues = std::collections::HashMap<String, Vec<brink_ir::ValueItem>>;
pub use formatting::{format_region, sort_knots_in_source, sort_stitches_in_knot};
pub use line_index::LineIndex;
pub use text::{
    builtin_hover_text, diff_to_edits, doc_extended_start, find_call_context, word_at_offset,
    word_range_at_offset,
};
