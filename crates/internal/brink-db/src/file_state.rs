use brink_ir::{Block, Diagnostic, HirFile, Knot, SymbolManifest};
use brink_syntax::Parse;
use rowan::GreenNode;

use crate::knot_cache::KnotEntry;

/// Cached lowering result for the top-level portion of a file (everything outside knots).
#[expect(
    dead_code,
    reason = "fields used for caching; consumers will read them"
)]
pub(crate) struct TopLevelEntry {
    /// Green nodes of the non-knot children (for change detection).
    pub green_children: Vec<GreenNode>,
    /// Lowered root content block.
    pub root_content: Block,
    /// Top-level stitches promoted to knots.
    pub top_level_knots: Vec<Knot>,
    /// Per-file manifest fragment from top-level declarations.
    pub manifest: SymbolManifest,
    /// Lowering diagnostics.
    pub diagnostics: Vec<Diagnostic>,
}

/// Complete cached state for a single source file.
#[expect(
    dead_code,
    reason = "fields used for caching; consumers will read them"
)]
pub(crate) struct FileState {
    pub source: String,
    pub parse: Parse,
    pub knot_entries: Vec<KnotEntry>,
    pub top_level: TopLevelEntry,
    /// Assembled from `knot_entries` + `top_level`. Always in sync.
    pub hir: HirFile,
    pub manifest: SymbolManifest,
    /// Combined parse + lowering diagnostics.
    pub diagnostics: Vec<Diagnostic>,
}
