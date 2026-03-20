use brink_ir::{Diagnostic, Knot, SymbolManifest};
use rowan::{GreenNode, TextSize};

/// Cached lowering result for a single knot.
pub(crate) struct KnotEntry {
    /// Green node of the AST `KnotDef` — used for identity comparison on re-parse.
    pub green: GreenNode,
    /// Byte offset of the knot in the file — cache is invalidated if this changes
    /// (e.g. when text is inserted before the knot, shifting its position).
    pub offset: TextSize,
    /// Lowered HIR knot (`None` if malformed).
    pub knot: Option<Knot>,
    /// Per-knot symbol manifest fragment.
    pub manifest: SymbolManifest,
    /// Lowering diagnostics from this knot.
    pub diagnostics: Vec<Diagnostic>,
}
