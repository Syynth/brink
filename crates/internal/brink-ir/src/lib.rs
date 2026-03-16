//! Intermediate representations for the brink ink compiler.
//!
//! This crate owns all intermediate representations between parsing
//! (`brink-syntax`) and codegen/execution:
//!
//! - **`hir`** — High-level IR: rich semantic tree from AST lowering
//! - **`symbols`** — Symbol tables shared between HIR, analyzer, and LIR
//! - **`lir`** — Low-level IR: resolved, container-centric program for codegen

pub mod hir;
pub mod lir;
pub mod suppressions;
pub mod symbols;

// Re-export HIR and symbol types at the crate root for convenience.
// LIR types are accessed via `brink_ir::lir::` to avoid name conflicts.
pub use hir::*;
pub use symbols::*;
