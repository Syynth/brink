//! Incremental project database for inkle's ink narrative scripting language.
//!
//! `ProjectDb` caches parsed trees and lowered HIR per file, enabling
//! efficient re-analysis when individual files change. Both the compiler
//! (one-shot) and LSP (long-lived) use this as their project model.

mod db;
mod file_state;
mod include_graph;
mod knot_cache;

pub use brink_ir::FileId;
pub use db::{ProjectDb, resolve_include_path};
