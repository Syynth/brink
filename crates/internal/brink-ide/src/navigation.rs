use brink_analyzer::AnalysisResult;
use brink_ir::{FileId, SymbolInfo};
use rowan::TextRange;

/// A location result for navigation operations.
pub struct LocationResult {
    pub file: FileId,
    pub range: TextRange,
}

/// Find the definition for the symbol at `offset`.
///
/// Tries, in order: resolved references, declaration sites, then local
/// variables (params/temps) by identifier text.
pub fn find_def_at_offset(
    analysis: &AnalysisResult,
    file_id: FileId,
    offset: rowan::TextSize,
) -> Option<&SymbolInfo> {
    // 1. Resolved reference at this position
    let def_id = analysis
        .resolutions
        .iter()
        .find(|r| r.file == file_id && (r.range.contains(offset) || r.range.start() == offset))
        .map(|r| r.target)
        // 2. Declaration site at this position
        .or_else(|| {
            analysis
                .index
                .symbols
                .values()
                .find(|info| {
                    info.file == file_id
                        && (info.range.contains(offset) || info.range.start() == offset)
                })
                .map(|info| info.id)
        });

    def_id.and_then(|id| analysis.index.symbols.get(&id))
}

/// Compute goto-definition for the symbol at `offset`.
pub fn goto_definition(
    analysis: &AnalysisResult,
    file_id: FileId,
    offset: rowan::TextSize,
) -> Option<LocationResult> {
    let info = find_def_at_offset(analysis, file_id, offset)?;
    Some(LocationResult {
        file: info.file,
        range: info.range,
    })
}

/// Find all references to the symbol at `offset`.
pub fn find_references(
    analysis: &AnalysisResult,
    file_id: FileId,
    offset: rowan::TextSize,
    include_declaration: bool,
) -> Vec<LocationResult> {
    let def_id = analysis
        .resolutions
        .iter()
        .find(|r| r.file == file_id && (r.range.contains(offset) || r.range.start() == offset))
        .map(|r| r.target)
        .or_else(|| {
            analysis
                .index
                .symbols
                .values()
                .find(|info| {
                    info.file == file_id
                        && (info.range.contains(offset) || info.range.start() == offset)
                })
                .map(|info| info.id)
        });

    let Some(def_id) = def_id else {
        return Vec::new();
    };

    let mut locations = Vec::new();

    // Include the definition itself if requested
    if include_declaration && let Some(info) = analysis.index.symbols.get(&def_id) {
        locations.push(LocationResult {
            file: info.file,
            range: info.range,
        });
    }

    // Collect all reference sites that resolve to this definition
    for resolved in &analysis.resolutions {
        if resolved.target == def_id {
            locations.push(LocationResult {
                file: resolved.file,
                range: resolved.range,
            });
        }
    }

    locations
}
