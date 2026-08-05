use std::collections::BTreeMap;

use rowan::TextRange;

use super::{Scope, SymbolKind};
use crate::TypeExpr;
use crate::host_manifest::DocBlock;

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
    /// Declared `STRUCT` shape names (TM-4b, docs/typed-mode-spec.md §6).
    pub structs: Vec<DeclaredSymbol>,
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
    /// Inline `///` doc-comment metadata for declarations, keyed by
    /// `(kind, declared name)` — stitch names are qualified (`knot.stitch`),
    /// matching how they're declared. Kept off `DeclaredSymbol` so the shared
    /// symbol type stays lean. For externals, merged with the registered
    /// host manifest by the analyzer.
    pub docs: BTreeMap<(SymbolKind, String), DocBlock>,
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
    /// Explicit `#@private`/`#@public` override on the declaration, if any
    /// (M-2, docs/modules-spec.md §4). `None` means "module default".
    pub visibility: Option<super::VisibilityMark>,
    /// The definition's old name and directive range, from a `#@was(old_name)`
    /// attached to this declaration (M-3, docs/modules-spec.md §5). `None`
    /// means no rename recorded.
    pub was: Option<(String, TextRange)>,
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
    /// The TM-2 (docs/typed-mode-spec.md §3) inline `: type` annotation on
    /// this local's own declaration, if any — a param's `name: type` or a
    /// `~ temp name: type = expr`'s ascription (issue #530: the per-file
    /// locals path `brink_analyzer::local_signature` reads to serve a
    /// `Param`/`Temp` `DefinitionId` a real signature instead of the
    /// `None` `signature_query` returns for one). `None` for a local with
    /// no annotation grammar at all (a `for`-loop binding, an `as` binding)
    /// as well as an unannotated param/temp.
    pub annotation: Option<TypeExpr>,
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
    /// `true` only for a `RefKind::Divert` whose source path crossed a
    /// module wall (`hir::Path::crosses_module_wall`, issue #2287) —
    /// `-> barter::haggle`, never ink's own dotted `-> knot.stitch`
    /// addressing, which reuses the same joined `path` string but never sets
    /// this. When `true`, `path` is joined with `::` (not `.`) so the
    /// qualifier prefix and bare target name split back apart cleanly
    /// (`resolve::lookup_qualified_divert`). Always `false` for every other
    /// `RefKind` — module-qualified access is a divert-only concern today.
    pub module_qualified: bool,
}

/// What kind of reference this is, for diagnostic context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefKind {
    Divert,
    Variable,
    Function,
    List,
    /// A struct construction literal's leading shape name (`Name#{…}`,
    /// TM-4b, docs/typed-mode-spec.md §6) — resolved against declared
    /// `SymbolKind::Struct` symbols.
    Struct,
    /// A TM-2 type annotation's bare nominal leaf name (docs/typed-mode-spec.md
    /// §3) — a struct field's declared type, or a `VAR`/`CONST`/`temp`
    /// annotation (issue #2249). Resolved against declared
    /// `SymbolKind::Struct` symbols exactly like `RefKind::Struct`, but
    /// unlike that kind, **not every occurrence names a struct**: `int`,
    /// `float`, `List`, … are equally legal `Named` leaves that were never
    /// meant to resolve here at all (`brink_ir::TypeExpr::Named`'s own doc).
    /// A miss is therefore never diagnosed by this reference's own
    /// resolution — `brink_analyzer::annotations::check` (`E061`) is the
    /// annotation-content diagnostic, run separately and project-flat (not
    /// referrer-scoped; issue #2249 leaves that asymmetry unresolved, same
    /// posture as issue #2233's `lookup_unique_by_name`).
    Type,
}
