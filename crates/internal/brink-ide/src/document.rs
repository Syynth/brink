use std::collections::BTreeSet;

use brink_analyzer::AnalysisResult;
use brink_ir::{FileId, HirFile, SymbolKind, SymbolManifest};
use rowan::TextRange;

/// A document symbol (outline entry) with optional children.
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub detail: Option<String>,
    /// The range of the symbol name (selection range).
    pub range: TextRange,
    /// The full range of the symbol including its body.
    pub full_range: TextRange,
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
///
/// Full ranges are *ownership* ranges: a declaration's contiguous preceding
/// `///` doc block is included (per the decision log it is structurally part
/// of the declaration), and the trailing trivia a syntax node swallows up to
/// the next declaration is clamped back so the next declaration's doc block
/// is never claimed by its predecessor.
pub fn document_symbols(
    hir: &HirFile,
    manifest: &SymbolManifest,
    source: &str,
) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();

    // Knots with their stitches as children
    for (ki, knot) in hir.knots.iter().enumerate() {
        let knot_range = ownership_range(
            source,
            knot.ptr.text_range(),
            hir.knots.get(ki + 1).map(|n| n.ptr.text_range().start()),
        );

        let children: Vec<_> = knot
            .stitches
            .iter()
            .enumerate()
            .map(|(si, stitch)| {
                let next_start = knot
                    .stitches
                    .get(si + 1)
                    .map_or(knot_range.end(), |n| n.ptr.text_range().start());
                DocumentSymbol {
                    name: stitch.name.text.clone(),
                    kind: SymbolKind::Stitch,
                    detail: None,
                    range: stitch.name.range,
                    full_range: ownership_range(source, stitch.ptr.text_range(), Some(next_start)),
                    children: Vec::new(),
                }
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
            full_range: knot_range,
            children,
        };
        symbols.push(sym);
    }

    // Top-level declarations from manifest
    let decl_groups: &[(&[brink_ir::DeclaredSymbol], SymbolKind)] = &[
        (&manifest.variables, SymbolKind::Variable),
        (&manifest.constants, SymbolKind::Constant),
        (&manifest.lists, SymbolKind::List),
        (&manifest.structs, SymbolKind::Struct),
        (&manifest.externals, SymbolKind::External),
    ];

    for (decls, kind) in decl_groups {
        for decl in *decls {
            symbols.push(DocumentSymbol {
                name: decl.name.clone(),
                kind: *kind,
                detail: None,
                range: decl.range,
                full_range: decl.range,
                children: Vec::new(),
            });
        }
    }

    symbols
}

/// A declaration's ownership range: start extended backward over its attached
/// `///` doc block; end clamped before the next declaration's doc block (a
/// syntax node swallows all trivia up to the next declaration's header, so an
/// unclamped end would claim the next declaration's docs).
fn ownership_range(
    source: &str,
    syntax_range: TextRange,
    next_decl_start: Option<rowan::TextSize>,
) -> TextRange {
    let start = crate::doc_extended_start(source, syntax_range.start().into());
    let start = u32::try_from(start).unwrap_or(u32::MAX);
    let end = next_decl_start.map_or(syntax_range.end(), |next| {
        let next_owned = crate::doc_extended_start(source, next.into());
        syntax_range.end().min(rowan::TextSize::from(
            u32::try_from(next_owned).unwrap_or(u32::MAX),
        ))
    });
    TextRange::new(rowan::TextSize::from(start).min(end), end)
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

#[cfg(test)]
mod tests {
    use super::document_symbols;

    fn symbols_for(src: &str) -> Vec<super::DocumentSymbol> {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _) = brink_ir::hir::lower(brink_ir::FileId(0), &parsed.tree());
        document_symbols(&hir, &manifest, src)
    }

    #[test]
    fn knot_full_range_owns_its_docs_not_the_next_knots() {
        let src = "\
=== function carrying(item) ===
~ return pack ? item

/// Uniform random.
/// @returns {int}
=== function roll(lo, hi) ===
~ return 0
";
        let syms = symbols_for(src);
        let carrying = &syms[0];
        let roll = &syms[1];

        let carrying_text =
            &src[usize::from(carrying.full_range.start())..usize::from(carrying.full_range.end())];
        let roll_text =
            &src[usize::from(roll.full_range.start())..usize::from(roll.full_range.end())];

        assert!(
            !carrying_text.contains("Uniform random"),
            "carrying must not claim roll's docs: {carrying_text:?}"
        );
        assert!(
            roll_text.starts_with("/// Uniform random."),
            "roll's slice starts at its doc block: {roll_text:?}"
        );
        assert!(roll_text.contains("=== function roll"));
    }

    #[test]
    fn stitch_full_range_owns_its_docs() {
        let src = "\
=== hub ===
intro
/// First stall.
= market
stalls
/// Quiet corner.
= shrine
candles
";
        let syms = symbols_for(src);
        let hub = &syms[0];
        let market = &hub.children[0];
        let shrine = &hub.children[1];

        let market_text =
            &src[usize::from(market.full_range.start())..usize::from(market.full_range.end())];
        let shrine_text =
            &src[usize::from(shrine.full_range.start())..usize::from(shrine.full_range.end())];

        assert!(
            market_text.starts_with("/// First stall."),
            "{market_text:?}"
        );
        assert!(
            !market_text.contains("Quiet corner"),
            "market must not claim shrine's docs: {market_text:?}"
        );
        assert!(
            shrine_text.starts_with("/// Quiet corner."),
            "{shrine_text:?}"
        );
    }

    /// #2292: a native `struct` declaration must appear in the outline
    /// alongside its knots, not just be projected into the manifest and
    /// then dropped on the floor by `document_symbols`'s decl-group list.
    ///
    /// Also covers the review finding that `manifest.constants` was missing
    /// from the same decl-group list — a `const` sitting alongside `struct`
    /// and `flow` must show up too.
    #[test]
    fn native_struct_declaration_appears_alongside_knot() {
        let src = "\
const MAX_CUES = 100

struct Cue {
  text: string,
  duration: float
}

flow main() {
  Hello!
}
";
        let parsed = brink_syntax_native::parse(src);
        let (hir, manifest, _) =
            brink_ir::hir::lower_native::lower(brink_ir::FileId(0), &parsed.tree());
        let syms = document_symbols(&hir, &manifest, src);

        let knot = syms
            .iter()
            .find(|s| s.kind == brink_ir::SymbolKind::Knot && s.name == "main");
        assert!(
            knot.is_some(),
            "knot must still be present: {:?}",
            syms.iter().map(|s| (&s.name, s.kind)).collect::<Vec<_>>()
        );

        let strukt = syms
            .iter()
            .find(|s| s.kind == brink_ir::SymbolKind::Struct && s.name == "Cue");
        assert!(
            strukt.is_some(),
            "struct Cue must be present in the outline, not just the knot: {:?}",
            syms.iter().map(|s| (&s.name, s.kind)).collect::<Vec<_>>()
        );

        let konst = syms
            .iter()
            .find(|s| s.kind == brink_ir::SymbolKind::Constant && s.name == "MAX_CUES");
        assert!(
            konst.is_some(),
            "const MAX_CUES must be present in the outline: {:?}",
            syms.iter().map(|s| (&s.name, s.kind)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn undocumented_knots_keep_syntax_ranges() {
        let src = "=== a ===\nA.\n=== b ===\nB.\n";
        let syms = symbols_for(src);
        let a_text =
            &src[usize::from(syms[0].full_range.start())..usize::from(syms[0].full_range.end())];
        assert!(a_text.starts_with("=== a ==="), "{a_text:?}");
        assert!(!a_text.contains("=== b ==="), "{a_text:?}");
    }
}
