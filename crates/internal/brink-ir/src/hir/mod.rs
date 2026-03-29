//! HIR (High-level Intermediate Representation) types and per-file lowering.
//!
//! The HIR is a rich semantic tree produced by lowering the untyped AST from
//! `brink-syntax`. It preserves the full structure of the source — expressions
//! stay as trees, choices and conditionals keep their branch structure, diverts
//! are semantic nodes — with weave nesting resolved and syntactic sugar stripped.

pub mod lower;
mod normalize;
mod stamp;
mod types;

pub use lower::{WeaveItem, fold_weave, lower, lower_single_knot, lower_top_level};
pub use normalize::normalize_file;
pub use stamp::stamp_container_ids;
pub use types::*;
