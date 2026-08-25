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

impl SymbolInfo {
    /// Whether this symbol is a statically-named **function definition** —
    /// ink's `=== function name ===` / native's `fn name(…)`, the only
    /// thing a function *value* can be taken of (`docs/t1c-spec.md` §2).
    ///
    /// A knot or stitch carrying the manifest's `"function"` sentinel in
    /// [`detail`](Self::detail). Deliberately *not* an `External`: an
    /// `EXTERNAL` has no body to address (see `brink-analyzer`'s `E079`,
    /// which reports exactly that).
    ///
    /// Shared by the two creation-site surfaces so they can never drift:
    /// `brink-analyzer`'s `#fn` creation-site check (`fn_values`) and
    /// `brink-ir`'s native bare-name fn-value lowering
    /// (`lir::lower::expr::lower_path`, issue #1862).
    #[must_use]
    pub fn is_function_definition(&self) -> bool {
        matches!(self.kind, SymbolKind::Knot | SymbolKind::Stitch)
            && self.detail.as_deref() == Some("function")
    }
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
    /// Stable numeric form for range-free memo seams (#3064 B4) — the
    /// per-segment resolution-kind maps carry `u32`s so their salsa memo
    /// payload is `Eq + Update` without this enum needing a salsa dep.
    #[must_use]
    pub fn to_u32(self) -> u32 {
        self as u32
    }

    /// Inverse of [`to_u32`](Self::to_u32); `None` for an unknown value.
    #[must_use]
    pub fn from_u32(value: u32) -> Option<Self> {
        [
            Self::Knot,
            Self::Stitch,
            Self::Variable,
            Self::Constant,
            Self::List,
            Self::ListItem,
            Self::External,
            Self::Label,
            Self::Param,
            Self::Temp,
            Self::Struct,
        ]
        .into_iter()
        .find(|k| *k as u32 == value)
    }
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
///
/// ## The `range` contract for a call-path reference (issue #1561)
///
/// For a call (`brink_ir::hir::Expr::Call(path, _)`), `range` **must equal
/// `path.range` exactly** — the callee `Path` node's own whole span, from
/// its first segment through its last. This holds for both an ordinary
/// single-segment callee (`f()`) and a UFCS-shaped multi-segment one
/// (`recv.verb(args)`, where the whole-path range still resolves to the
/// **receiver**, per `brink-analyzer::resolve::resolve_function`'s B3a
/// branch — never to a receiver-only or method-only sub-segment).
///
/// This is produced once, structurally, by
/// `brink_ir::symbols::project::Projector::walk_expr`'s `Expr::Call` arm
/// (`path.range` is what it hands to `push_ref`, becoming
/// `UnresolvedRef::range`), and every push site in
/// `brink-analyzer::resolve::resolve_function` carries it through
/// unchanged as `ResolvedRef::range` — see that function's own doc for the
/// full push-site list.
///
/// It is then an **exact lookup key** at least six separate consumers
/// key their own `(FileId, range)` maps on, independently:
///
/// - `brink_ir::lir::lower::expr::lower_call`'s `ctx.resolve_path(path.range)`;
/// - `brink_ir::lir::lower::expr::ufcs_receiver_path`, which deliberately
///   keeps `path.range` (not a receiver-only sub-range) on the desugared
///   receiver sub-path it builds, precisely so the *same* `resolve_path`
///   lookup above still hits when lowering that receiver as its own
///   expression;
/// - `brink_analyzer::strict::check_void_root`'s
///   `resolution_by_range.get(&range_key(path.range))` (the `E067`
///   void-assignment check);
/// - `brink_analyzer::coalesce::classify_coalesce_operand`'s equivalent
///   `resolution_by_range` lookup on a coalescing operand's call;
/// - `brink_analyzer::ufcs::value_receiver_def`'s
///   `resolution_by_range.get(&key)` lookup on the callee path — the mirror
///   of `resolve::resolve_function`'s own UFCS-shaped fallback, which must
///   agree with it or a call is diagnosed twice or not at all; and
/// - `brink_analyzer::infer::body::infer_call`'s `self.resolve(path.range)`
///   (backed by the same `resolution_by_range` map), whose B3a branch
///   explicitly handles a multi-segment (dotted UFCS) callee path.
///
/// Narrowing this range anywhere upstream — even in service of a real bug
/// fix elsewhere, e.g. a rename edit that must span only one segment — is a
/// silent miscompile here: every consumer above misses its lookup and
/// either falls back to a wrong resolution or refuses the compile with no
/// clue this field is the cause. That happened once already (#1550/#1554)
/// and was caught only by review, not by a test — hence this doc and the
/// cross-layer regression test in
/// `brink-test-harness/tests/resolved_ref_range_contract.rs`. A narrowing
/// fix for a *different* consumer (e.g. an IDE rename edit) belongs at that
/// consumer's own layer, never here — `brink-ide::ufcs_hover`'s
/// segment-narrowing helpers are the established pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRef {
    /// Which file the reference appears in.
    pub file: FileId,
    /// Source span of the reference. For a call-path reference this is
    /// load-bearing beyond diagnostics — see the contract on this struct's
    /// own doc (issue #1561).
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
