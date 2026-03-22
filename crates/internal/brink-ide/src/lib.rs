pub mod code_actions;
mod completion;
pub mod document;
pub mod folding;
mod formatting;
pub mod hover;
pub mod inlay_hints;
pub mod line_context;
pub mod line_convert;
mod line_index;
pub mod navigation;
pub mod rename;
pub mod semantic_tokens;
pub mod session;
pub mod signature;
pub mod structural_move;
mod text;

pub use completion::{
    CompletionContext, CursorScope, cursor_scope, detect_completion_context, is_visible_in_context,
};
pub use formatting::{format_region, sort_knots_in_source, sort_stitches_in_knot};
pub use line_index::LineIndex;
pub use text::{
    builtin_hover_text, diff_to_edits, find_call_context, word_at_offset, word_range_at_offset,
};
