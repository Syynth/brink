mod completion;
mod line_index;
mod text;

pub use completion::{
    CompletionContext, CursorScope, cursor_scope, detect_completion_context, is_visible_in_context,
};
pub use line_index::LineIndex;
pub use text::{
    builtin_hover_text, diff_to_edits, find_call_context, word_at_offset, word_range_at_offset,
};
