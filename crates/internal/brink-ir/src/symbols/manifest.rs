use rowan::TextRange;

use super::Scope;

/// Per-file symbol collection for cross-file resolution by the analyzer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SymbolManifest {
    /// Declared knot names.
    pub knots: Vec<DeclaredSymbol>,
    /// Declared stitch names (qualified: `knot.stitch`).
    pub stitches: Vec<DeclaredSymbol>,
    /// Declared global variable names (VAR + CONST).
    pub variables: Vec<DeclaredSymbol>,
    /// Declared list names.
    pub lists: Vec<DeclaredSymbol>,
    /// Declared external function names.
    pub externals: Vec<DeclaredSymbol>,
    /// Declared labels (qualified: `knot.label` or `knot.stitch.label`).
    pub labels: Vec<DeclaredSymbol>,
    /// Declared list items (qualified: `ListName.ItemName`).
    pub list_items: Vec<DeclaredSymbol>,
    /// Unresolved references (divert targets, variable accesses).
    pub unresolved: Vec<UnresolvedRef>,
}

/// A symbol declared in this file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredSymbol {
    pub name: String,
    pub range: TextRange,
    /// Parameter info (for knots, stitches, externals).
    pub params: Vec<super::ParamInfo>,
    /// Additional detail (e.g. "function" for function knots).
    pub detail: Option<String>,
}

/// An unresolved reference that needs cross-file resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedRef {
    pub path: String,
    pub range: TextRange,
    pub kind: RefKind,
    /// The scope where this reference was encountered.
    pub scope: Scope,
}

/// What kind of reference this is, for diagnostic context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefKind {
    Divert,
    Variable,
    Function,
    List,
}
