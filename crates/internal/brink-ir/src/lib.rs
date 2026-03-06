//! Intermediate representations for the brink ink compiler.
//!
//! This crate owns all intermediate representations between parsing
//! (`brink-syntax`) and codegen/execution:
//!
//! - **`hir`** — High-level IR: rich semantic tree from AST lowering
//! - **`symbols`** — Symbol tables shared between HIR, analyzer, and LIR
//! - **`lir`** — (planned) Low-level IR: resolved, linearized program for codegen

pub mod hir;
pub mod symbols;

// Re-export all HIR types and symbol types at the crate root for convenience.
pub use hir::*;
pub use symbols::*;
