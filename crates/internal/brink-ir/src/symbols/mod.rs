//! Symbol types shared between HIR lowering and semantic analysis.
//!
//! `SymbolManifest` is produced by HIR lowering (per-file declarations and
//! unresolved references). `SymbolIndex` is populated by the analyzer
//! (cross-file resolution). Both live here so that `brink-ir::lir` can
//! consume the resolved index without depending on `brink-analyzer`.

mod index;
mod manifest;
mod project;
mod roots;

pub use index::{
    ParamInfo, ResolutionMap, ResolvedRef, Scope, SymbolIndex, SymbolInfo, SymbolKind, Visibility,
    VisibilityMark,
};
pub use manifest::{DeclaredSymbol, LocalSymbol, RefKind, SymbolManifest, UnresolvedRef};
pub use project::project_manifest;
pub use roots::{RESERVED_ROOTS, STD_ROOT, STORY_ROOT, is_reserved_root_module};
