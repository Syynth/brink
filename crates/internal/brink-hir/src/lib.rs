//! HIR types and per-file lowering for inkle's ink narrative scripting language.
//!
//! The HIR (High-level Intermediate Representation) is a rich semantic tree
//! produced by lowering the untyped AST from `brink-syntax`. It preserves the
//! full structure of the source — expressions stay as trees, choices and
//! conditionals keep their branch structure, diverts are semantic nodes — with
//! weave nesting resolved and syntactic sugar stripped.
//!
//! Both `brink-analyzer` (semantic analysis) and `brink-compiler` (codegen)
//! consume the HIR.

mod lower;
mod types;

pub use lower::lower;
pub use types::*;
