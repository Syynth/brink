//! Intermediate representations for the brink ink compiler.
//!
//! This crate owns all intermediate representations between parsing
//! (`brink-syntax`) and codegen/execution:
//!
//! - **`hir`** — High-level IR: rich semantic tree from AST lowering
//! - **`symbols`** — Symbol tables shared between HIR, analyzer, and LIR
//! - **`lir`** — Low-level IR: resolved, container-centric program for codegen

pub(crate) mod determinism;
pub mod dialect;
pub mod hir;
pub mod host_manifest;
pub mod lir;
pub mod provenance;
pub mod suppressions;
pub mod symbols;

// Re-export HIR and symbol types at the crate root for convenience.
// LIR types are accessed via `brink_ir::lir::` to avoid name conflicts.
mod line_index;
pub mod semantic_tokens;
pub mod trivia;
pub use dialect::{
    DialectError, DialogueDialect, ElementNature, ResolvedDialect, TemplateEntry, Templates,
    TransitionAction, TransitionRow, reserved_structural_kinds, validate_succession,
};
pub use hir::*;
pub use host_manifest::*;
pub use line_index::{LineIndex, doc_extended_start};
pub use provenance::{KindToken, NodeClass, Provenance, ProvenanceResolver};
/// Re-exported so a consumer can construct the [`Provenance`] ranges this
/// crate's own pub APIs take as parameters without naming `rowan` itself
/// (#3273: `brink-test-harness` builds synthetic `lir::Program`s).
pub use rowan::TextRange;
pub use symbols::*;
