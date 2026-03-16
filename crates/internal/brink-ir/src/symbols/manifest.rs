use rowan::TextRange;

use super::Scope;

/// Per-file symbol collection for cross-file resolution by the analyzer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SymbolManifest {
    /// Declared knot names.
    pub knots: Vec<DeclaredSymbol>,
    /// Declared stitch names (qualified: `knot.stitch`).
    pub stitches: Vec<DeclaredSymbol>,
    /// Declared global variable names (VAR).
    pub variables: Vec<DeclaredSymbol>,
    /// Declared constant names (CONST).
    pub constants: Vec<DeclaredSymbol>,
    /// Declared list names.
    pub lists: Vec<DeclaredSymbol>,
    /// Declared external function names.
    pub externals: Vec<DeclaredSymbol>,
    /// Declared labels (qualified: `knot.label` or `knot.stitch.label`).
    pub labels: Vec<DeclaredSymbol>,
    /// Declared list items (qualified: `ListName.ItemName`).
    pub list_items: Vec<DeclaredSymbol>,
    /// Local variables: params and temps, scoped to a container.
    pub locals: Vec<LocalSymbol>,
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

/// A local variable (param or temp) scoped to a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSymbol {
    /// Bare name (e.g. `x`).
    pub name: String,
    /// Source span of the declaration.
    pub range: TextRange,
    /// The scope this local belongs to.
    pub scope: Scope,
    /// Whether this is a param or a temp.
    pub kind: super::SymbolKind,
    /// For params: ref/divert metadata.
    pub param_detail: Option<super::ParamInfo>,
}

/// An unresolved reference that needs cross-file resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedRef {
    pub path: String,
    pub range: TextRange,
    pub kind: RefKind,
    /// The scope where this reference was encountered.
    pub scope: Scope,
    /// For `RefKind::Function` calls, the number of arguments at the call site.
    pub arg_count: Option<usize>,
}

/// What kind of reference this is, for diagnostic context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefKind {
    Divert,
    Variable,
    Function,
    List,
}
