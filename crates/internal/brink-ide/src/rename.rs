use brink_analyzer::AnalysisResult;
use brink_ir::FileId;
use rowan::TextRange;

use crate::navigation::find_def_at_offset;

/// A single text edit within a file.
pub struct FileEdit {
    pub file: FileId,
    pub range: TextRange,
    pub new_text: String,
}

/// The result of a rename operation.
pub struct RenameResult {
    pub edits: Vec<FileEdit>,
}

/// Check if a rename is possible at `offset` and return the renameable range.
pub fn prepare_rename(
    analysis: &AnalysisResult,
    file_id: FileId,
    offset: rowan::TextSize,
) -> Option<TextRange> {
    let info = find_def_at_offset(analysis, file_id, offset)?;

    // Builtins and externals cannot be renamed
    if matches!(info.kind, brink_ir::SymbolKind::External) {
        return None;
    }

    // Return the range of the symbol under the cursor (reference or definition site)
    analysis
        .resolutions
        .iter()
        .find(|r| r.file == file_id && (r.range.contains(offset) || r.range.start() == offset))
        .map(|r| r.range)
        .or_else(|| (info.file == file_id).then_some(info.range))
}

/// Compute a rename of the symbol at `offset` to `new_name`.
pub fn rename(
    analysis: &AnalysisResult,
    file_id: FileId,
    offset: rowan::TextSize,
    new_name: &str,
) -> Option<RenameResult> {
    let info = find_def_at_offset(analysis, file_id, offset)?;

    if matches!(info.kind, brink_ir::SymbolKind::External) {
        return None;
    }

    let def_id = info.id;
    let mut edits = Vec::new();

    // 1. Rename the definition site
    edits.push(FileEdit {
        file: info.file,
        range: info.range,
        new_text: new_name.to_owned(),
    });

    // 2. Rename all reference sites
    for resolved in &analysis.resolutions {
        if resolved.target == def_id {
            edits.push(FileEdit {
                file: resolved.file,
                range: resolved.range,
                new_text: new_name.to_owned(),
            });
        }
    }

    Some(RenameResult { edits })
}
