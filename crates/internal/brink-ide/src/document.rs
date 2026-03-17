use std::collections::BTreeSet;

use brink_analyzer::AnalysisResult;
use brink_ir::{FileId, HirFile, SymbolKind, SymbolManifest};
use rowan::TextRange;

/// A document symbol (outline entry) with optional children.
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub detail: Option<String>,
    pub range: TextRange,
    pub children: Vec<DocumentSymbol>,
}

/// A workspace-wide symbol search result.
pub struct WorkspaceSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file: FileId,
    pub range: TextRange,
}

/// Compute document symbols (outline) for a single file.
pub fn document_symbols(hir: &HirFile, manifest: &SymbolManifest) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();

    // Knots with their stitches as children
    for knot in &hir.knots {
        let children: Vec<_> = knot
            .stitches
            .iter()
            .map(|stitch| DocumentSymbol {
                name: stitch.name.text.clone(),
                kind: SymbolKind::Stitch,
                detail: None,
                range: stitch.name.range,
                children: Vec::new(),
            })
            .collect();

        let sym = DocumentSymbol {
            name: knot.name.text.clone(),
            kind: SymbolKind::Knot,
            detail: if knot.is_function {
                Some("function".to_owned())
            } else {
                None
            },
            range: knot.name.range,
            children,
        };
        symbols.push(sym);
    }

    // Top-level declarations from manifest
    let decl_groups: &[(&[brink_ir::DeclaredSymbol], SymbolKind)] = &[
        (&manifest.variables, SymbolKind::Variable),
        (&manifest.lists, SymbolKind::List),
        (&manifest.externals, SymbolKind::External),
    ];

    for (decls, kind) in decl_groups {
        for decl in *decls {
            symbols.push(DocumentSymbol {
                name: decl.name.clone(),
                kind: *kind,
                detail: None,
                range: decl.range,
                children: Vec::new(),
            });
        }
    }

    symbols
}

/// Search workspace symbols across all analysis results.
///
/// Deduplicates by `(FileId, TextRange)` using a `BTreeSet` for determinism.
pub fn workspace_symbols<'a>(
    analyses: impl Iterator<Item = &'a AnalysisResult>,
    query: &str,
) -> Vec<WorkspaceSymbol> {
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();
    let mut seen = BTreeSet::new();

    for analysis in analyses {
        for info in analysis.index.symbols.values() {
            if !query_lower.is_empty() && !info.name.to_lowercase().contains(&query_lower) {
                continue;
            }

            if !seen.insert((info.file.0, info.range.start(), info.range.end())) {
                continue;
            }

            results.push(WorkspaceSymbol {
                name: info.name.clone(),
                kind: info.kind,
                file: info.file,
                range: info.range,
            });
        }
    }

    results
}
