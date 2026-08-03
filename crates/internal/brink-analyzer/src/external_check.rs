//! Symbol metadata enrichment: host-manifest checks for externals, doc/type
//! enrichment for knots and stitches, and initializer info for VAR/CONST.
//!
//! For externals, merges inline `///` doc-comments (parsed during HIR
//! lowering) with the registered [`HostManifest`] into [`SymbolMeta`] (keyed
//! by `DefinitionId`), and emits manifest-driven diagnostics. Knots/stitches
//! get doc-only enrichment ([`enrich_callables`]); VAR/CONST/LIST get
//! initializer-derived value info ([`infer_value_meta`]). Tooling /
//! author-time only — the runtime and codegen never see any of this.
//!
//! The enrichment map is always built (the IDE consumes it for hover /
//! signature even with checks disabled); only the *diagnostics* are gated by
//! the [`ExternalCheckSeverity`] policy.
//!
//! Semantic-type resolution additionally degrades gracefully when **no**
//! [`HostManifest`] is registered at all: an unknown type name is treated as
//! opaque (no `E040`) rather than hard-erroring, so ink that references host
//! vocabulary (e.g. `actor_id`) still compiles host-free. Once a manifest is
//! registered, checking is fully binding again — an unresolved name is a real
//! `E040`. See issue #339. A host that wants strict checking even without a
//! manifest (e.g. to catch typo'd semantic-type tags) can raise the
//! [`SemanticTypeDiagnosticSeverity`] lever to `Error`. See issue #532.

use std::collections::BTreeMap;

use brink_format::DefinitionId;
use brink_ir::hir::{Expr, HirFile, HirVisitor, Path, StringPart};
use brink_ir::{
    BaseType, Constraint, Diagnostic, DiagnosticCode, DocBlock, ExternalKind, FileId,
    SemanticTypeDef, SymbolIndex, SymbolInfo, SymbolKind, TypeRef,
};

/// Severity policy for manifest-driven external checks. Configurable as a
/// compiler/IDE flag; defaults to `Error` (a registered manifest is binding).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExternalCheckSeverity {
    /// Emit manifest-driven diagnostics (default).
    #[default]
    Error,
    /// Suppress manifest-driven diagnostics (enrichment is still built).
    Off,
}

/// Severity policy for unknown-semantic-type diagnostics (`E040`), parallel to
/// [`ExternalCheckSeverity`]. Configurable as a compiler/IDE flag; defaults to
/// `Tolerant` — the #339/#527 default-tolerant path, where an unresolved
/// semantic type is only diagnosed once a [`HostManifest`](brink_ir::HostManifest)
/// is registered. Raising it to `Error` opts back into strict checking (`E040`
/// fires for any unresolved type even with no manifest registered) — e.g. so a
/// host can catch typo'd semantic-type tags before wiring up a full manifest
/// (#532).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SemanticTypeDiagnosticSeverity {
    /// Tolerate unknown semantic types when no manifest is registered —
    /// opaque, no `E040` (default).
    #[default]
    Tolerant,
    /// Always emit `E040` for an unresolved semantic type, even with no
    /// manifest registered.
    Error,
}

/// Per-symbol merged metadata (docs, types, values), surfaced to the IDE and
/// used by the call-site checks. Keyed by the symbol's `DefinitionId` on the
/// `AnalysisResult`. For externals this merges inline docs with the registered
/// host manifest; knots/stitches carry inline docs only; VAR/CONST add an
/// inferred initializer value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SymbolMeta {
    /// Free-text documentation.
    pub doc: Option<String>,
    /// Presentation/effect category (informational; externals only —
    /// `Plain` everywhere else).
    pub kind: ExternalKind,
    /// Resolved return type, if specified.
    pub returns: Option<ResolvedType>,
    /// Resolved parameter types, by ink declaration order.
    pub params: Vec<ResolvedParam>,
    /// Initializer-derived value info (VAR/CONST only).
    pub value: Option<ValueMeta>,
    /// Arg-group widgets declared on the external (argument-widget spec §2);
    /// empty for non-externals.
    pub group_widgets: Vec<brink_ir::ArgGroupWidget>,
}

/// Initializer-derived metadata for a VAR or CONST declaration. Purely
/// presentational — ink variables are dynamically retyped at runtime, so this
/// never drives diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueMeta {
    /// Type inferred from the initializer literal, if it is one.
    pub ty: Option<InferredType>,
    /// Display text of the initializer value (CONST only), e.g. `"0.5"`.
    pub value_text: Option<String>,
}

/// The type of a VAR/CONST initializer literal. Deliberately separate from
/// the host-manifest `BaseType` vocabulary — `Divert`/`List` are ink runtime
/// concepts that must not leak into the manifest serialization schema.
///
/// `List` carries the declaring LIST's name (issue #628): a list-literal
/// initializer's type is nominal (`List<L>`), same as every other list type
/// in the `Ty` universe (`Ty::List`, TM-2's `List<L>` annotation) — the
/// scalar/divert variants have no such nominal identity, so they stay bare.
/// Not `Copy` any more (the `String` payload), unlike before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferredType {
    Int,
    Float,
    Bool,
    String,
    Divert,
    List(String),
}

impl InferredType {
    /// Display name, as shown in hover (e.g. `health: int`,
    /// `weather: List<Weathers>`).
    #[must_use]
    pub fn name(&self) -> String {
        match self {
            Self::Int => "int".to_string(),
            Self::Float => "float".to_string(),
            Self::Bool => "bool".to_string(),
            Self::String => "string".to_string(),
            Self::Divert => "divert".to_string(),
            Self::List(list) => format!("List<{list}>"),
        }
    }
}

/// A merged parameter: name (from the ink declaration) and resolved type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedParam {
    pub name: String,
    pub ty: Option<ResolvedType>,
}

/// A resolved type reference: the written name, its base type (if resolvable),
/// and any closed-domain constraint (from a semantic type definition).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedType {
    /// The type name as written (a base keyword or a semantic-type name).
    pub name: String,
    /// The underlying base type, if resolvable.
    pub base: Option<BaseType>,
    /// A closed-domain constraint, for literal-argument validation.
    pub constraint: Option<Constraint>,
    /// The picker's value source (Tier 3, #174) — advisory; drives the
    /// argument picker + value-label inlay hints, never checked against.
    pub values: Option<brink_ir::ValueSource>,
    /// The studio-builtin argument widget for this type (argument-widget spec)
    /// — advisory; drives the inline affordance + editor, never checked.
    pub widget: Option<brink_ir::WidgetDecl>,
}

impl ResolvedType {
    /// Whether this type actually resolved against a base keyword or a
    /// registered semantic type — `false` means `name` is neither (#1027):
    /// [`resolve_type`] still builds a `ResolvedType` for an unregistered
    /// name (so callers keep the written name for display), but `base` is
    /// `None` in that case. Consumers that render a type with unconditional
    /// confidence (hover, signature help) must check this first — showing
    /// `id: var_id` for an unregistered `var_id` is exactly the #1004
    /// divergence this issue closes.
    #[must_use]
    pub fn is_registered(&self) -> bool {
        self.base.is_some()
    }
}

/// Build the per-external enrichment map and collect manifest-driven
/// diagnostics. The map is always returned; diagnostics are empty when the
/// severity policy is `Off`.
///
/// `check_unknown_types` gates unknown-semantic-type checking (`E040`):
/// callers pass `true` when either a `HostManifest` is registered or the
/// [`SemanticTypeDiagnosticSeverity`] lever is raised to `Error`; otherwise an
/// unresolved type name is tolerated as opaque rather than diagnosed
/// (#339/#532).
pub fn analyze_externals(
    index: &SymbolIndex,
    inline_docs: &BTreeMap<(SymbolKind, String), DocBlock>,
    types: &BTreeMap<String, SemanticTypeDef>,
    registered: &BTreeMap<String, &brink_ir::ManifestExternal>,
    severity: ExternalCheckSeverity,
    check_unknown_types: bool,
) -> (BTreeMap<DefinitionId, SymbolMeta>, Vec<Diagnostic>) {
    let mut metas: BTreeMap<DefinitionId, SymbolMeta> = BTreeMap::new();
    let mut diags: Vec<Diagnostic> = Vec::new();

    // Deterministic order for diagnostics: sort externals by (file, offset).
    let mut externals: Vec<&SymbolInfo> = index
        .symbols
        .values()
        .filter(|info| info.kind == SymbolKind::External)
        .collect();
    externals.sort_by_key(|info| (info.file.0, info.range.start()));

    for info in externals {
        let inline = inline_docs.get(&(SymbolKind::External, info.name.clone()));
        let reg = registered.get(&info.name).copied();
        if inline.is_none() && reg.is_none() {
            continue; // no enrichment for this external
        }

        // Arity disagreement: registered manifest vs the (authoritative) ink decl.
        if let Some(reg) = reg
            && reg.params.len() != info.params.len()
        {
            diags.push(Diagnostic {
                file: info.file,
                range: info.range,
                message: format!(
                    "{}: `{}` is declared with {} parameter(s) but the manifest lists {}",
                    DiagnosticCode::E039.title(),
                    info.name,
                    info.params.len(),
                    reg.params.len(),
                ),
                code: DiagnosticCode::E039,
            });
        }

        // Param types: inline `@param` (by name) wins, else registered (by position).
        let mut params = Vec::with_capacity(info.params.len());
        for (i, p) in info.params.iter().enumerate() {
            let tref: Option<&TypeRef> = inline
                .and_then(|d| d.params.iter().find(|(n, _)| n == &p.name).map(|(_, t)| t))
                .or_else(|| reg.and_then(|r| r.params.get(i).map(|mp| &mp.ty)));
            let ty =
                tref.and_then(|t| resolve_type(t, types, info, check_unknown_types, &mut diags));
            params.push(ResolvedParam {
                name: p.name.clone(),
                ty,
            });
        }

        // Return type, kind, doc: inline wins, else registered.
        let returns = inline
            .and_then(|d| d.returns.as_ref())
            .or_else(|| reg.map(|r| &r.returns))
            .and_then(|t| resolve_type(t, types, info, check_unknown_types, &mut diags));
        let kind = inline
            .and_then(|d| d.kind)
            .or_else(|| reg.map(|r| r.kind))
            .unwrap_or_default();
        let doc = inline
            .and_then(|d| d.doc.clone())
            .or_else(|| reg.and_then(|r| r.doc.clone()));

        metas.insert(
            info.id,
            SymbolMeta {
                doc,
                kind,
                returns,
                params,
                value: None,
                group_widgets: reg.map(|r| r.widgets.clone()).unwrap_or_default(),
            },
        );
    }

    if severity == ExternalCheckSeverity::Off {
        diags.clear();
    }
    (metas, diags)
}

/// Build doc/type enrichment for knots and stitches from their inline `///`
/// docs. Signature tags (`@param` / `@returns`) resolve against the same
/// semantic-type vocabulary as externals (E040 on unknown types, tolerated as
/// opaque when `check_unknown_types` is `false` — #339/#532), but unlike
/// externals there are no call-site checks — callable metadata is
/// presentational only.
pub fn enrich_callables(
    index: &SymbolIndex,
    inline_docs: &BTreeMap<(SymbolKind, String), DocBlock>,
    types: &BTreeMap<String, SemanticTypeDef>,
    severity: ExternalCheckSeverity,
    check_unknown_types: bool,
) -> (BTreeMap<DefinitionId, SymbolMeta>, Vec<Diagnostic>) {
    let mut metas: BTreeMap<DefinitionId, SymbolMeta> = BTreeMap::new();
    let mut diags: Vec<Diagnostic> = Vec::new();

    // Deterministic order for diagnostics: sort callables by (file, offset).
    let mut callables: Vec<&SymbolInfo> = index
        .symbols
        .values()
        .filter(|info| matches!(info.kind, SymbolKind::Knot | SymbolKind::Stitch))
        .collect();
    callables.sort_by_key(|info| (info.file.0, info.range.start()));

    for info in callables {
        let Some(inline) = inline_docs.get(&(info.kind, info.name.clone())) else {
            continue;
        };

        // `@param` tags match declared params by name; unmatched tags are
        // ignored (same leniency as externals).
        let params = info
            .params
            .iter()
            .map(|p| {
                let tref = inline
                    .params
                    .iter()
                    .find(|(n, _)| n == &p.name)
                    .map(|(_, t)| t);
                ResolvedParam {
                    name: p.name.clone(),
                    ty: tref.and_then(|t| {
                        resolve_type(t, types, info, check_unknown_types, &mut diags)
                    }),
                }
            })
            .collect();
        let returns = inline
            .returns
            .as_ref()
            .and_then(|t| resolve_type(t, types, info, check_unknown_types, &mut diags));

        metas.insert(
            info.id,
            SymbolMeta {
                doc: inline.doc.clone(),
                kind: ExternalKind::Plain,
                returns,
                params,
                value: None,
                group_widgets: Vec::new(),
            },
        );
    }

    if severity == ExternalCheckSeverity::Off {
        diags.clear();
    }
    (metas, diags)
}

/// Build VAR/CONST/LIST metadata: initializer-inferred types, CONST display
/// values, and attached `///` docs. Purely presentational — ink variables are
/// dynamically retyped at runtime, so this never produces diagnostics.
pub fn infer_value_meta(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inline_docs: &BTreeMap<(SymbolKind, String), DocBlock>,
) -> BTreeMap<DefinitionId, SymbolMeta> {
    let mut metas: BTreeMap<DefinitionId, SymbolMeta> = BTreeMap::new();

    for &(_file_id, hir) in files {
        for v in &hir.variables {
            add_value_meta(
                &mut metas,
                index,
                inline_docs,
                SymbolKind::Variable,
                &v.name.text,
                Some(&v.value),
                false,
            );
        }
        for c in &hir.constants {
            add_value_meta(
                &mut metas,
                index,
                inline_docs,
                SymbolKind::Constant,
                &c.name.text,
                Some(&c.value),
                true,
            );
        }
        // Lists carry docs only — there is nothing to infer.
        for l in &hir.lists {
            add_value_meta(
                &mut metas,
                index,
                inline_docs,
                SymbolKind::List,
                &l.name.text,
                None,
                false,
            );
        }
    }
    metas
}

/// Insert a [`SymbolMeta`] for one VAR/CONST/LIST declaration, if it has a
/// doc or an inferable initializer.
fn add_value_meta(
    metas: &mut BTreeMap<DefinitionId, SymbolMeta>,
    index: &SymbolIndex,
    inline_docs: &BTreeMap<(SymbolKind, String), DocBlock>,
    kind: SymbolKind,
    name: &str,
    init: Option<&Expr>,
    show_value: bool,
) {
    let doc = inline_docs
        .get(&(kind, name.to_string()))
        .and_then(|d| d.doc.clone());
    let ty = init.and_then(|e| infer_literal_type(e, index));
    let value_text = if show_value {
        init.and_then(literal_display)
    } else {
        None
    };
    if doc.is_none() && ty.is_none() && value_text.is_none() {
        return;
    }
    let Some(id) = index.by_name.get(name).and_then(|ids| {
        ids.iter()
            .copied()
            .find(|id| index.symbols.get(id).is_some_and(|s| s.kind == kind))
    }) else {
        return;
    };
    let value = (ty.is_some() || value_text.is_some()).then_some(ValueMeta { ty, value_text });
    metas.insert(
        id,
        SymbolMeta {
            doc,
            kind: ExternalKind::Plain,
            returns: None,
            params: Vec::new(),
            value,
            group_widgets: Vec::new(),
        },
    );
}

/// The [`InferredType`] of an initializer literal, or `None` for anything
/// whose type isn't statically obvious (calls, references, arithmetic).
///
/// `index` resolves a `ListLiteral`'s items to their declaring LIST (issue
/// #628) — every other arm is purely syntactic and ignores it.
pub(crate) fn infer_literal_type(expr: &Expr, index: &SymbolIndex) -> Option<InferredType> {
    match expr {
        Expr::Int(_) => Some(InferredType::Int),
        Expr::Float(_) => Some(InferredType::Float),
        Expr::Bool(_) => Some(InferredType::Bool),
        Expr::String(_) => Some(InferredType::String),
        Expr::DivertTarget(_) => Some(InferredType::Divert),
        Expr::ListLiteral(items) => list_literal_name(items, index).map(InferredType::List),
        Expr::Prefix(brink_ir::hir::PrefixOp::Negate, inner) => match inner.as_ref() {
            Expr::Int(_) | Expr::Float(_) => infer_literal_type(inner, index),
            _ => None,
        },
        _ => None,
    }
}

/// The declaring LIST name for a list literal's items (issue #628), if any
/// item resolves unambiguously. Mirrors `infer::body::infer_list_literal`'s
/// policy exactly — first-resolved-item wins, so this phase-0 stub and the
/// real per-body HM inference never disagree: a "mixed" literal whose items
/// span more than one LIST (legal ink; the spec has no ruling narrowing it
/// further) reports whichever list its first resolvable item belongs to, not
/// a synthesized union. An item that doesn't resolve to a known list item
/// (typo, or a genuinely ambiguous bare name with no qualifying prefix) is
/// skipped, same "Unknown escape" fallback every other unrepresentable
/// initializer already gets — not a silent drop, since `None` still surfaces
/// as "no inferred type" rather than a wrong guess.
fn list_literal_name(items: &[brink_ir::hir::Path], index: &SymbolIndex) -> Option<String> {
    items
        .iter()
        .find_map(|item| resolve_list_item_name(item, index))
}

/// One list-literal item's declaring LIST name, resolved the same way
/// `resolve::lookup_list_item_bare` (bare form) and the qualified-list-item
/// branch of `resolve::lookup_by_name` (qualified form) do. List items are
/// always project-global — never locally scoped — so no `ImportScope`/
/// per-file resolution map is needed here, unlike general path resolution.
fn resolve_list_item_name(item: &brink_ir::hir::Path, index: &SymbolIndex) -> Option<String> {
    let segments: Vec<&str> = item.segments.iter().map(|s| s.text.as_str()).collect();
    let is_list_item = |id: &DefinitionId| {
        index
            .symbols
            .get(id)
            .is_some_and(|info| info.kind == SymbolKind::ListItem)
    };

    if segments.len() > 1 {
        // Qualified `ListName.ItemName` — an exact index hit is authoritative,
        // same as `resolve::lookup_by_name`'s qualified-list-item branch.
        let qualified = segments.join(".");
        if index
            .by_name
            .get(qualified.as_str())
            .is_some_and(|ids| ids.iter().any(is_list_item))
        {
            return qualified.split_once('.').map(|(list, _)| list.to_string());
        }
    }

    // Bare — suffix-match over every declared list item; ambiguous (two-plus
    // lists share this item name) or unresolved both fall through to `None`.
    let bare = segments.last()?;
    match crate::resolve::lookup_list_item_bare(index, bare) {
        crate::resolve::BareItemResult::Unique(id) => index
            .symbols
            .get(&id)
            .and_then(|info| info.name.split_once('.').map(|(list, _)| list.to_string())),
        crate::resolve::BareItemResult::Ambiguous | crate::resolve::BareItemResult::NotFound => {
            None
        }
    }
}

/// Display text for a literal initializer (CONST hover), e.g. `0.5`,
/// `"sword"`, `-> hub`. `None` for non-literals and interpolated strings.
fn literal_display(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Int(n) => Some(n.to_string()),
        Expr::Float(f) => Some(float_display(f.to_f64())),
        Expr::Bool(b) => Some(b.to_string()),
        Expr::String(_) => plain_string_value(expr).map(|s| format!("\"{s}\"")),
        Expr::DivertTarget(p) => Some(format!("-> {}", path_display(p))),
        Expr::Prefix(brink_ir::hir::PrefixOp::Negate, inner) => match inner.as_ref() {
            Expr::Int(_) | Expr::Float(_) => literal_display(inner).map(|s| format!("-{s}")),
            _ => None,
        },
        _ => None,
    }
}

/// Format a float for display, keeping a trailing `.0` so it still reads as
/// a float (`1.0`, not `1`).
fn float_display(v: f64) -> String {
    let s = v.to_string();
    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("NaN") {
        s
    } else {
        format!("{s}.0")
    }
}

fn path_display(path: &Path) -> String {
    path.segments
        .iter()
        .map(|n| n.text.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

/// Resolve a [`TypeRef`] to a [`ResolvedType`], emitting E040 for an unknown
/// semantic type — but only when `check_unknown_types` is `true`. With no manifest
/// registered, semantic types are opaque by default (host vocabulary can't be
/// validated against nothing) so an unresolved name is silently accepted as
/// `base: None` rather than diagnosed (#339). Returns `None` for an
/// unspecified (empty) ref.
///
/// Classification (base keyword / registered / unregistered) goes through
/// [`crate::type_resolution::classify`] — the same function
/// `infer::type_ref_to_ty` (strict inference) uses — so an unregistered name
/// is classified identically on both paths (#1027). A `base: None`
/// [`ResolvedType`] coming out of the `Unregistered` arm below is this
/// function's honest way of saying "unresolved"; see
/// [`ResolvedType::is_registered`] for the flag consumers must check before
/// rendering the name with any confidence.
fn resolve_type(
    t: &TypeRef,
    types: &BTreeMap<String, SemanticTypeDef>,
    info: &SymbolInfo,
    check_unknown_types: bool,
    diags: &mut Vec<Diagnostic>,
) -> Option<ResolvedType> {
    use crate::type_resolution::{TypeShape, classify};

    match classify(t, types) {
        TypeShape::Unspecified => None,
        TypeShape::Base(base) => Some(ResolvedType {
            name: t.0.clone(),
            base: Some(base),
            constraint: None,
            values: None,
            widget: None,
        }),
        TypeShape::Registered(def) => Some(ResolvedType {
            name: t.0.clone(),
            base: Some(def.base),
            constraint: def.constraint.clone(),
            values: def.values.clone(),
            widget: def.widget.clone(),
        }),
        TypeShape::Unregistered => {
            if check_unknown_types {
                diags.push(Diagnostic {
                    file: info.file,
                    range: info.range,
                    message: format!(
                        "{}: `{}` (on `{}`)",
                        DiagnosticCode::E040.title(),
                        t.0.trim(),
                        info.name,
                    ),
                    code: DiagnosticCode::E040,
                });
            }
            Some(ResolvedType {
                name: t.0.clone(),
                base: None,
                constraint: None,
                values: None,
                widget: None,
            })
        }
    }
}

// ─── Call-site literal checks (E041 type, E042 closed-domain) ───────────

/// Walk the HIR and check literal arguments at external call sites against the
/// merged [`SymbolMeta`]. Only literal arguments are checked (ink is
/// dynamically typed); non-literals and untyped params are skipped, so there
/// are no false positives. Returns the diagnostics (caller gates on severity).
pub fn check_call_sites(
    files: &[(FileId, &HirFile)],
    name_to_meta: &BTreeMap<&str, &SymbolMeta>,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    if name_to_meta.is_empty() {
        return diags;
    }
    for &(file_id, hir) in files {
        let mut checker = CallSiteChecker {
            file_id,
            name_to_meta,
            diags: &mut diags,
        };
        brink_ir::hir::visit::visit(hir, &mut checker);
    }
    diags
}

/// Visits every call site in a file via the shared HIR visitor, checking each
/// literal argument against the merged [`SymbolMeta`].
struct CallSiteChecker<'a, 'm> {
    file_id: FileId,
    name_to_meta: &'m BTreeMap<&'m str, &'m SymbolMeta>,
    diags: &'a mut Vec<Diagnostic>,
}

impl HirVisitor for CallSiteChecker<'_, '_> {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_expr(&mut self, expr: &Expr) {
        if let Expr::Call(path, args) = expr {
            check_call(self.file_id, path, args, self.name_to_meta, self.diags);
        }
    }
}

fn check_call(
    file: FileId,
    path: &Path,
    args: &[Expr],
    name_to_meta: &BTreeMap<&str, &SymbolMeta>,
    diags: &mut Vec<Diagnostic>,
) {
    let name = path
        .segments
        .iter()
        .map(|n| n.text.as_str())
        .collect::<Vec<_>>()
        .join(".");
    let Some(meta) = name_to_meta.get(name.as_str()) else {
        return; // not an external we have metadata for
    };
    for (i, arg) in args.iter().enumerate() {
        let Some(param) = meta.params.get(i) else {
            continue; // surplus args — arity is checked elsewhere
        };
        let Some(ty) = &param.ty else {
            continue; // untyped param — nothing to check
        };
        check_literal_arg(file, path, &name, arg, ty, diags);
    }
}

/// Check one literal argument against a resolved param type. Non-literals are
/// ignored (literals-only). A type mismatch suppresses the domain check.
fn check_literal_arg(
    file: FileId,
    path: &Path,
    call: &str,
    arg: &Expr,
    ty: &ResolvedType,
    diags: &mut Vec<Diagnostic>,
) {
    if let (Some(lit), Some(expected)) = (literal_base(arg), ty.base)
        && !compatible(lit, expected)
    {
        diags.push(Diagnostic {
            file,
            range: path.range,
            message: format!(
                "{}: `{call}` expects {} but a {} literal was passed",
                DiagnosticCode::E041.title(),
                base_name(expected),
                base_name(lit),
            ),
            code: DiagnosticCode::E041,
        });
        return;
    }
    if let Some(constraint) = &ty.constraint {
        check_constraint(file, path, call, &ty.name, arg, constraint, diags);
    }
}

/// The base type of a literal expression, or `None` if not a literal.
fn literal_base(expr: &Expr) -> Option<BaseType> {
    match expr {
        Expr::Int(_) => Some(BaseType::Int),
        Expr::Float(_) => Some(BaseType::Float),
        Expr::Bool(_) => Some(BaseType::Bool),
        Expr::String(_) => Some(BaseType::String),
        _ => None,
    }
}

/// Whether a literal of base `lit` is acceptable for a param of base
/// `expected`. Int widens to Float; otherwise an exact match is required.
fn compatible(lit: BaseType, expected: BaseType) -> bool {
    lit == expected || (lit == BaseType::Int && expected == BaseType::Float)
}

fn base_name(base: BaseType) -> &'static str {
    match base {
        BaseType::String => "string",
        BaseType::Int => "int",
        BaseType::Float => "float",
        BaseType::Bool => "bool",
        BaseType::Void => "void",
        // No handle literal syntax exists (T1d-1), so `literal_base` never
        // produces this arm in practice — kept exhaustive so a future
        // literal form can't silently skip this display path.
        BaseType::Handle => "handle",
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "range bounds are small integers; f64 comparison is exact in practice"
)]
fn check_constraint(
    file: FileId,
    path: &Path,
    call: &str,
    type_name: &str,
    arg: &Expr,
    constraint: &Constraint,
    diags: &mut Vec<Diagnostic>,
) {
    match constraint {
        Constraint::Enum { values } => {
            if let Some(s) = plain_string_value(arg)
                && !values.iter().any(|v| v == s)
            {
                diags.push(Diagnostic {
                    file,
                    range: path.range,
                    message: format!(
                        "{}: `{s}` is not a valid `{type_name}` value for `{call}`",
                        DiagnosticCode::E042.title(),
                    ),
                    code: DiagnosticCode::E042,
                });
            }
        }
        Constraint::Range { min, max } => {
            if let Some(v) = numeric_value(arg)
                && (min.is_some_and(|m| v < m as f64) || max.is_some_and(|m| v > m as f64))
            {
                diags.push(Diagnostic {
                    file,
                    range: path.range,
                    message: format!(
                        "{}: value out of range for `{type_name}` on `{call}`",
                        DiagnosticCode::E042.title(),
                    ),
                    code: DiagnosticCode::E042,
                });
            }
        }
        // Regex enforcement is deferred (no regex dependency at the MVP); the
        // pattern is still surfaced to the IDE via SymbolMeta.
        Constraint::Regex { .. } => {}
    }
}

/// The string value of a plain (non-interpolated) string literal.
fn plain_string_value(expr: &Expr) -> Option<&str> {
    let Expr::String(s) = expr else { return None };
    match s.parts.as_slice() {
        [] => Some(""),
        [StringPart::Literal(text)] => Some(text),
        _ => None, // interpolated — value not statically known
    }
}

/// The numeric value of an int/float literal as `f64`.
fn numeric_value(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Int(n) => Some(f64::from(*n)),
        Expr::Float(f) => Some(f.to_f64()),
        _ => None,
    }
}

#[cfg(test)]
#[expect(clippy::cast_possible_truncation, reason = "test helper ranges")]
mod tests {
    use brink_ir::{
        DeclaredSymbol, DocBlock, ManifestExternal, ManifestParam, ParamInfo, SemanticTypeDef,
        SymbolManifest, TypeRef,
    };
    use brink_ir::{DiagnosticCode, FileId};
    use rowan::{TextRange, TextSize};

    use super::*;
    use crate::manifest::merge_manifests;

    fn index_with_external(name: &str, params: &[&str]) -> SymbolIndex {
        let mut m = SymbolManifest::default();
        m.externals.push(DeclaredSymbol {
            name: name.to_string(),
            range: TextRange::new(TextSize::new(0), TextSize::new(name.len() as u32)),
            params: params
                .iter()
                .map(|n| ParamInfo {
                    name: (*n).to_string(),
                    is_ref: false,
                    is_divert: false,
                })
                .collect(),
            detail: None,
            visibility: None,
            was: None,
        });
        merge_manifests(&[(FileId(0), &m)]).0
    }

    fn meta_for<'a>(
        metas: &'a BTreeMap<DefinitionId, SymbolMeta>,
        index: &SymbolIndex,
        name: &str,
    ) -> &'a SymbolMeta {
        let id = index
            .symbols
            .values()
            .find(|s| s.kind == SymbolKind::External && s.name == name)
            .expect("external in index")
            .id;
        metas.get(&id).expect("meta for external")
    }

    fn inline(
        params: &[(&str, &str)],
        returns: Option<&str>,
        kind: Option<ExternalKind>,
    ) -> DocBlock {
        DocBlock {
            doc: None,
            params: params
                .iter()
                .map(|(n, t)| ((*n).to_string(), TypeRef((*t).to_string())))
                .collect(),
            returns: returns.map(|t| TypeRef(t.to_string())),
            kind,
        }
    }

    #[test]
    fn inline_doc_enriches_meta() {
        let index = index_with_external("has", &["item"]);
        let mut docs = BTreeMap::new();
        docs.insert(
            (SymbolKind::External, "has".to_string()),
            inline(&[("item", "bool")], Some("bool"), Some(ExternalKind::Query)),
        );
        let (metas, diags) = analyze_externals(
            &index,
            &docs,
            &BTreeMap::new(),
            &BTreeMap::new(),
            ExternalCheckSeverity::Error,
            true,
        );
        assert!(diags.is_empty(), "no diags: {diags:?}");
        let meta = meta_for(&metas, &index, "has");
        assert_eq!(meta.kind, ExternalKind::Query);
        assert_eq!(
            meta.returns.as_ref().and_then(|t| t.base),
            Some(BaseType::Bool)
        );
        assert_eq!(
            meta.params[0].ty.as_ref().and_then(|t| t.base),
            Some(BaseType::Bool)
        );
    }

    #[test]
    fn registered_enriches_when_no_inline() {
        let index = index_with_external("grant", &["item"]);
        let reg_ext = ManifestExternal {
            name: "grant".to_string(),
            params: vec![ManifestParam {
                name: "item".to_string(),
                ty: TypeRef("string".to_string()),
            }],
            returns: TypeRef("void".to_string()),
            kind: ExternalKind::Effect,
            doc: Some("Grant an item.".to_string()),

            widgets: vec![],
            path: Vec::new(),
        };
        let mut registered = BTreeMap::new();
        registered.insert("grant".to_string(), &reg_ext);
        let (metas, _) = analyze_externals(
            &index,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &registered,
            ExternalCheckSeverity::Error,
            true,
        );
        let meta = meta_for(&metas, &index, "grant");
        assert_eq!(meta.kind, ExternalKind::Effect);
        assert_eq!(meta.doc.as_deref(), Some("Grant an item."));
        assert_eq!(
            meta.params[0].ty.as_ref().and_then(|t| t.base),
            Some(BaseType::String)
        );
    }

    #[test]
    fn inline_wins_over_registered() {
        let index = index_with_external("has", &["item"]);
        let reg_ext = ManifestExternal {
            name: "has".to_string(),
            params: vec![ManifestParam {
                name: "item".to_string(),
                ty: TypeRef("int".to_string()),
            }],
            returns: TypeRef("int".to_string()),
            kind: ExternalKind::Effect,
            doc: None,

            widgets: vec![],
            path: Vec::new(),
        };
        let mut registered = BTreeMap::new();
        registered.insert("has".to_string(), &reg_ext);
        let mut docs = BTreeMap::new();
        docs.insert(
            (SymbolKind::External, "has".to_string()),
            inline(&[("item", "bool")], Some("bool"), Some(ExternalKind::Query)),
        );

        let (metas, _) = analyze_externals(
            &index,
            &docs,
            &BTreeMap::new(),
            &registered,
            ExternalCheckSeverity::Error,
            true,
        );
        let meta = meta_for(&metas, &index, "has");
        assert_eq!(meta.kind, ExternalKind::Query, "inline @kind wins");
        assert_eq!(
            meta.params[0].ty.as_ref().and_then(|t| t.base),
            Some(BaseType::Bool)
        );
    }

    #[test]
    fn semantic_type_resolves_constraint() {
        let index = index_with_external("give", &["item"]);
        let mut types = BTreeMap::new();
        types.insert(
            "item_id".to_string(),
            SemanticTypeDef {
                name: "item_id".to_string(),
                base: BaseType::String,
                constraint: Some(Constraint::Enum {
                    values: vec!["sword".into(), "shield".into()],
                }),
                values: None,
                widget: None,
            },
        );
        let mut docs = BTreeMap::new();
        docs.insert(
            (SymbolKind::External, "give".to_string()),
            inline(&[("item", "item_id")], None, None),
        );

        let (metas, diags) = analyze_externals(
            &index,
            &docs,
            &types,
            &BTreeMap::new(),
            ExternalCheckSeverity::Error,
            true,
        );
        assert!(diags.is_empty(), "known semantic type: {diags:?}");
        let ty = meta_for(&metas, &index, "give").params[0]
            .ty
            .clone()
            .unwrap();
        assert_eq!(ty.base, Some(BaseType::String));
        assert!(matches!(ty.constraint, Some(Constraint::Enum { .. })));
    }

    #[test]
    fn unknown_semantic_type_emits_e040() {
        let index = index_with_external("foo", &["x"]);
        let mut docs = BTreeMap::new();
        docs.insert(
            (SymbolKind::External, "foo".to_string()),
            inline(&[("x", "bogus")], None, None),
        );

        let (metas, diags) = analyze_externals(
            &index,
            &docs,
            &BTreeMap::new(),
            &BTreeMap::new(),
            ExternalCheckSeverity::Error,
            true,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E040);
        // Meta is still built, with an unresolved (base: None) param type.
        assert!(
            meta_for(&metas, &index, "foo").params[0]
                .ty
                .as_ref()
                .unwrap()
                .base
                .is_none()
        );
    }

    /// #339: with no `HostManifest` registered at all, an unresolved
    /// semantic type is tolerated as opaque — no E040 — rather than
    /// hard-erroring. Enrichment is still built (base: None).
    #[test]
    fn unknown_semantic_type_tolerated_without_manifest() {
        let index = index_with_external("foo", &["x"]);
        let mut docs = BTreeMap::new();
        docs.insert(
            (SymbolKind::External, "foo".to_string()),
            inline(&[("x", "actor_id")], None, None),
        );

        let (metas, diags) = analyze_externals(
            &index,
            &docs,
            &BTreeMap::new(),
            &BTreeMap::new(),
            ExternalCheckSeverity::Error,
            false, // no manifest registered
        );
        assert!(
            diags.is_empty(),
            "no manifest: tolerated, no E040: {diags:?}"
        );
        assert!(
            meta_for(&metas, &index, "foo").params[0]
                .ty
                .as_ref()
                .unwrap()
                .base
                .is_none(),
            "type is still opaque (base: None) — just not diagnosed"
        );
    }

    /// #339, other half: once a manifest *is* registered, checking is fully
    /// binding again — a genuinely unknown type still emits E040 even though
    /// other types resolve fine.
    #[test]
    fn unknown_semantic_type_still_errors_with_manifest_registered() {
        let index = index_with_external("foo", &["x"]);
        let mut types = BTreeMap::new();
        types.insert(
            "actor_id".to_string(),
            SemanticTypeDef {
                name: "actor_id".to_string(),
                base: BaseType::String,
                constraint: None,
                values: None,
                widget: None,
            },
        );
        let mut docs = BTreeMap::new();
        docs.insert(
            (SymbolKind::External, "foo".to_string()),
            inline(&[("x", "totally_bogus")], None, None),
        );

        let (_metas, diags) = analyze_externals(
            &index,
            &docs,
            &types,
            &BTreeMap::new(),
            ExternalCheckSeverity::Error,
            true, // manifest registered (has `actor_id`, but not `totally_bogus`)
        );
        assert_eq!(
            diags.len(),
            1,
            "manifest present: unknown type still errors"
        );
        assert_eq!(diags[0].code, DiagnosticCode::E040);
    }

    #[test]
    fn arity_disagreement_emits_e039() {
        let index = index_with_external("has", &["item"]); // ink: 1 param
        let reg_ext = ManifestExternal {
            name: "has".to_string(),
            params: vec![
                ManifestParam {
                    name: "item".to_string(),
                    ty: TypeRef("string".to_string()),
                },
                ManifestParam {
                    name: "qty".to_string(),
                    ty: TypeRef("int".to_string()),
                },
            ],
            returns: TypeRef::default(),
            kind: ExternalKind::default(),
            doc: None,

            widgets: vec![],
            path: Vec::new(),
        };
        let mut registered = BTreeMap::new();
        registered.insert("has".to_string(), &reg_ext);

        let (_metas, diags) = analyze_externals(
            &index,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &registered,
            ExternalCheckSeverity::Error,
            true,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E039);
    }

    #[test]
    fn severity_off_suppresses_diagnostics_but_keeps_meta() {
        let index = index_with_external("foo", &["x"]);
        let mut docs = BTreeMap::new();
        docs.insert(
            (SymbolKind::External, "foo".to_string()),
            inline(&[("x", "bogus")], None, None),
        );

        let (metas, diags) = analyze_externals(
            &index,
            &docs,
            &BTreeMap::new(),
            &BTreeMap::new(),
            ExternalCheckSeverity::Off,
            true,
        );
        assert!(diags.is_empty(), "Off suppresses diagnostics");
        assert!(!metas.is_empty(), "enrichment still built when Off");
    }

    // ── Callable (knot/stitch) doc enrichment ────────────────────

    /// An index with one function knot and one (qualified) stitch.
    fn index_with_callables() -> SymbolIndex {
        let mut m = SymbolManifest::default();
        m.knots.push(DeclaredSymbol {
            name: "damage".to_string(),
            range: TextRange::new(TextSize::new(0), TextSize::new(6)),
            params: vec![ParamInfo {
                name: "weapon".to_string(),
                is_ref: false,
                is_divert: false,
            }],
            detail: Some("function".to_string()),
            visibility: None,
            was: None,
        });
        m.stitches.push(DeclaredSymbol {
            name: "hub.market".to_string(),
            range: TextRange::new(TextSize::new(10), TextSize::new(16)),
            params: Vec::new(),
            detail: None,
            visibility: None,
            was: None,
        });
        merge_manifests(&[(FileId(0), &m)]).0
    }

    fn meta_for_kind<'a>(
        metas: &'a BTreeMap<DefinitionId, SymbolMeta>,
        index: &SymbolIndex,
        kind: SymbolKind,
        name: &str,
    ) -> &'a SymbolMeta {
        let id = index
            .symbols
            .values()
            .find(|s| s.kind == kind && s.name == name)
            .expect("symbol in index")
            .id;
        metas.get(&id).expect("meta for symbol")
    }

    #[test]
    fn knot_doc_enriches_meta_with_resolved_types() {
        let index = index_with_callables();
        let mut docs = BTreeMap::new();
        docs.insert(
            (SymbolKind::Knot, "damage".to_string()),
            DocBlock {
                doc: Some("Damage roll.".to_string()),
                params: vec![("weapon".to_string(), TypeRef("item_id".to_string()))],
                returns: Some(TypeRef("int".to_string())),
                kind: None,
            },
        );
        let mut types = BTreeMap::new();
        types.insert(
            "item_id".to_string(),
            SemanticTypeDef {
                name: "item_id".to_string(),
                base: BaseType::String,
                constraint: None,
                values: None,
                widget: None,
            },
        );

        let (metas, diags) =
            enrich_callables(&index, &docs, &types, ExternalCheckSeverity::Error, true);
        assert!(diags.is_empty(), "known types: {diags:?}");
        let meta = meta_for_kind(&metas, &index, SymbolKind::Knot, "damage");
        assert_eq!(meta.doc.as_deref(), Some("Damage roll."));
        assert_eq!(meta.kind, ExternalKind::Plain);
        assert_eq!(
            meta.params[0].ty.as_ref().and_then(|t| t.base),
            Some(BaseType::String)
        );
        assert_eq!(
            meta.returns.as_ref().and_then(|t| t.base),
            Some(BaseType::Int)
        );
    }

    #[test]
    fn stitch_doc_keyed_by_qualified_name() {
        let index = index_with_callables();
        let mut docs = BTreeMap::new();
        docs.insert(
            (SymbolKind::Stitch, "hub.market".to_string()),
            DocBlock {
                doc: Some("The market square.".to_string()),
                params: Vec::new(),
                returns: None,
                kind: None,
            },
        );
        let (metas, diags) = enrich_callables(
            &index,
            &docs,
            &BTreeMap::new(),
            ExternalCheckSeverity::Error,
            true,
        );
        assert!(diags.is_empty());
        let meta = meta_for_kind(&metas, &index, SymbolKind::Stitch, "hub.market");
        assert_eq!(meta.doc.as_deref(), Some("The market square."));
    }

    #[test]
    fn unknown_semantic_type_on_knot_emits_e040() {
        let index = index_with_callables();
        let mut docs = BTreeMap::new();
        docs.insert(
            (SymbolKind::Knot, "damage".to_string()),
            DocBlock {
                doc: None,
                params: vec![("weapon".to_string(), TypeRef("bogus".to_string()))],
                returns: None,
                kind: None,
            },
        );
        let (metas, diags) = enrich_callables(
            &index,
            &docs,
            &BTreeMap::new(),
            ExternalCheckSeverity::Error,
            true,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E040);
        // Meta still built with an unresolved param type.
        let meta = meta_for_kind(&metas, &index, SymbolKind::Knot, "damage");
        assert!(meta.params[0].ty.as_ref().is_some_and(|t| t.base.is_none()));

        // Severity Off keeps the meta but suppresses the diagnostic.
        let (metas, diags) = enrich_callables(
            &index,
            &docs,
            &BTreeMap::new(),
            ExternalCheckSeverity::Off,
            true,
        );
        assert!(diags.is_empty());
        assert!(!metas.is_empty());
    }

    /// #339: with no manifest registered, an unresolved semantic type on a
    /// knot/stitch doc tag is tolerated as opaque too (callables share the
    /// same vocabulary as externals).
    #[test]
    fn unknown_semantic_type_on_knot_tolerated_without_manifest() {
        let index = index_with_callables();
        let mut docs = BTreeMap::new();
        docs.insert(
            (SymbolKind::Knot, "damage".to_string()),
            DocBlock {
                doc: None,
                params: vec![("weapon".to_string(), TypeRef("item_id".to_string()))],
                returns: None,
                kind: None,
            },
        );
        let (metas, diags) = enrich_callables(
            &index,
            &docs,
            &BTreeMap::new(),
            ExternalCheckSeverity::Error,
            false, // no manifest registered
        );
        assert!(
            diags.is_empty(),
            "no manifest: tolerated, no E040: {diags:?}"
        );
        let meta = meta_for_kind(&metas, &index, SymbolKind::Knot, "damage");
        assert!(meta.params[0].ty.as_ref().is_some_and(|t| t.base.is_none()));
    }

    #[test]
    fn undocumented_callables_get_no_meta() {
        let index = index_with_callables();
        let (metas, diags) = enrich_callables(
            &index,
            &BTreeMap::new(),
            &BTreeMap::new(),
            ExternalCheckSeverity::Error,
            true,
        );
        assert!(metas.is_empty());
        assert!(diags.is_empty());
    }

    // ── VAR/CONST/LIST value metadata ────────────────────────────

    /// Parse, lower, and fully analyze a single source file.
    fn analyze_source(src: &str) -> crate::AnalysisResult {
        let parsed = brink_syntax::parse(src);
        let tree = parsed.tree();
        let (hir, manifest, diags) = brink_ir::hir::lower(FileId(0), &tree);
        assert!(diags.is_empty(), "lowering diagnostics: {diags:?}");
        crate::analyze(&[(FileId(0), &hir, &manifest)])
    }

    fn meta_by_name<'a>(
        result: &'a crate::AnalysisResult,
        kind: SymbolKind,
        name: &str,
    ) -> &'a SymbolMeta {
        let id = result
            .index
            .symbols
            .values()
            .find(|s| s.kind == kind && s.name == name)
            .expect("symbol in index")
            .id;
        result.symbol_meta.get(&id).expect("meta for symbol")
    }

    #[test]
    fn var_initializer_types_are_inferred() {
        let result = analyze_source(
            "VAR health = 100\nVAR speed = 0.5\nVAR alive = true\nVAR name = \"Ada\"\n",
        );
        let ty = |name: &str| {
            meta_by_name(&result, SymbolKind::Variable, name)
                .value
                .as_ref()
                .expect("value meta")
                .ty
                .clone()
        };
        assert_eq!(ty("health"), Some(InferredType::Int));
        assert_eq!(ty("speed"), Some(InferredType::Float));
        assert_eq!(ty("alive"), Some(InferredType::Bool));
        assert_eq!(ty("name"), Some(InferredType::String));
        // VARs never get display values — only CONSTs do.
        assert!(
            meta_by_name(&result, SymbolKind::Variable, "health")
                .value
                .as_ref()
                .is_some_and(|v| v.value_text.is_none())
        );
    }

    #[test]
    fn const_gets_type_and_display_value() {
        let result = analyze_source(
            "CONST SPEED = 0.5\nCONST LIVES = -3\nCONST NAME = \"Ada\"\nCONST WHOLE = 1.0\n",
        );
        let value = |name: &str| {
            meta_by_name(&result, SymbolKind::Constant, name)
                .value
                .clone()
                .expect("value meta")
        };
        assert_eq!(value("SPEED").ty, Some(InferredType::Float));
        assert_eq!(value("SPEED").value_text.as_deref(), Some("0.5"));
        assert_eq!(value("LIVES").ty, Some(InferredType::Int));
        assert_eq!(value("LIVES").value_text.as_deref(), Some("-3"));
        assert_eq!(value("NAME").value_text.as_deref(), Some("\"Ada\""));
        assert_eq!(
            value("WHOLE").value_text.as_deref(),
            Some("1.0"),
            "whole floats keep a trailing .0"
        );
    }

    #[test]
    fn docs_attach_to_values_and_lists() {
        let result = analyze_source(
            "/// Player health.\nVAR health = 100\n/// Mood states.\nLIST mood = happy, sad\n",
        );
        assert_eq!(
            meta_by_name(&result, SymbolKind::Variable, "health")
                .doc
                .as_deref(),
            Some("Player health.")
        );
        let list_meta = meta_by_name(&result, SymbolKind::List, "mood");
        assert_eq!(list_meta.doc.as_deref(), Some("Mood states."));
        assert!(list_meta.value.is_none(), "lists carry docs only");
    }

    #[test]
    fn divert_target_initializer_infers_divert() {
        let result = analyze_source("VAR exit = -> hub\n== hub ==\ntext\n-> DONE\n");
        assert_eq!(
            meta_by_name(&result, SymbolKind::Variable, "exit")
                .value
                .as_ref()
                .and_then(|v| v.ty.clone()),
            Some(InferredType::Divert)
        );
    }

    #[test]
    fn list_literal_var_infers_the_declaring_list_name() {
        // Issue #628: a VAR initialized directly to a list literal must keep
        // the nominal LIST identity through the phase-0 `Sig`
        // stub/`SymbolMeta` enrichment, not collapse to a bare "list".
        let result = analyze_source("LIST Weathers = sunny, rainy, snowy\nVAR w = (sunny)\n");
        assert_eq!(
            meta_by_name(&result, SymbolKind::Variable, "w")
                .value
                .as_ref()
                .and_then(|v| v.ty.clone()),
            Some(InferredType::List("Weathers".to_string()))
        );
    }

    #[test]
    fn list_literal_var_infers_the_list_name_when_qualified() {
        let result = analyze_source(
            "LIST Weathers = sunny, rainy\nVAR w = (Weathers.sunny, Weathers.rainy)\n",
        );
        assert_eq!(
            meta_by_name(&result, SymbolKind::Variable, "w")
                .value
                .as_ref()
                .and_then(|v| v.ty.clone()),
            Some(InferredType::List("Weathers".to_string()))
        );
    }

    // ── Call-site literal checks (E041, E042) ────────────────────

    use brink_ir::hir::{
        Block, Expr, HirFile, Name, Path as HirPath, Stmt, StringExpr, StringPart,
    };

    fn rng() -> TextRange {
        TextRange::new(TextSize::new(0), TextSize::new(1))
    }

    /// A HIR file whose root content is a single `~ name(args)` expression statement.
    fn hir_calling(name: &str, args: Vec<Expr>) -> HirFile {
        let path = HirPath {
            segments: vec![Name {
                text: name.to_string(),
                range: rng(),
            }],
            range: rng(),
        };
        HirFile {
            root_content: Block::from_stmts(vec![Stmt::ExprStmt(Expr::Call(path, args))]),
            knots: Vec::new(),
            variables: Vec::new(),
            constants: Vec::new(),
            lists: Vec::new(),
            structs: Vec::new(),
            externals: Vec::new(),
            includes: Vec::new(),
            module: None,
            imports: Vec::new(),
            visibility: Vec::new(),
            was_directives: Vec::new(),
            allow_scopes: Vec::new(),
            element_matches: Vec::new(),
            cue_names: Vec::new(),
            native: false,
            claim_handlers: Vec::new(),
        }
    }

    fn typed_meta(ty: ResolvedType) -> SymbolMeta {
        SymbolMeta {
            doc: None,
            kind: ExternalKind::default(),
            returns: None,
            params: vec![ResolvedParam {
                name: "x".to_string(),
                ty: Some(ty),
            }],
            value: None,
            group_widgets: Vec::new(),
        }
    }

    fn run_call_check(call: &str, args: Vec<Expr>, meta: &SymbolMeta) -> Vec<Diagnostic> {
        let hir = hir_calling(call, args);
        let mut n2m: BTreeMap<&str, &SymbolMeta> = BTreeMap::new();
        n2m.insert(call, meta);
        check_call_sites(&[(FileId(0), &hir)], &n2m)
    }

    fn string_lit(s: &str) -> Expr {
        Expr::String(StringExpr {
            parts: vec![StringPart::Literal(s.to_string())],
        })
    }

    #[test]
    fn type_mismatch_emits_e041() {
        let meta = typed_meta(ResolvedType {
            name: "string".to_string(),
            base: Some(BaseType::String),
            constraint: None,
            values: None,
            widget: None,
        });
        let diags = run_call_check("tint", vec![Expr::Int(5)], &meta);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E041);
    }

    #[test]
    fn matching_literal_no_diagnostic() {
        let meta = typed_meta(ResolvedType {
            name: "string".to_string(),
            base: Some(BaseType::String),
            constraint: None,
            values: None,
            widget: None,
        });
        let diags = run_call_check("tint", vec![string_lit("ok")], &meta);
        assert!(diags.is_empty(), "matching string literal: {diags:?}");
    }

    #[test]
    fn int_widens_to_float_param() {
        let meta = typed_meta(ResolvedType {
            name: "float".to_string(),
            base: Some(BaseType::Float),
            constraint: None,
            values: None,
            widget: None,
        });
        let diags = run_call_check("scale", vec![Expr::Int(3)], &meta);
        assert!(
            diags.is_empty(),
            "int literal accepted for float param: {diags:?}"
        );
    }

    #[test]
    fn non_literal_arg_skipped() {
        // A variable reference is not a literal — never flagged (literals-only).
        let meta = typed_meta(ResolvedType {
            name: "string".to_string(),
            base: Some(BaseType::String),
            constraint: None,
            values: None,
            widget: None,
        });
        let var = Expr::Path(HirPath {
            segments: vec![Name {
                text: "v".to_string(),
                range: rng(),
            }],
            range: rng(),
        });
        let diags = run_call_check("tint", vec![var], &meta);
        assert!(
            diags.is_empty(),
            "non-literal arg is not checked: {diags:?}"
        );
    }

    #[test]
    fn enum_violation_emits_e042() {
        let meta = typed_meta(ResolvedType {
            name: "item_id".to_string(),
            base: Some(BaseType::String),
            constraint: Some(Constraint::Enum {
                values: vec!["sword".into(), "shield".into()],
            }),
            values: None,
            widget: None,
        });
        let bad = run_call_check("give", vec![string_lit("banana")], &meta);
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].code, DiagnosticCode::E042);
        let ok = run_call_check("give", vec![string_lit("sword")], &meta);
        assert!(ok.is_empty(), "valid enum value: {ok:?}");
    }

    #[test]
    fn range_violation_emits_e042() {
        let meta = typed_meta(ResolvedType {
            name: "percent".to_string(),
            base: Some(BaseType::Int),
            constraint: Some(Constraint::Range {
                min: Some(0),
                max: Some(100),
            }),
            values: None,
            widget: None,
        });
        let bad = run_call_check("set", vec![Expr::Int(150)], &meta);
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].code, DiagnosticCode::E042);
        let ok = run_call_check("set", vec![Expr::Int(50)], &meta);
        assert!(ok.is_empty(), "in-range value: {ok:?}");
    }
}
