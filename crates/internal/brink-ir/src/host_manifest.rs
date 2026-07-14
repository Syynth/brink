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
    /// Arg-group widgets (argument-widget spec §2): a single widget spanning
    /// several params (e.g. `place_object(x, y)` → one `map_point` over `[0, 1]`).
    /// Advisory tooling metadata; never affects the compiled program.
    #[serde(default)]
    pub widgets: Vec<ArgGroupWidget>,
    /// Category breadcrumb for the Host Functions panel (#210), e.g.
    /// `["Map", "Movement"]` → nested collapsible sections. Advisory tooling
    /// metadata; never affects the compiled program. Empty = ungrouped.
    #[serde(default)]
    pub path: Vec<String>,
}

/// A widget over an argument group on an external (argument-widget spec §2).
/// Flat + serializable: the indices it spans, the widget/semantic type, the
/// editor surface, and optional inter-arg context (sibling arg → a key the
/// editor reads).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgGroupWidget {
    /// Argument indices the widget spans, e.g. `[0, 1]`.
    pub group: Vec<u32>,
    /// Semantic type / widget id (matches a host `ArgumentWidget.type`).
    #[serde(rename = "type")]
    pub ty: String,
    /// The editor container — `"popover"` (default) or `"modal"`.
    #[serde(default)]
    pub surface: Option<String>,
    /// Inter-arg context: context key → the sibling arg index supplying it,
    /// e.g. `{ "map": 1 }`. Sorted for deterministic resolution.
    #[serde(default)]
    pub context: std::collections::BTreeMap<String, u32>,
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
    /// Where this type's pickable values + labels come from (Tier 3, #174).
    /// Drives the author-time argument picker; **advisory** — orthogonal to
    /// `constraint` (which does checking) and never affects the compiled
    /// program. `None` means the param is entered as a plain literal.
    #[serde(default)]
    pub values: Option<ValueSource>,
    /// A studio-builtin argument widget for this type (Tier 3, argument-widget
    /// spec). Names a built-in kind (`color`, …); the studio renders the inline
    /// affordance + editor. Advisory tooling metadata; `None` means no widget.
    #[serde(default)]
    pub widget: Option<WidgetDecl>,
}

/// A studio-builtin argument widget declaration (argument-widget spec §2).
/// Flat by design — just the built-in kind for now; host-rendered editors are
/// declared per-external (`widgets`), not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetDecl {
    /// The built-in widget kind, e.g. `"color"`.
    pub kind: String,
}

/// One pickable value with its host-given display label (Tier 3, #174).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueItem {
    /// The literal inserted into source (e.g. `"5"`).
    pub value: String,
    /// The display label (e.g. `"HarborGate"`).
    pub label: String,
    /// Optional secondary text (e.g. `"Switch #5"`).
    #[serde(default)]
    pub detail: Option<String>,
}

/// Where a semantic type's pickable values come from (Tier 3, #174). Advisory
/// tooling metadata only — never checked against, never compiled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "source")]
pub enum ValueSource {
    /// A closed, labelled set baked into the manifest — drives the picker with
    /// no host attached (the static slice).
    Static { items: Vec<ValueItem> },
    /// Values are provided by the attached host at author time (pushed into the
    /// studio); empty until a host connects.
    Host,
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
///
/// `Handle` (T1d-2, docs/t1d-spec.md §3) is a distinct category from the
/// scalar bases above: a `SemanticTypeDef { base: Handle, .. }` entry doesn't
/// specialize a primitive (the way `switch_id` specializes `int`) — its
/// `name` field *is* the declared handle-kind name (e.g. `AudioInstance`),
/// the nominal vocabulary `handle<K>` type annotations resolve `K` against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseType {
    String,
    Int,
    Float,
    Bool,
    Void,
    /// A host-resource handle kind (T1d-2) — this type def's `name` is the
    /// kind name itself, not a specialization label.
    Handle,
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
            "handle" => Some(Self::Handle),
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

#[cfg(test)]
mod value_source_tests {
    use super::{SemanticTypeDef, ValueSource};

    #[test]
    fn static_value_source_json_roundtrip() {
        // The host authors this JSON; lock the wire shape.
        let json = r#"{
            "name": "switch_id",
            "base": "int",
            "values": { "source": "static", "items": [
                { "value": "5", "label": "HarborGate", "detail": "Switch #5" },
                { "value": "9", "label": "Vault" }
            ] }
        }"#;
        let def: SemanticTypeDef = serde_json::from_str(json).expect("parse");
        let items = match def.values {
            Some(ValueSource::Static { items }) => items,
            _ => Vec::new(),
        };
        assert_eq!(items.len(), 2, "two static items parsed");
        assert_eq!(items[0].value, "5");
        assert_eq!(items[0].label, "HarborGate");
        assert_eq!(items[0].detail.as_deref(), Some("Switch #5"));
        assert_eq!(items[1].detail, None);
    }

    #[test]
    fn host_value_source_and_omitted_values_parse() {
        let host: SemanticTypeDef = serde_json::from_str(
            r#"{ "name": "item_id", "base": "int", "values": { "source": "host" } }"#,
        )
        .expect("parse host");
        assert!(matches!(host.values, Some(ValueSource::Host)));

        // Omitted `values` is fine (Tiers 1–2 manifests, plain literals).
        let none: SemanticTypeDef =
            serde_json::from_str(r#"{ "name": "x", "base": "int" }"#).expect("parse bare");
        assert!(none.values.is_none());
    }
}
