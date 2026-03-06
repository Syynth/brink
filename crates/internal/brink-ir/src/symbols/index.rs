use std::collections::HashMap;

use brink_format::{DefinitionId, DefinitionTag};
use rowan::TextRange;

use crate::FileId;

// ─── Symbol index ───────────────────────────────────────────────────

/// The unified symbol table produced by merging per-file manifests and
/// resolving references.
#[derive(Debug, Clone, Default)]
pub struct SymbolIndex {
    /// All known definitions across all files.
    pub symbols: HashMap<DefinitionId, SymbolInfo>,
    /// Reverse index from canonical name to definition IDs.
    pub by_name: HashMap<String, Vec<DefinitionId>>,
}

/// Metadata for a resolved symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolInfo {
    /// What kind of symbol this is.
    pub kind: SymbolKind,
    /// Which file declared it.
    pub file: FileId,
    /// Source span of the declaration.
    pub range: TextRange,
    /// The tagged definition id.
    pub id: DefinitionId,
    /// The canonical/qualified name.
    pub name: String,
    /// Parameter names (for knots, stitches, externals).
    pub params: Vec<ParamInfo>,
    /// Additional detail (e.g. "function" for function knots).
    pub detail: Option<String>,
}

/// Parameter metadata for hover/signature help.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamInfo {
    /// Parameter name.
    pub name: String,
    /// `ref` parameter — passed by reference.
    pub is_ref: bool,
    /// `->` parameter — divert target.
    pub is_divert: bool,
}

/// The kind of a declared symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Knot,
    Stitch,
    Variable,
    Constant,
    List,
    ListItem,
    External,
    Label,
}

impl SymbolKind {
    /// Map a `SymbolKind` to the corresponding `DefinitionTag` for id generation.
    pub fn definition_tag(self) -> DefinitionTag {
        match self {
            Self::Knot | Self::Stitch | Self::Label => DefinitionTag::Container,
            Self::Variable | Self::Constant => DefinitionTag::GlobalVar,
            Self::List => DefinitionTag::ListDef,
            Self::ListItem => DefinitionTag::ListItem,
            Self::External => DefinitionTag::ExternalFn,
        }
    }
}

/// Resolution context — identifies the current knot/stitch for relative
/// path lookup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    /// The current knot name, if any.
    pub knot: Option<String>,
    /// The current stitch name, if any.
    pub stitch: Option<String>,
}
