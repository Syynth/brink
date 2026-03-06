use brink_ir::{Diagnostic, Knot, SymbolManifest};
use rowan::GreenNode;

/// Cached lowering result for a single knot.
pub(crate) struct KnotEntry {
    /// Green node of the AST `KnotDef` — used for identity comparison on re-parse.
    pub green: GreenNode,
    /// Lowered HIR knot (`None` if malformed).
    pub knot: Option<Knot>,
    /// Per-knot symbol manifest fragment.
    pub manifest: SymbolManifest,
    /// Lowering diagnostics from this knot.
    pub diagnostics: Vec<Diagnostic>,
}
