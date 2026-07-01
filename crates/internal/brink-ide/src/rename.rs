use brink_analyzer::AnalysisResult;
use brink_ir::{FileId, HirFile};
use rowan::{TextRange, TextSize};

use crate::navigation::find_def_at_offset;
use crate::session::IdeSession;
use crate::structural_result::{StructuralResult, gate};

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

/// Apply `edits` to `src`, splicing from the end so earlier offsets stay valid.
fn apply_edits(src: &str, mut edits: Vec<&FileEdit>) -> String {
    let mut s = src.to_owned();
    edits.sort_by_key(|e| std::cmp::Reverse(e.range.start()));
    for e in edits {
        let (start, end) = (usize::from(e.range.start()), usize::from(e.range.end()));
        if start <= end && end <= s.len() && s.is_char_boundary(start) && s.is_char_boundary(end) {
            s.replace_range(start..end, &e.new_text);
        }
    }
    s
}

/// Compute a rename and the diagnostics it would introduce, by overlaying the
/// edits and re-analyzing the whole project (via the op-agnostic [`gate`]). The
/// primary file (`file_id`)'s edits are folded into `new_source`; edits in other
/// files travel out as `cross_file_edits`. The session is not mutated.
#[must_use]
pub fn rename_safe(
    session: &IdeSession,
    file_id: FileId,
    offset: TextSize,
    new_name: &str,
) -> Option<StructuralResult> {
    let analysis = session.analysis()?;
    let result = rename(analysis, file_id, offset, new_name)?;

    // The gate overlays every edit (primary + cross-file) and re-analyzes.
    let introduced = gate(session, &result.edits);

    // Fold the primary file's edits into its new source; the rest are cross-file.
    let primary: Vec<&FileEdit> = result.edits.iter().filter(|e| e.file == file_id).collect();
    let new_source = session.source(file_id).map(|src| apply_edits(src, primary));
    let cross_file_edits: Vec<FileEdit> = result
        .edits
        .into_iter()
        .filter(|e| e.file != file_id)
        .collect();

    Some(StructuralResult {
        new_source,
        cross_file_edits,
        safe: introduced.is_empty(),
        introduced,
    })
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

        // Both the divert reference and the declaration (same file) are folded
        // into new_source — no cross-file edits, and the old name is gone.
        let new_source = res.new_source.as_deref().expect("new_source");
        assert!(
            new_source.contains("-> greeting") && new_source.contains("=== greeting ==="),
            "decl + ref both rewritten: {new_source}"
        );
        assert!(
            !new_source.contains("hello"),
            "old name fully removed: {new_source}"
        );
        assert!(
            res.cross_file_edits.is_empty(),
            "single-file rename has no cross-file edits"
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
        // Not safe, but the edits are still produced — applying is the caller's
        // choice (force). The rewritten primary source is present.
        assert!(!res.safe);
        assert!(res.new_source.is_some());
    }
}
