//! Incremental project database for inkle's ink narrative scripting language.
//!
//! `ProjectDb` is the query-shaped project model (scripting-substrate spec
//! §3): file texts are salsa inputs, and every pipeline stage — parse, HIR,
//! include graph, symbol index, resolution, signatures, analysis, LIR,
//! `StoryData` — is a memoized tracked query with dependency tracking and
//! early cutoff. This crate is the only one that knows salsa exists; stage
//! crates (`brink-syntax`, `brink-ir`, `brink-analyzer`,
//! `brink-codegen-inkb`) export plain functions the queries call. The
//! compiler (one-shot), LSP, and IDE all use `ProjectDb` as their project
//! model.

mod db;
mod include_graph;
#[cfg(feature = "memory-introspection")]
mod memory;
mod modules;
mod queries;

pub use brink_analyzer::{BodyTypes, InferenceResult, InferredSig, Sig, Ty};
pub use brink_ir::FileId;
pub use db::{ProjectDb, compute_relative_path, resolve_include_path};
#[cfg(feature = "memory-introspection")]
pub use memory::{IngredientKind, IngredientMemory};
pub use queries::{
    CompileProduct, FileDiagnostics, LirProduct, ResolvedProject, partition_diagnostics,
};
