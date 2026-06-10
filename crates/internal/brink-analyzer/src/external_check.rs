//! Host-capability-manifest enrichment + checks over external functions.
//!
//! Merges inline `///` doc-comments (parsed during HIR lowering) with the
//! registered [`HostManifest`] into per-external [`ExternalMeta`] (keyed by
//! `DefinitionId`), and emits manifest-driven diagnostics. Tooling /
//! author-time only — the runtime and codegen never see any of this.
//!
//! The enrichment map is always built (the IDE consumes it for hover /
//! signature even with checks disabled); only the *diagnostics* are gated by
//! the [`ExternalCheckSeverity`] policy.

use std::collections::BTreeMap;

use brink_format::DefinitionId;
use brink_ir::hir::{
    Block, ChoiceSet, CondKind, Conditional, Content, ContentPart, DivertTarget, Expr, HirFile,
    Path, Sequence, Stmt, StringPart,
};
use brink_ir::{
    BaseType, Constraint, Diagnostic, DiagnosticCode, ExternalDoc, ExternalKind, FileId,
    SemanticTypeDef, SymbolIndex, SymbolInfo, SymbolKind, TypeRef,
};

/// Severity policy for manifest-driven external checks. Configurable as a
/// compiler/IDE flag; defaults to `Error` (a registered manifest is binding).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExternalCheckSeverity {
    /// Emit manifest-driven diagnostics (default).
    #[default]
    Error,
    /// Suppress manifest-driven diagnostics (enrichment is still built).
    Off,
}

/// Per-external merged metadata, surfaced to the IDE and used by the
/// call-site checks. Keyed by the external's `DefinitionId` on the
/// `AnalysisResult`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalMeta {
    /// Free-text documentation.
    pub doc: Option<String>,
    /// Presentation/effect category (informational).
    pub kind: ExternalKind,
    /// Resolved return type, if specified.
    pub returns: Option<ResolvedType>,
    /// Resolved parameter types, by ink declaration order.
    pub params: Vec<ResolvedParam>,
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
}

/// Build the per-external enrichment map and collect manifest-driven
/// diagnostics. The map is always returned; diagnostics are empty when the
/// severity policy is `Off`.
pub fn analyze_externals(
    index: &SymbolIndex,
    inline_docs: &BTreeMap<String, ExternalDoc>,
    types: &BTreeMap<String, SemanticTypeDef>,
    registered: &BTreeMap<String, &brink_ir::ManifestExternal>,
    severity: ExternalCheckSeverity,
) -> (BTreeMap<DefinitionId, ExternalMeta>, Vec<Diagnostic>) {
    let mut metas: BTreeMap<DefinitionId, ExternalMeta> = BTreeMap::new();
    let mut diags: Vec<Diagnostic> = Vec::new();

    // Deterministic order for diagnostics: sort externals by (file, offset).
    let mut externals: Vec<&SymbolInfo> = index
        .symbols
        .values()
        .filter(|info| info.kind == SymbolKind::External)
        .collect();
    externals.sort_by_key(|info| (info.file.0, info.range.start()));

    for info in externals {
        let inline = inline_docs.get(&info.name);
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
            let ty = tref.and_then(|t| resolve_type(t, types, info, &mut diags));
            params.push(ResolvedParam {
                name: p.name.clone(),
                ty,
            });
        }

        // Return type, kind, doc: inline wins, else registered.
        let returns = inline
            .and_then(|d| d.returns.as_ref())
            .or_else(|| reg.map(|r| &r.returns))
            .and_then(|t| resolve_type(t, types, info, &mut diags));
        let kind = inline
            .and_then(|d| d.kind)
            .or_else(|| reg.map(|r| r.kind))
            .unwrap_or_default();
        let doc = inline
            .and_then(|d| d.doc.clone())
            .or_else(|| reg.and_then(|r| r.doc.clone()));

        metas.insert(
            info.id,
            ExternalMeta {
                doc,
                kind,
                returns,
                params,
            },
        );
    }

    if severity == ExternalCheckSeverity::Off {
        diags.clear();
    }
    (metas, diags)
}

/// Resolve a [`TypeRef`] to a [`ResolvedType`], emitting E040 for an unknown
/// semantic type. Returns `None` for an unspecified (empty) ref.
fn resolve_type(
    t: &TypeRef,
    types: &BTreeMap<String, SemanticTypeDef>,
    info: &SymbolInfo,
    diags: &mut Vec<Diagnostic>,
) -> Option<ResolvedType> {
    if t.is_unspecified() {
        return None;
    }
    if let Some(base) = t.as_base() {
        return Some(ResolvedType {
            name: t.0.clone(),
            base: Some(base),
            constraint: None,
        });
    }
    if let Some(def) = types.get(t.0.trim()) {
        return Some(ResolvedType {
            name: t.0.clone(),
            base: Some(def.base),
            constraint: def.constraint.clone(),
        });
    }
    diags.push(Diagnostic {
        file: info.file,
        range: info.range,
        message: format!(
            "{}: `{}` (on external `{}`)",
            DiagnosticCode::E040.title(),
            t.0.trim(),
            info.name,
        ),
        code: DiagnosticCode::E040,
    });
    Some(ResolvedType {
        name: t.0.clone(),
        base: None,
        constraint: None,
    })
}

// ─── Call-site literal checks (E041 type, E042 closed-domain) ───────────

/// Walk the HIR and check literal arguments at external call sites against the
/// merged [`ExternalMeta`]. Only literal arguments are checked (ink is
/// dynamically typed); non-literals and untyped params are skipped, so there
/// are no false positives. Returns the diagnostics (caller gates on severity).
pub fn check_call_sites(
    files: &[(FileId, &HirFile)],
    name_to_meta: &BTreeMap<&str, &ExternalMeta>,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    if name_to_meta.is_empty() {
        return diags;
    }
    for &(file_id, hir) in files {
        let mut visit = |path: &Path, args: &[Expr]| {
            check_call(file_id, path, args, name_to_meta, &mut diags);
        };
        walk_block(&hir.root_content, &mut visit);
        for knot in &hir.knots {
            walk_block(&knot.body, &mut visit);
            for stitch in &knot.stitches {
                walk_block(&stitch.body, &mut visit);
            }
        }
    }
    diags
}

fn walk_block(block: &Block, visit: &mut dyn FnMut(&Path, &[Expr])) {
    for stmt in &block.stmts {
        walk_stmt(stmt, visit);
    }
}

fn walk_stmt(stmt: &Stmt, visit: &mut dyn FnMut(&Path, &[Expr])) {
    match stmt {
        Stmt::Content(c) => walk_content(c, visit),
        Stmt::Divert(d) => walk_target(&d.target, visit),
        Stmt::TunnelCall(t) => {
            for target in &t.targets {
                walk_target(target, visit);
            }
        }
        Stmt::ThreadStart(t) => walk_target(&t.target, visit),
        Stmt::TempDecl(t) => {
            if let Some(e) = &t.value {
                walk_expr(e, visit);
            }
        }
        Stmt::Assignment(a) => walk_expr(&a.value, visit),
        Stmt::Return(r) => {
            if let Some(e) = &r.value {
                walk_expr(e, visit);
            }
            for e in &r.onwards_args {
                walk_expr(e, visit);
            }
        }
        Stmt::ChoiceSet(cs) => walk_choice_set(cs, visit),
        Stmt::LabeledBlock(b) => walk_block(b, visit),
        Stmt::Conditional(c) => walk_conditional(c, visit),
        Stmt::Sequence(s) => walk_sequence(s, visit),
        Stmt::ExprStmt(e) => walk_expr(e, visit),
        Stmt::EndOfLine => {}
    }
}

fn walk_target(target: &DivertTarget, visit: &mut dyn FnMut(&Path, &[Expr])) {
    for e in &target.args {
        walk_expr(e, visit);
    }
}

fn walk_content(content: &Content, visit: &mut dyn FnMut(&Path, &[Expr])) {
    for part in &content.parts {
        match part {
            ContentPart::Interpolation(e) => walk_expr(e, visit),
            ContentPart::InlineConditional(c) => walk_conditional(c, visit),
            ContentPart::InlineSequence(s) => walk_sequence(s, visit),
            ContentPart::Text(_) | ContentPart::Glue | ContentPart::Spring => {}
        }
    }
}

fn walk_conditional(cond: &Conditional, visit: &mut dyn FnMut(&Path, &[Expr])) {
    if let CondKind::Switch(e) = &cond.kind {
        walk_expr(e, visit);
    }
    for branch in &cond.branches {
        if let Some(e) = &branch.condition {
            walk_expr(e, visit);
        }
        walk_block(&branch.body, visit);
    }
}

fn walk_sequence(seq: &Sequence, visit: &mut dyn FnMut(&Path, &[Expr])) {
    for branch in &seq.branches {
        walk_block(branch, visit);
    }
}

fn walk_choice_set(cs: &ChoiceSet, visit: &mut dyn FnMut(&Path, &[Expr])) {
    for choice in &cs.choices {
        if let Some(e) = &choice.condition {
            walk_expr(e, visit);
        }
        for content in [
            &choice.start_content,
            &choice.bracket_content,
            &choice.inner_content,
        ]
        .into_iter()
        .flatten()
        {
            walk_content(content, visit);
        }
        walk_block(&choice.body, visit);
    }
    walk_block(&cs.continuation, visit);
}

fn walk_expr(expr: &Expr, visit: &mut dyn FnMut(&Path, &[Expr])) {
    match expr {
        Expr::Call(path, args) => {
            visit(path, args);
            for arg in args {
                walk_expr(arg, visit);
            }
        }
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => walk_expr(inner, visit),
        Expr::Infix(lhs, _, rhs) => {
            walk_expr(lhs, visit);
            walk_expr(rhs, visit);
        }
        Expr::String(s) => {
            for part in &s.parts {
                if let StringPart::Interpolation(e) = part {
                    walk_expr(e, visit);
                }
            }
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Path(_)
        | Expr::DivertTarget(_)
        | Expr::ListLiteral(_) => {}
    }
}

fn check_call(
    file: FileId,
    path: &Path,
    args: &[Expr],
    name_to_meta: &BTreeMap<&str, &ExternalMeta>,
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
        // pattern is still surfaced to the IDE via ExternalMeta.
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
        DeclaredSymbol, ExternalDoc, ManifestExternal, ManifestParam, ParamInfo, SemanticTypeDef,
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
        });
        merge_manifests(&[(FileId(0), &m)]).0
    }

    fn meta_for<'a>(
        metas: &'a BTreeMap<DefinitionId, ExternalMeta>,
        index: &SymbolIndex,
        name: &str,
    ) -> &'a ExternalMeta {
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
    ) -> ExternalDoc {
        ExternalDoc {
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
            "has".to_string(),
            inline(&[("item", "bool")], Some("bool"), Some(ExternalKind::Query)),
        );
        let (metas, diags) = analyze_externals(
            &index,
            &docs,
            &BTreeMap::new(),
            &BTreeMap::new(),
            ExternalCheckSeverity::Error,
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
        };
        let mut registered = BTreeMap::new();
        registered.insert("grant".to_string(), &reg_ext);
        let (metas, _) = analyze_externals(
            &index,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &registered,
            ExternalCheckSeverity::Error,
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
        };
        let mut registered = BTreeMap::new();
        registered.insert("has".to_string(), &reg_ext);
        let mut docs = BTreeMap::new();
        docs.insert(
            "has".to_string(),
            inline(&[("item", "bool")], Some("bool"), Some(ExternalKind::Query)),
        );

        let (metas, _) = analyze_externals(
            &index,
            &docs,
            &BTreeMap::new(),
            &registered,
            ExternalCheckSeverity::Error,
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
            },
        );
        let mut docs = BTreeMap::new();
        docs.insert(
            "give".to_string(),
            inline(&[("item", "item_id")], None, None),
        );

        let (metas, diags) = analyze_externals(
            &index,
            &docs,
            &types,
            &BTreeMap::new(),
            ExternalCheckSeverity::Error,
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
        docs.insert("foo".to_string(), inline(&[("x", "bogus")], None, None));

        let (metas, diags) = analyze_externals(
            &index,
            &docs,
            &BTreeMap::new(),
            &BTreeMap::new(),
            ExternalCheckSeverity::Error,
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
        };
        let mut registered = BTreeMap::new();
        registered.insert("has".to_string(), &reg_ext);

        let (_metas, diags) = analyze_externals(
            &index,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &registered,
            ExternalCheckSeverity::Error,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E039);
    }

    #[test]
    fn severity_off_suppresses_diagnostics_but_keeps_meta() {
        let index = index_with_external("foo", &["x"]);
        let mut docs = BTreeMap::new();
        docs.insert("foo".to_string(), inline(&[("x", "bogus")], None, None));

        let (metas, diags) = analyze_externals(
            &index,
            &docs,
            &BTreeMap::new(),
            &BTreeMap::new(),
            ExternalCheckSeverity::Off,
        );
        assert!(diags.is_empty(), "Off suppresses diagnostics");
        assert!(!metas.is_empty(), "enrichment still built when Off");
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
            root_content: Block {
                label: None,
                stmts: vec![Stmt::ExprStmt(Expr::Call(path, args))],
                container_id: None,
            },
            knots: Vec::new(),
            variables: Vec::new(),
            constants: Vec::new(),
            lists: Vec::new(),
            externals: Vec::new(),
            includes: Vec::new(),
        }
    }

    fn typed_meta(ty: ResolvedType) -> ExternalMeta {
        ExternalMeta {
            doc: None,
            kind: ExternalKind::default(),
            returns: None,
            params: vec![ResolvedParam {
                name: "x".to_string(),
                ty: Some(ty),
            }],
        }
    }

    fn run_call_check(call: &str, args: Vec<Expr>, meta: &ExternalMeta) -> Vec<Diagnostic> {
        let hir = hir_calling(call, args);
        let mut n2m: BTreeMap<&str, &ExternalMeta> = BTreeMap::new();
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
        });
        let bad = run_call_check("set", vec![Expr::Int(150)], &meta);
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].code, DiagnosticCode::E042);
        let ok = run_call_check("set", vec![Expr::Int(50)], &meta);
        assert!(ok.is_empty(), "in-range value: {ok:?}");
    }
}
