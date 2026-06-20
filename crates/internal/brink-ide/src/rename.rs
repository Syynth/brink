use std::collections::BTreeMap;

use brink_analyzer::AnalysisResult;
use brink_ir::{DiagnosticCode, FileId, HirFile, Severity};
use rowan::{TextRange, TextSize};

use crate::line_index::LineIndex;
use crate::navigation::find_def_at_offset;
use crate::session::IdeSession;

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

// ─── Safe rename (studio path) ──────────────────────────────────────────

/// A diagnostic a rename *would introduce* — present after the rename but not
/// before. Carries everything the studio's breakage report needs to render and
/// navigate (1-based line/col, matching the CLI's `DiagEntry`).
pub struct IntroducedDiagnostic {
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub message: String,
    /// Project-relative path of the file the diagnostic lands in.
    pub path: String,
    /// 1-based line of the diagnostic's start.
    pub line: u32,
    /// 1-based column of the diagnostic's start.
    pub col: u32,
}

/// The result of a *safe* rename: the cross-file edits to apply, plus the
/// diagnostics the rename would introduce. `introduced.is_empty()` ⇒ the
/// rename is safe to apply directly; otherwise the caller shows a breakage
/// report and only applies on an explicit force.
pub struct SafeRenameResult {
    pub edits: Vec<FileEdit>,
    pub introduced: Vec<IntroducedDiagnostic>,
}

/// Resolve the declaration offset (name-range start) of a knot, or a stitch
/// within a knot, by name. Returns `None` if the container doesn't exist.
#[must_use]
pub fn declaration_offset(hir: &HirFile, knot: &str, stitch: Option<&str>) -> Option<TextSize> {
    let k = hir.knots.iter().find(|k| k.name.text == knot)?;
    match stitch {
        None => Some(k.name.range.start()),
        Some(s) => k
            .stitches
            .iter()
            .find(|st| st.name.text == s)
            .map(|st| st.name.range.start()),
    }
}

/// Compute a rename and the diagnostics it would introduce, by overlaying the
/// edits and re-analyzing the whole project. The session is not mutated.
#[must_use]
pub fn rename_safe(
    session: &IdeSession,
    file_id: FileId,
    offset: TextSize,
    new_name: &str,
) -> Option<SafeRenameResult> {
    let analysis = session.analysis()?;
    let result = rename(analysis, file_id, offset, new_name)?;

    // Build the overlay: apply each file's edits to its current source.
    // `FileId` isn't `Ord`, so key the group map by its raw id.
    let mut by_file: BTreeMap<u32, Vec<&FileEdit>> = BTreeMap::new();
    for e in &result.edits {
        by_file.entry(e.file.0).or_default().push(e);
    }
    let mut overlay: BTreeMap<String, String> = BTreeMap::new();
    for (raw, mut edits) in by_file {
        let fid = FileId(raw);
        let (Some(path), Some(src)) = (session.file_path(fid), session.source(fid)) else {
            continue;
        };
        let mut s = src.to_owned();
        // Splice from the end so earlier offsets stay valid.
        edits.sort_by_key(|e| std::cmp::Reverse(e.range.start()));
        for e in edits {
            let (start, end) = (usize::from(e.range.start()), usize::from(e.range.end()));
            if start <= end
                && end <= s.len()
                && s.is_char_boundary(start)
                && s.is_char_boundary(end)
            {
                s.replace_range(start..end, &e.new_text);
            }
        }
        overlay.insert(path.to_owned(), s);
    }

    let (new_analysis, new_db) = session.analyze_overlay(&overlay);
    let introduced = introduced_diagnostics(analysis, &new_analysis, &new_db);
    Some(SafeRenameResult {
        edits: result.edits,
        introduced,
    })
}

/// Diff `new_analysis` against the baseline `analysis`, returning the
/// diagnostics that the edit introduced — present now but not before, matched
/// as a multiset keyed by `(code, message)` so duplicate messages are counted.
/// Locations resolve through `new_db` (the overlay db owns the new `FileId`s).
fn introduced_diagnostics(
    analysis: &AnalysisResult,
    new_analysis: &AnalysisResult,
    new_db: &brink_db::ProjectDb,
) -> Vec<IntroducedDiagnostic> {
    let mut baseline: BTreeMap<(&str, &str), i32> = BTreeMap::new();
    for d in &analysis.diagnostics {
        *baseline
            .entry((d.code.as_str(), d.message.as_str()))
            .or_default() += 1;
    }

    let mut introduced = Vec::new();
    for d in &new_analysis.diagnostics {
        let count = baseline
            .entry((d.code.as_str(), d.message.as_str()))
            .or_default();
        if *count > 0 {
            *count -= 1;
            continue;
        }
        let path = new_db.file_path(d.file).unwrap_or_default().to_owned();
        let src = new_db.source(d.file).unwrap_or_default();
        let (line, col) = LineIndex::new(src).line_col(d.range.start());
        introduced.push(IntroducedDiagnostic {
            severity: d.code.severity(),
            code: d.code,
            message: d.message.clone(),
            path,
            line: line + 1,
            col: col + 1,
        });
    }
    introduced
}

#[cfg(test)]
mod tests {
    use super::{declaration_offset, rename_safe};
    use crate::session::IdeSession;

    fn session(src: &str) -> (IdeSession, brink_ir::FileId) {
        let mut s = IdeSession::new();
        let id = s.update_and_analyze("t.ink", src.to_string());
        (s, id)
    }

    #[test]
    fn declaration_offset_resolves_knot_and_stitch() {
        let (s, id) = session("=== outer ===\n= inner\nHi.\n-> END\n");
        let hir = s.hir(id).expect("hir");
        let knot = declaration_offset(hir, "outer", None).expect("knot offset");
        let stitch = declaration_offset(hir, "outer", Some("inner")).expect("stitch offset");
        assert!(stitch > knot, "stitch decl comes after the knot decl");
        assert!(declaration_offset(hir, "missing", None).is_none());
        assert!(declaration_offset(hir, "outer", Some("missing")).is_none());
    }

    #[test]
    fn safe_rename_updates_refs_with_no_new_diagnostics() {
        let (s, id) = session("-> hello\n=== hello ===\nHi.\n-> END\n");
        let hir = s.hir(id).expect("hir");
        let offset = declaration_offset(hir, "hello", None).expect("offset");
        let res = rename_safe(&s, id, offset, "greeting").expect("rename");

        // The divert reference and the declaration are both rewritten.
        assert!(
            res.edits.len() >= 2,
            "expected decl + ref edits, got {}",
            res.edits.len()
        );
        assert!(
            res.introduced.is_empty(),
            "a consistent rename introduces nothing, got {:?}",
            res.introduced
                .iter()
                .map(|d| (d.code.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn rename_into_collision_reports_breakage() {
        // Two knots; renaming `a` to `b` collides with the existing `b`.
        let (s, id) = session("-> a\n=== a ===\n-> END\n=== b ===\n-> END\n");
        let hir = s.hir(id).expect("hir");
        let offset = declaration_offset(hir, "a", None).expect("offset");
        let res = rename_safe(&s, id, offset, "b").expect("rename");

        assert!(
            res.introduced
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E022),
            "expected E022 duplicate-knot, got {:?}",
            res.introduced
                .iter()
                .map(|d| d.code.as_str())
                .collect::<Vec<_>>()
        );
        // The edits are still produced — applying is the caller's choice (force).
        assert!(!res.edits.is_empty());
    }
}
