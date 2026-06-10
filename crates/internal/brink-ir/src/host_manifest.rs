//! Host capability manifest — a tooling / author-time schema describing the
//! host's external-function vocabulary (signatures, semantic types) so the
//! analyzer and IDE can validate call sites and offer richer affordances.
//!
//! This is **never** consumed by the runtime or by codegen: the author always
//! writes `EXTERNAL foo(x)` in ink for the compiler, and the manifest only
//! *enriches* it. See `docs/host-capability-manifest.md`.
//!
//! Metadata comes from two sources that the analyzer merges:
//! 1. inline `///` doc-comments on `EXTERNAL` declarations (parsed during HIR
//!    lowering into [`DocBlock`]), and
//! 2. a registered [`HostManifest`] (host-owned, project-wide), deserialized
//!    from JSON.

use serde::{Deserialize, Serialize};

// ─── Registered manifest (deserialized from host-supplied JSON) ─────────

/// The host-owned, project-wide vocabulary registered with the editor session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostManifest {
    /// Per-external signatures (for verbs generated/registered in bulk rather
    /// than annotated inline).
    #[serde(default)]
    pub externals: Vec<ManifestExternal>,
    /// Semantic-type *definitions* — the vocabulary that inline `@param {…}`
    /// tags and manifest entries reference by name.
    #[serde(default)]
    pub types: Vec<SemanticTypeDef>,
}

/// A registered external-function signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestExternal {
    pub name: String,
    #[serde(default)]
    pub params: Vec<ManifestParam>,
    #[serde(default)]
    pub returns: TypeRef,
    #[serde(default)]
    pub kind: ExternalKind,
    #[serde(default)]
    pub doc: Option<String>,
}

/// A single registered parameter: a name and a (possibly unspecified) type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestParam {
    pub name: String,
    #[serde(default)]
    pub ty: TypeRef,
}

/// A flat-nominal semantic type: a base type plus one optional constraint.
/// Records, unions, and generics are intentionally out of scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticTypeDef {
    pub name: String,
    pub base: BaseType,
    #[serde(default)]
    pub constraint: Option<Constraint>,
}

/// A closed-domain constraint, statically checkable against literal arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Constraint {
    /// One of a fixed set of string values.
    Enum { values: Vec<String> },
    /// A string matching a regular expression.
    Regex { pattern: String },
    /// An integer within an inclusive `[min, max]` range (either end optional).
    Range {
        #[serde(default)]
        min: Option<i64>,
        #[serde(default)]
        max: Option<i64>,
    },
}

/// A reference to a type: either a base-type keyword (`string`, `int`,
/// `float`, `bool`, `void`) or the name of a registered [`SemanticTypeDef`].
/// Resolution happens at merge time; an empty ref means "unspecified".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TypeRef(pub String);

impl TypeRef {
    /// Whether no type was specified.
    #[must_use]
    pub fn is_unspecified(&self) -> bool {
        self.0.trim().is_empty()
    }

    /// The base type if this ref names one directly (not a semantic type).
    #[must_use]
    pub fn as_base(&self) -> Option<BaseType> {
        BaseType::from_keyword(self.0.trim())
    }
}

/// The underlying base types ink values can take at an external boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseType {
    String,
    Int,
    Float,
    Bool,
    Void,
}

impl BaseType {
    /// Parse a base-type keyword (case-insensitive).
    #[must_use]
    pub fn from_keyword(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "string" => Some(Self::String),
            "int" => Some(Self::Int),
            "float" => Some(Self::Float),
            "bool" => Some(Self::Bool),
            "void" => Some(Self::Void),
            _ => None,
        }
    }
}

/// The presentation/effect category of an external. Informational at the MVP:
/// surfaced in hover, drives no diagnostic.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalKind {
    /// A read-only query (no side effects).
    Query,
    /// A state-changing effect.
    Effect,
    /// A presentation-only call (client-side; no authoritative effect).
    Presentation,
    /// Unclassified.
    #[default]
    Plain,
}

impl ExternalKind {
    /// Parse a `@kind` tag value (case-insensitive). `None` for unknown values.
    #[must_use]
    pub fn from_tag(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "query" => Some(Self::Query),
            "effect" => Some(Self::Effect),
            "presentation" => Some(Self::Presentation),
            "plain" => Some(Self::Plain),
            _ => None,
        }
    }
}

// ─── Inline source-resident doc (parsed from `///` comments) ────────────

/// Metadata parsed from the `///` doc-comment block preceding a declaration
/// (`EXTERNAL`, knot, stitch, `VAR`, `CONST`, `LIST`). For externals it is the
/// inline counterpart of [`ManifestExternal`], minus type *definitions* (those
/// only come from a registered [`HostManifest`]).
///
/// Carries only [`TypeRef`]s (resolved against semantic types at merge time),
/// so it stays `Eq` and can live on a per-file `DeclaredSymbol`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocBlock {
    /// Free-text documentation (non-tag lines), joined by newlines.
    pub doc: Option<String>,
    /// `@param <name> {<type>}` entries, in source order.
    pub params: Vec<(String, TypeRef)>,
    /// `@returns {<type>}`.
    pub returns: Option<TypeRef>,
    /// `@kind <kind>`.
    pub kind: Option<ExternalKind>,
}
