use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use brink_format::{DefinitionId, DefinitionTag};
use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, LocalSymbol, SymbolIndex, SymbolInfo, SymbolKind,
    SymbolManifest,
};

/// Merge per-file symbol manifests into a unified symbol index.
///
/// Returns the index and any diagnostics (e.g. duplicate definitions).
pub fn merge_manifests(files: &[(FileId, &SymbolManifest)]) -> (SymbolIndex, Vec<Diagnostic>) {
    let mut index = SymbolIndex::default();
    let mut diagnostics = Vec::new();

    for &(file_id, manifest) in files {
        for sym in &manifest.knots {
            insert_symbol(
                &mut index,
                &mut diagnostics,
                file_id,
                sym,
                SymbolKind::Knot,
                DiagnosticCode::E022,
            );
        }
        for sym in &manifest.stitches {
            insert_symbol(
                &mut index,
                &mut diagnostics,
                file_id,
                sym,
                SymbolKind::Stitch,
                DiagnosticCode::E022,
            );
        }
        for sym in &manifest.variables {
            insert_symbol(
                &mut index,
                &mut diagnostics,
                file_id,
                sym,
                SymbolKind::Variable,
                DiagnosticCode::E023,
            );
        }
        for sym in &manifest.constants {
            insert_symbol(
                &mut index,
                &mut diagnostics,
                file_id,
                sym,
                SymbolKind::Constant,
                DiagnosticCode::E023,
            );
        }
        for sym in &manifest.lists {
            insert_symbol(
                &mut index,
                &mut diagnostics,
                file_id,
                sym,
                SymbolKind::List,
                DiagnosticCode::E023,
            );
        }
        for sym in &manifest.externals {
            insert_symbol(
                &mut index,
                &mut diagnostics,
                file_id,
                sym,
                SymbolKind::External,
                DiagnosticCode::E023,
            );
        }
        for sym in &manifest.labels {
            insert_symbol(
                &mut index,
                &mut diagnostics,
                file_id,
                sym,
                SymbolKind::Label,
                DiagnosticCode::E022,
            );
        }
        for sym in &manifest.list_items {
            insert_symbol(
                &mut index,
                &mut diagnostics,
                file_id,
                sym,
                SymbolKind::ListItem,
                DiagnosticCode::E026,
            );
        }
        for local in &manifest.locals {
            insert_local(&mut index, file_id, local);
        }
    }

    (index, diagnostics)
}

fn insert_symbol(
    index: &mut SymbolIndex,
    diagnostics: &mut Vec<Diagnostic>,
    file: FileId,
    sym: &brink_ir::DeclaredSymbol,
    kind: SymbolKind,
    dup_code: DiagnosticCode,
) {
    // Skip duplicates of the same kind — inklecate permits redefinition but we warn.
    if let Some(existing_ids) = index.by_name.get(&sym.name) {
        let has_dup = existing_ids
            .iter()
            .any(|id| index.symbols.get(id).is_some_and(|info| info.kind == kind));
        if has_dup {
            diagnostics.push(Diagnostic {
                file,
                range: sym.range,
                message: format!("{}: `{}`", dup_code.title(), sym.name),
                code: dup_code,
            });
            return;
        }
    }

    let tag = kind.definition_tag();
    let hash = hash_name(&sym.name, tag);
    let id = DefinitionId::new(tag, hash);

    index.symbols.insert(
        id,
        SymbolInfo {
            kind,
            file,
            range: sym.range,
            id,
            name: sym.name.clone(),
            params: sym.params.clone(),
            detail: sym.detail.clone(),
            scope: None,
            param_detail: None,
        },
    );
    index.by_name.entry(sym.name.clone()).or_default().push(id);

    // Warn if the symbol name shadows a built-in function.
    if matches!(
        kind,
        SymbolKind::Knot | SymbolKind::Variable | SymbolKind::Constant | SymbolKind::External
    ) && crate::resolve::is_builtin_function(&sym.name)
    {
        diagnostics.push(Diagnostic {
            file,
            range: sym.range,
            message: format!("{}: `{}`", DiagnosticCode::E035.title(), sym.name),
            code: DiagnosticCode::E035,
        });
    }
}

fn insert_local(index: &mut SymbolIndex, file: FileId, local: &LocalSymbol) {
    let tag = local.kind.definition_tag();
    // Scope-qualify the hash so identically-named locals in different
    // containers get distinct DefinitionIds.
    let scope_prefix = match (&local.scope.knot, &local.scope.stitch) {
        (Some(k), Some(s)) => format!("{k}.{s}."),
        (Some(k), None) => format!("{k}."),
        _ => String::new(),
    };
    let qualified = format!("{scope_prefix}{}", local.name);
    let hash = hash_name(&qualified, tag);
    let id = DefinitionId::new(tag, hash);

    index.symbols.insert(
        id,
        SymbolInfo {
            kind: local.kind,
            file,
            range: local.range,
            id,
            name: local.name.clone(),
            params: Vec::new(),
            detail: None,
            scope: Some(local.scope.clone()),
            param_detail: local.param_detail.clone(),
        },
    );
    index
        .by_name
        .entry(local.name.clone())
        .or_default()
        .push(id);
}

fn hash_name(name: &str, tag: DefinitionTag) -> u64 {
    let mut hasher = DefaultHasher::new();
    tag.hash(&mut hasher);
    name.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
#[expect(clippy::cast_possible_truncation, reason = "test helper ranges")]
mod tests {
    use brink_ir::{DeclaredSymbol, DiagnosticCode, FileId, SymbolManifest};
    use rowan::{TextRange, TextSize};

    use super::merge_manifests;

    fn range(offset: u32, len: u32) -> TextRange {
        TextRange::new(TextSize::new(offset), TextSize::new(offset + len))
    }

    fn sym(name: &str, offset: u32) -> DeclaredSymbol {
        DeclaredSymbol {
            name: name.to_string(),
            range: range(offset, name.len() as u32),
            params: Vec::new(),
            detail: None,
        }
    }

    #[test]
    fn duplicate_knot_emits_e022() {
        let mut m1 = SymbolManifest::default();
        m1.knots.push(sym("start", 0));

        let mut m2 = SymbolManifest::default();
        m2.knots.push(sym("start", 100));

        let files = vec![(FileId(0), &m1), (FileId(1), &m2)];
        let (_index, diags) = merge_manifests(&files);

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E022);
    }

    #[test]
    fn duplicate_variable_emits_e023() {
        let mut m1 = SymbolManifest::default();
        m1.variables.push(sym("score", 0));

        let mut m2 = SymbolManifest::default();
        m2.variables.push(sym("score", 100));

        let files = vec![(FileId(0), &m1), (FileId(1), &m2)];
        let (_index, diags) = merge_manifests(&files);

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E023);
    }

    #[test]
    fn different_kind_same_name_no_warning() {
        let mut manifest = SymbolManifest::default();
        manifest.knots.push(sym("thing", 0));
        manifest.variables.push(sym("thing", 100));

        let files = vec![(FileId(0), &manifest)];
        let (_index, diags) = merge_manifests(&files);

        // A knot and a variable with the same name are different kinds — no duplicate.
        assert!(diags.is_empty(), "expected no diagnostics: {diags:?}");
    }

    #[test]
    fn builtin_name_shadow_emits_e035() {
        let mut manifest = SymbolManifest::default();
        manifest.knots.push(sym("RANDOM", 0));

        let files = vec![(FileId(0), &manifest)];
        let (_index, diags) = merge_manifests(&files);

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E035);
    }

    #[test]
    fn non_builtin_name_no_shadow_warning() {
        let mut manifest = SymbolManifest::default();
        manifest.knots.push(sym("my_function", 0));

        let files = vec![(FileId(0), &manifest)];
        let (_index, diags) = merge_manifests(&files);

        assert!(diags.is_empty(), "expected no diagnostics: {diags:?}");
    }
}
