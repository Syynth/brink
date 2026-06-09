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
use brink_ir::{
    BaseType, Constraint, Diagnostic, DiagnosticCode, ExternalDoc, ExternalKind, SemanticTypeDef,
    SymbolIndex, SymbolInfo, SymbolKind, TypeRef,
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
}
