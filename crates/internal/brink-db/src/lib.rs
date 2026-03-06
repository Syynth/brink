//! Incremental project database for inkle's ink narrative scripting language.
//!
//! `ProjectDb` caches parsed trees and lowered HIR per file, enabling
//! efficient re-analysis when individual files change. Both the compiler
//! (one-shot) and LSP (long-lived) use this as their project model.

mod db;
mod file_state;
mod include_graph;
mod knot_cache;

pub use brink_analyzer::AnalysisResult;
pub use brink_ir::FileId;
pub use db::ProjectDb;

/// Errors from file discovery.
#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    /// File I/O error during discovery.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Circular INCLUDE dependency detected.
    #[error("circular INCLUDE: {0}")]
    CircularInclude(String),
}
