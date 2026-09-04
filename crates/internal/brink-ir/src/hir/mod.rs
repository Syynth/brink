//! HIR (High-level Intermediate Representation) types and per-file lowering.
//!
//! The HIR is a rich semantic tree produced by lowering the untyped AST from
//! `brink-syntax`. It preserves the full structure of the source — expressions
//! stay as trees, choices and conditionals keep their branch structure, diverts
//! are semantic nodes — with weave nesting resolved and syntactic sugar stripped.

mod classify;
pub mod construct;
mod diagnostic_explanations;
mod diagnostics;
pub(crate) mod doc_block;
pub mod emit_native;
mod explain;
pub mod frame_shape;
mod ink_provenance;
pub mod line_context;
pub mod lower;
pub mod lower_native;
mod normalize;
pub mod projection;
pub mod rebase;
mod spans;
mod stamp;
mod types;
pub mod visit;

pub use classify::{
    ClassifiedCapture, ClassifiedMatch, LineClassification, classify_line,
    nearest_element_candidate,
};
pub use construct::{ConstructForm, ConstructTarget};
pub use diagnostics::*;
pub use explain::{ExplainMatchCache, LineExplanation, explain_match, explain_match_node};
pub use frame_shape::{AwaitFrameShape, ContinuationSite, compute_frame_shapes};
pub use ink_provenance::{InkProvenanceResolver, ink_provenance};
pub use lower::{
    WeaveItem, fold_weave, lower, lower_declarations, lower_single_knot, lower_top_level,
};
pub use normalize::{SYNTHETIC_TEMP_PREFIX, is_synthetic_temp_name, normalize_file};
pub use spans::expr_span;
pub use stamp::{root_content_scope_path, stamp_container_ids};
pub use types::*;
pub use visit::{ContentContext, HirVisitor, walk_block};
