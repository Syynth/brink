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
//!
//! **Shared file, separate type (issue #911, BH follow-up deliverable 1).**
//! `bevy-brink`'s `CapabilityManifest`/`CapabilityManifestExternal`
//! (`crates/bevy-brink/src/capability.rs`) deserializes this **same**
//! on-disk manifest file for a disjoint purpose (the host/ECS capability
//! grammar, `docs/effects-spec.md` §13.2) — reading only its `effects` key
//! and ignoring everything [`ManifestExternal`] owns, the same way this type
//! ignores `effects`. The two types are not converged onto one canonical
//! shape: `brink-ir` is compiler/IDE-only and must never depend on
//! `bevy-brink`'s ECS types, and the reverse edge is equally unwanted, so a
//! shared type would need a new third crate for two fields (`externals[].name`)
//! in common. Instead, `brink_format::manifest_field_names` pins the shared
//! key spellings (`externals`, `name`) both types' serde derives must keep
//! matching, and `crates/bevy-brink/tests/manifest_field_convergence.rs`
//! cross-validates one manifest literal against both types.

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
    /// The host's **inline markup vocabulary** (`docs/prose-dialect-spec.md`
    /// §4.2, issue #1733): the span kinds `<name attr="v">…</name>` may use,
    /// and the attributes each kind allows.
    ///
    /// Host-authored and co-located with [`Self::externals`] by the §3.4
    /// authorship test — a text-effect plugin can generate its tag
    /// declarations the same way bindings generate externals. (Element
    /// conventions are *project*-authored and live elsewhere, in the
    /// `brink.toml`-referenced conventions module; the two must not be
    /// conflated.)
    ///
    /// **Empty means freeform**, which is the default: markup is freeform by
    /// default (§4.2's first half, landed in PR #1732) and a manifest is what
    /// *tightens* it. A project that declares no vocabulary — including one
    /// that registers a manifest for its externals alone — is never diagnosed
    /// for a markup tag. See `brink_analyzer::markup_check`.
    #[serde(default)]
    pub markup: Vec<ManifestSpanKind>,
}

/// One declared inline-markup span kind (`docs/prose-dialect-spec.md` §4.2).
///
/// A tag name plus the attributes that tag accepts. Attribute *values* are
/// not modelled — span attributes are static text by construction (see
/// `SyntaxKind::SPAN_ATTR_VALUE`), so there is nothing to type-check a value
/// against, and the flat-nominal scope guardrail that governs
/// [`SemanticTypeDef`] applies here for the same reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestSpanKind {
    /// The tag name as written in source, e.g. `wave` for `<wave>…</wave>`.
    pub name: String,
    /// The attributes this kind accepts, e.g. `[{"name": "amount"}]` for
    /// `<wave amount="3">`. Empty means the kind takes no attributes.
    ///
    /// Issue #1997 widened this from a bare `Vec<String>` (issue #1733's
    /// original, allow-list-only shape) to [`Vec<ManifestSpanAttr>`] so a
    /// declared attribute can carry a `required` flag (`E173`) — see that
    /// type's own doc for the schema-headroom rationale.
    #[serde(default)]
    pub attrs: Vec<ManifestSpanAttr>,
}

/// One attribute a [`ManifestSpanKind`] accepts (`docs/prose-dialect-spec.md`
/// §4.2, issue #1780 gap 1, ruled by issue #1997).
///
/// Widens `ManifestSpanKind.attrs` from a bare `Vec<String>` to a record so
/// that [`Self::required`] has somewhere to live, *and* so that a future
/// attribute-value type has somewhere to land later without another schema
/// break — issue #1780's gap 2. **That second half is schema headroom
/// only: attribute-value typing is NOT implemented by this type.** Span
/// attribute values stay static text by construction
/// (`SyntaxKind::SPAN_ATTR_VALUE`); nothing here parses, resolves, or checks
/// one against anything. A future PR that wants typed values adds a new
/// `#[serde(default)]` field here — additive on an already-object-shaped
/// array element, unlike widening a bare `String` would have been.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestSpanAttr {
    /// The attribute name, e.g. `"amount"` for `<wave amount="3">`.
    pub name: String,
    /// Whether a span of this kind must carry this attribute, checked by
    /// `brink_analyzer::markup_check` (`E173`). Defaults to `false`
    /// (optional) when an attribute record omits this key. Note this is
    /// about the *record* shape, not the pre-#1997 bare-string element
    /// shape (`"attrs": ["amount"]`) — that older form does not deserialize
    /// at all and must be migrated to `{ "name": "amount" }`.
    #[serde(default)]
    pub required: bool,
    /// Reserved slot for a future attribute-value type (issue #1780 gap 2).
    /// **Inert.** Round-tripped through serde like any other field, but no
    /// pass in this crate reads it, resolves it against
    /// [`SemanticTypeDef`], or checks an attribute value against it — doing
    /// so is explicitly out of scope for issue #1997. It exists only so a
    /// later PR that *does* implement typing needs a new check, not a new
    /// manifest shape: `TypeRef` is `#[serde(transparent)]`, so this field's
    /// wire form is already the plain-string shape `ManifestParam::ty` uses
    /// (e.g. `"ty": "int"`), and switching it on is additive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<TypeRef>,
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
/// the nominal vocabulary `Handle<K>` type annotations resolve `K` against.
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

/// De-drift enforcement for issue #911's manifest convergence decision: the
/// wire keys [`HostManifest`]/[`ManifestExternal`] actually serialize under
/// must literally match `brink_format::manifest_field_names`'s constants —
/// the shared spellings `bevy_brink::capability::CapabilityManifest` also
/// depends on for the same on-disk file. If either side's `#[serde]` shape
/// ever renames `externals` or an external's `name`, this test's substring
/// checks fail here rather than the drift only surfacing as a silently
/// unparsed field on the other consumer.
#[cfg(test)]
mod manifest_field_name_tests {
    use brink_format::manifest_field_names::{EXTERNALS, NAME};

    use super::{HostManifest, ManifestExternal};

    #[test]
    fn serialized_wire_keys_match_the_shared_field_name_constants() {
        let manifest = HostManifest {
            markup: Vec::new(),
            externals: vec![ManifestExternal {
                name: "has".to_string(),
                params: vec![],
                returns: super::TypeRef::default(),
                kind: super::ExternalKind::default(),
                doc: None,
                widgets: vec![],
                path: vec![],
            }],
            types: vec![],
        };
        let json = serde_json::to_string(&manifest).expect("serialize");
        assert!(
            json.contains(&format!("\"{EXTERNALS}\":")),
            "top-level wrapper key drifted from manifest_field_names::EXTERNALS: {json}"
        );
        assert!(
            json.contains(&format!("\"{NAME}\":\"has\"")),
            "external entry's name key drifted from manifest_field_names::NAME: {json}"
        );

        // Round-trips through the constant-derived shape unchanged.
        let parsed: HostManifest = serde_json::from_str(&json).expect("re-parse");
        assert_eq!(parsed, manifest);
    }
}

#[cfg(test)]
mod value_source_tests {
    use super::{BaseType, SemanticTypeDef, ValueSource};

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

    #[test]
    fn handle_base_json_roundtrip() {
        // The host authors this JSON (e.g. via `setHostManifest`); lock the
        // wire shape for `"base": "handle"` the same way the other base
        // keywords are locked above — the with-manifest Handle<K> path
        // depends on this JSON -> BaseType::Handle deserialization, not just
        // on constructing BaseType::Handle directly in Rust.
        let def: SemanticTypeDef =
            serde_json::from_str(r#"{ "name": "AudioInstance", "base": "handle" }"#)
                .expect("parse handle base");
        assert_eq!(def.name, "AudioInstance");
        assert_eq!(def.base, BaseType::Handle);

        assert_eq!(BaseType::from_keyword("handle"), Some(BaseType::Handle));
    }
}

/// Guards the drift class flagged twice against `docs/host-capability-manifest.md`
/// (during #911's batch work and again in #921's review, tracked from #897 as
/// issue #924): the docs' Tier-1 JSON examples and `bevy-brink::capability`'s
/// doc-header example previously showed `params` as 2-tuples (`["item","string"]`)
/// or `{"type": "Handle<Npc>"}`, but [`ManifestParam`]'s real serde shape is
/// `{"name": ..., "ty": ...}`. This test parses the doc's actual Tier-1 example
/// JSON (`docs/host-capability-manifest.md` §"Tier 1") verbatim and round-trips
/// it through serde, so if a future edit reintroduces the wrong param shape in
/// either the docs or this fixture, the mismatch fails here instead of staying
/// latent (no real consumer round-trips this JSON yet).
#[cfg(test)]
mod doc_example_tests {
    use super::{ExternalKind, HostManifest, ManifestExternal, ManifestParam, TypeRef};

    #[test]
    fn tier_1_doc_example_roundtrips_through_manifest_param() {
        // Verbatim (minus jsonc comments) from the Tier 1 section of
        // docs/host-capability-manifest.md.
        let json = r#"
        { "externals": [
            { "name": "has",    "params": [{"name": "item", "ty": "string"}], "returns": "bool", "kind": "query" },
            { "name": "camera", "params": [{"name": "target", "ty": "string"}], "returns": "void", "kind": "presentation" },
            { "name": "grant",  "params": [{"name": "item", "ty": "string"}], "returns": "void", "kind": "effect" }
        ] }
        "#;

        let manifest: HostManifest = serde_json::from_str(json).expect("parse doc example");
        assert_eq!(manifest.externals.len(), 3);

        let has = &manifest.externals[0];
        assert_eq!(has.name, "has");
        assert_eq!(
            has.params,
            vec![ManifestParam {
                name: "item".to_string(),
                ty: TypeRef("string".to_string()),
            }]
        );
        assert_eq!(has.kind, ExternalKind::Query);

        // Round-trips unchanged: re-serializing and re-parsing produces the
        // same manifest, proving `{"name", "ty"}` is really the wire shape
        // ManifestParam's serde derive emits and accepts (not just something
        // this literal happens to parse into via defaulting).
        let serialized = serde_json::to_string(&manifest).expect("serialize");
        let round_tripped: HostManifest =
            serde_json::from_str(&serialized).expect("re-parse serialized manifest");
        assert_eq!(manifest, round_tripped);

        // The re-serialized wire form actually uses the documented keys, not
        // just field access we can pun on defaults.
        assert!(serialized.contains(r#""name":"item""#));
        assert!(serialized.contains(r#""ty":"string""#));

        let camera_params = &manifest.externals[1].params;
        assert_eq!(camera_params[0].name, "target");

        // A bare `ManifestExternal` literal in the exact shape used for the
        // `example(int actor)` "path" example a few paragraphs later.
        let set_move_route: ManifestExternal = serde_json::from_str(
            r#"{ "name": "set_move_route", "params": [{"name": "actor", "ty": "int"}],
                 "returns": "void", "kind": "effect", "path": ["Map", "Movement"] }"#,
        )
        .expect("parse path example");
        assert_eq!(
            set_move_route.params,
            vec![ManifestParam {
                name: "actor".to_string(),
                ty: TypeRef("int".to_string()),
            }]
        );
        assert_eq!(
            set_move_route.path,
            vec!["Map".to_string(), "Movement".to_string()]
        );
    }

    #[test]
    fn markup_vocabulary_doc_example_roundtrips() {
        // Verbatim (minus jsonc comments) from the "Markup vocabulary"
        // section of docs/host-capability-manifest.md — same guard as the
        // Tier-1 example above: the doc's JSON is the shape hosts copy, so a
        // drift between it and `ManifestSpanKind`'s serde derive fails here.
        //
        // Issue #1997 widened `attrs` from `Vec<String>` to
        // `Vec<ManifestSpanAttr>`; `sfx`'s `volume` is declared `required`
        // here to also pin the new flag's wire shape in the same fixture.
        let json = r#"
        { "markup": [
            { "name": "wave", "attrs": [{ "name": "amount" }] },
            { "name": "b" },
            { "name": "sfx", "attrs": [{ "name": "name" }, { "name": "volume", "required": true }] }
        ] }
        "#;

        let manifest: super::HostManifest = serde_json::from_str(json).expect("parse doc example");
        assert_eq!(manifest.markup.len(), 3);
        assert_eq!(manifest.markup[0].name, "wave");
        assert_eq!(
            manifest.markup[0].attrs,
            vec![super::ManifestSpanAttr {
                name: "amount".to_string(),
                required: false,
                ty: None,
            }]
        );
        // Omitted `attrs` defaults to empty — a tag that takes none.
        assert!(manifest.markup[1].attrs.is_empty());
        // Omitted `required` defaults to `false` (optional) — `name`'s
        // requiredness is unaffected by its sibling `volume` declaring one.
        assert!(!manifest.markup[2].attrs[0].required);
        assert!(manifest.markup[2].attrs[1].required);
        // A manifest carrying only `markup` leaves the other sections empty,
        // which is what makes markup declarable independently of externals.
        assert!(manifest.externals.is_empty());
        assert!(manifest.types.is_empty());

        let serialized = serde_json::to_string(&manifest).expect("serialize");
        let round_tripped: super::HostManifest =
            serde_json::from_str(&serialized).expect("re-parse serialized manifest");
        assert_eq!(manifest, round_tripped);
        assert!(
            serialized.contains(
                r#""markup":[{"name":"wave","attrs":[{"name":"amount","required":false}]}"#
            )
        );
    }

    /// Issue #1997 is a **breaking** wire-format change: the pre-#1997 bare
    /// attribute-name-array form (`"attrs": ["amount"]`) must no longer
    /// parse. `ManifestSpanAttr` is a plain derived-`Deserialize` struct
    /// with no untagged/custom impl, so a JSON string element where a
    /// `{ "name": ... }` record is expected is a hard type error, not a
    /// silently-defaulted optional field. This guards the migration claim
    /// made in `docs/host-capability-manifest.md`'s "Markup vocabulary"
    /// section and in [`ManifestSpanAttr::required`]'s doc comment, both of
    /// which previously (wrongly) implied this form still parsed.
    #[test]
    fn pre_1997_bare_attribute_name_array_is_rejected() {
        let json = r#"{ "markup": [{ "name": "wave", "attrs": ["amount"] }] }"#;
        assert!(
            serde_json::from_str::<super::HostManifest>(json).is_err(),
            "the pre-#1997 bare attribute-name form is deliberately rejected"
        );
    }
}
