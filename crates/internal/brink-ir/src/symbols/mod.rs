//! Symbol types shared between HIR lowering and semantic analysis.
//!
//! `SymbolManifest` is produced by HIR lowering (per-file declarations and
//! unresolved references). `SymbolIndex` is populated by the analyzer
//! (cross-file resolution). Both live here so that `brink-ir::lir` can
//! consume the resolved index without depending on `brink-analyzer`.

mod index;
mod manifest;

pub use index::{
    ParamInfo, ResolutionMap, ResolvedRef, Scope, SymbolIndex, SymbolInfo, SymbolKind,
};
pub use manifest::{DeclaredSymbol, RefKind, SymbolManifest, UnresolvedRef};
