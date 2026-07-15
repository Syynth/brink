use brink_format::{AliasEntry, DefinitionId, DefinitionTag};
use rowan::TextRange;

use crate::FileId;
use crate::determinism::LookupMap;

// ─── Symbol index ───────────────────────────────────────────────────

/// The unified symbol table produced by merging per-file manifests and
/// resolving references.
///
/// `symbols`/`by_name` are keyed lookup tables (`LookupMap` — a `HashMap`
/// under an issue #801 audited alias, see `crate::determinism`'s doc): every
/// consumer that needs a deterministic order over these builds it explicitly
/// at the point of consumption (e.g. `lir::lower::mod`'s `private_defs`
/// sorts by raw id; `brink-analyzer::modules`'s file-attribution fold is
/// order-independent by construction — see its doc comment).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SymbolIndex {
    /// All known definitions across all files.
    pub symbols: LookupMap<DefinitionId, SymbolInfo>,
    /// Reverse index from canonical name to definition IDs.
    pub by_name: LookupMap<String, Vec<DefinitionId>>,
    /// M-3 (docs/modules-spec.md §5): old→new `DefinitionId` rename records
    /// collected from `#@was(old_name)` directives while merging manifests.
    /// Unordered here (append order follows file/symbol processing order);
    /// `brink-ir::lir::lower` sorts by `old` before handing this to codegen.
    pub aliases: Vec<AliasEntry>,
}

/// Explicit visibility override on a declaration, from a `#@private` /
/// `#@public` directive (M-2, docs/modules-spec.md §4).
///
/// `None` on a declaration means "no override — take the module's default".
/// The *effective* [`Visibility`] is computed downstream by the analyzer,
/// which knows each file's declared-ness (including INCLUDE inheritance) and
/// so can apply declaration-flips-default (§4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibilityMark {
    Private,
    Public,
}

/// Effective visibility of a definition (M-2, docs/modules-spec.md §4) —
/// who may *reference the name*.
///
/// `Public` names cross module boundaries via `IMPORT`; `Private` names are
/// module-internal (and the host is outside every module). Computed by
/// declaration-flips-default: a *declared* module defaults `Private`, an
/// *undeclared* stem-module defaults `Public`, and a per-definition
/// [`VisibilityMark`] overrides that default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    #[default]
    Public,
    Private,
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
    /// Scope context — `Some` for locals (params/temps), `None` for globals.
    pub scope: Option<Scope>,
    /// For `Param` symbols: ref/divert metadata from the parent declaration.
    pub param_detail: Option<ParamInfo>,
    /// The declaring file's module name, `Some` only for a **declared**
    /// module (`#@module`, including INCLUDE inheritance). `None` for an
    /// undeclared stem-module — the permeable legacy world (M-2,
    /// docs/modules-spec.md §2/§4). Cross-module import enforcement keys off
    /// this: a reference into a declared module needs an `IMPORT`.
    pub module: Option<String>,
    /// Effective visibility (declaration-flips-default, §4). `Public` for
    /// the entire pre-modules world.
    pub visibility: Visibility,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SymbolKind {
    Knot,
    Stitch,
    Variable,
    Constant,
    List,
    ListItem,
    External,
    Label,
    /// A knot/stitch/function parameter.
    Param,
    /// A `~ temp` local variable.
    Temp,
    /// A `STRUCT` shape declaration (TM-4b, docs/typed-mode-spec.md §6).
    Struct,
}

impl SymbolKind {
    /// Map a `SymbolKind` to the corresponding `DefinitionTag` for id generation.
    pub fn definition_tag(self) -> DefinitionTag {
        match self {
            Self::Knot | Self::Stitch | Self::Label => DefinitionTag::Address,
            Self::Variable | Self::Constant => DefinitionTag::GlobalVar,
            Self::List => DefinitionTag::ListDef,
            Self::ListItem => DefinitionTag::ListItem,
            Self::External => DefinitionTag::ExternalFn,
            Self::Param | Self::Temp => DefinitionTag::LocalVar,
            Self::Struct => DefinitionTag::StructDef,
        }
    }
}

// ─── Resolution types ───────────────────────────────────────────────

/// A resolved reference: a use-site that has been matched to a definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRef {
    /// Which file the reference appears in.
    pub file: FileId,
    /// Source span of the reference.
    pub range: TextRange,
    /// The definition this reference resolves to.
    pub target: DefinitionId,
}

/// Maps reference use-sites to their resolved definitions, with file provenance.
pub type ResolutionMap = Vec<ResolvedRef>;

/// Resolution context — identifies the current knot/stitch for relative
/// path lookup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    /// The current knot name, if any.
    pub knot: Option<String>,
    /// The current stitch name, if any.
    pub stitch: Option<String>,
}
