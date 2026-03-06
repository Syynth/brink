use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use brink_format::{DefinitionId, DefinitionTag};
use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, SymbolIndex, SymbolInfo, SymbolKind, SymbolManifest,
};

/// Merge per-file symbol manifests into a unified symbol index.
///
/// Returns the index and any diagnostics (e.g. duplicate definitions).
pub fn merge_manifests(files: &[(FileId, SymbolManifest)]) -> (SymbolIndex, Vec<Diagnostic>) {
    let mut index = SymbolIndex::default();
    let mut diagnostics = Vec::new();

    for (file_id, manifest) in files {
        for sym in &manifest.knots {
            insert_symbol(
                &mut index,
                &mut diagnostics,
                *file_id,
                sym,
                SymbolKind::Knot,
                DiagnosticCode::E022,
            );
        }
        for sym in &manifest.stitches {
            insert_symbol(
                &mut index,
                &mut diagnostics,
                *file_id,
                sym,
                SymbolKind::Stitch,
                DiagnosticCode::E022,
            );
        }
        for sym in &manifest.variables {
            insert_symbol(
                &mut index,
                &mut diagnostics,
                *file_id,
                sym,
                SymbolKind::Variable,
                DiagnosticCode::E023,
            );
        }
        for sym in &manifest.lists {
            insert_symbol(
                &mut index,
                &mut diagnostics,
                *file_id,
                sym,
                SymbolKind::List,
                DiagnosticCode::E023,
            );
        }
        for sym in &manifest.externals {
            insert_symbol(
                &mut index,
                &mut diagnostics,
                *file_id,
                sym,
                SymbolKind::External,
                DiagnosticCode::E023,
            );
        }
        for sym in &manifest.labels {
            insert_symbol(
                &mut index,
                &mut diagnostics,
                *file_id,
                sym,
                SymbolKind::Label,
                DiagnosticCode::E022,
            );
        }
        for sym in &manifest.list_items {
            insert_symbol(
                &mut index,
                &mut diagnostics,
                *file_id,
                sym,
                SymbolKind::ListItem,
                DiagnosticCode::E026,
            );
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
    // Check for duplicates of the same kind
    if let Some(existing_ids) = index.by_name.get(&sym.name) {
        let has_dup = existing_ids
            .iter()
            .any(|id| index.symbols.get(id).is_some_and(|info| info.kind == kind));
        if has_dup {
            diagnostics.push(Diagnostic {
                file,
                range: sym.range,
                message: format!("{}: `{}` is already defined", dup_code.title(), sym.name),
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
        },
    );
    index.by_name.entry(sym.name.clone()).or_default().push(id);
}

fn hash_name(name: &str, tag: DefinitionTag) -> u64 {
    let mut hasher = DefaultHasher::new();
    tag.hash(&mut hasher);
    name.hash(&mut hasher);
    hasher.finish()
}
