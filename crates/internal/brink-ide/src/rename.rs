use brink_analyzer::AnalysisResult;
use brink_db::ProjectDb;
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
///
/// B3a UFCS resolution (issue #1539): if `offset` sits on a UFCS call site's
/// method segment, checked first via the same verdict table
/// `crate::ufcs_hover` uses — a field call or prelude intrinsic (no
/// `DefinitionId`) is not renameable; a free-function target is, and the
/// renameable range is the method segment's own span (mirroring how a plain
/// reference's own range, not its target's declaration range, is returned
/// below).
pub fn prepare_rename(
    db: &ProjectDb,
    analysis: &AnalysisResult,
    file_id: FileId,
    offset: rowan::TextSize,
) -> Option<TextRange> {
    if let Some(hir) = db.hir(file_id)
        && let Some(target) =
            crate::ufcs_hover::ufcs_goto_definition_target(db, hir, file_id, offset)
    {
        // `target.is_none()` (field call / prelude intrinsic): not
        // renameable — return `None` rather than falling through to the
        // generic lookup below, which would offer the receiver's range.
        return target.and(crate::ufcs_hover::ufcs_method_range_at_offset(hir, offset));
    }

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
///
/// B3a UFCS resolution (issue #1539): if `offset` sits on a UFCS call
/// site's method segment, the target free function is resolved through the
/// same verdict table `crate::ufcs_hover` uses, rather than
/// `find_def_at_offset` (which would target the *receiver*). Either way,
/// once the definition is known, every UFCS call site project-wide that
/// desugars to it is rewritten alongside the plain `ResolutionMap`
/// references — this is the fix for the "renaming a free function silently
/// misses every UFCS call site" bug: without it, `analysis.resolutions`
/// alone never carries a UFCS call site's true target (see `ufcs_hover`'s
/// module doc), so those call sites were never in the edit set at all.
pub fn rename(
    db: &ProjectDb,
    analysis: &AnalysisResult,
    file_id: FileId,
    offset: rowan::TextSize,
    new_name: &str,
) -> Option<RenameResult> {
    let ufcs_target = db
        .hir(file_id)
        .and_then(|hir| crate::ufcs_hover::ufcs_goto_definition_target(db, hir, file_id, offset));

    let (decl_file, decl_range, analysis_def_id, db_def_id) = match ufcs_target {
        // A field call or prelude intrinsic has no `DefinitionId` — not
        // renameable.
        Some(None) => return None,
        Some(Some(target)) => {
            let info = db.resolutions_index().index.symbols.get(&target)?.clone();
            if matches!(info.kind, brink_ir::SymbolKind::External) {
                return None;
            }
            let analysis_id = analysis
                .index
                .symbols
                .values()
                .find(|i| i.file == info.file && i.range == info.range)
                .map(|i| i.id);
            (info.file, info.range, analysis_id, Some(target))
        }
        None => {
            let info = find_def_at_offset(analysis, file_id, offset)?;
            if matches!(info.kind, brink_ir::SymbolKind::External) {
                return None;
            }
            let db_id = db
                .resolutions_index()
                .index
                .symbols
                .values()
                .find(|i| i.file == info.file && i.range == info.range)
                .map(|i| i.id);
            (info.file, info.range, Some(info.id), db_id)
        }
    };

    let mut edits = Vec::new();

    // 1. Rename the definition site
    edits.push(FileEdit {
        file: decl_file,
        range: decl_range,
        new_text: new_name.to_owned(),
    });

    // 2. Rename all plain reference sites (analysis's own identity space)
    if let Some(def_id) = analysis_def_id {
        for resolved in &analysis.resolutions {
            if resolved.target == def_id {
                edits.push(FileEdit {
                    file: resolved.file,
                    range: resolved.range,
                    new_text: new_name.to_owned(),
                });
            }
        }
    }

    // 3. Rename every UFCS-desugared call site targeting the same free
    // function (issue #1539, `db`'s own identity space).
    if let Some(def_id) = db_def_id {
        for (file, path_range) in db.ufcs_call_sites_for_target(def_id) {
            let Some(hir) = db.hir(file) else {
                continue;
            };
            let Some(method_range) = crate::ufcs_hover::ufcs_method_range_at_path(hir, path_range)
            else {
                continue;
            };
            edits.push(FileEdit {
                file,
                range: method_range,
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
    let result = rename(session.db(), analysis, file_id, offset, new_name)?;

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
    use rowan::TextSize;

    use super::{declaration_offset, prepare_rename, rename, rename_safe};
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

    // ── Issue #1539: rename follows UFCS call sites ──────────────────────

    const UFCS_FREE_FN_SRC: &str = "\
struct Guest {
  name: string
}

fn greet(g, loudness) {
  return loudness;
}

fn main() {
  let g = Guest { name: \"ada\" };
  let n = g.greet(3);
}
";

    fn native_session(src: &str) -> (IdeSession, brink_ir::FileId) {
        let mut s = IdeSession::new();
        let id = s.update_and_analyze("test.brink", src.to_string());
        (s, id)
    }

    #[test]
    fn renaming_a_free_function_from_its_declaration_rewrites_its_ufcs_call_site() {
        // The core #1539 bug: `fn greet` is called only via UFCS
        // (`g.greet(3)`) — before this fix, `rename` keyed solely off
        // `analysis.resolutions`, which never carries a UFCS call site's
        // true target, so the call site was silently left unrenamed,
        // producing a broken program (`greet` still called under its old
        // name after the declaration moved).
        let (s, id) = native_session(UFCS_FREE_FN_SRC);
        let decl_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(g").expect("decl")).expect("offset");
        let analysis = s.analysis().expect("analysis");

        let result =
            rename(s.db(), analysis, id, TextSize::from(decl_pos), "salute").expect("rename");

        let call_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(3)").expect("call")).expect("offset");
        assert!(
            result.edits.iter().any(|e| e.file == id
                && e.range.start() == TextSize::from(call_pos)
                && e.new_text == "salute"),
            "expected the UFCS call site's method segment rewritten to `salute`, got {:?}",
            result
                .edits
                .iter()
                .map(|e| (e.range, e.new_text.as_str()))
                .collect::<Vec<_>>()
        );
        // The declaration itself is also rewritten.
        assert!(
            result
                .edits
                .iter()
                .any(|e| e.range.start() == TextSize::from(decl_pos) && e.new_text == "salute"),
            "expected the declaration site rewritten too"
        );
    }

    #[test]
    fn renaming_a_free_function_from_its_ufcs_call_site_rewrites_the_declaration() {
        // The reverse direction: initiating the rename *from* the UFCS call
        // site's method segment must resolve to the free function (not the
        // receiver `g`) and still rewrite the declaration.
        let (s, id) = native_session(UFCS_FREE_FN_SRC);
        let call_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(3)").expect("call")).expect("offset");
        let analysis = s.analysis().expect("analysis");

        let result =
            rename(s.db(), analysis, id, TextSize::from(call_pos), "salute").expect("rename");

        let decl_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(g").expect("decl")).expect("offset");
        assert!(
            result
                .edits
                .iter()
                .any(|e| e.range.start() == TextSize::from(decl_pos) && e.new_text == "salute"),
            "expected the `fn greet` declaration rewritten, got {:?}",
            result
                .edits
                .iter()
                .map(|e| (e.range, e.new_text.as_str()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn rename_via_rename_safe_folds_the_ufcs_call_site_into_new_source() {
        // The studio-facing `rename_safe` path (issue #1539): the UFCS call
        // site is in the same file as the declaration, so it must be folded
        // into `new_source` alongside the declaration, not silently dropped.
        let (s, id) = native_session(UFCS_FREE_FN_SRC);
        let decl_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(g").expect("decl")).expect("offset");

        let res = rename_safe(&s, id, TextSize::from(decl_pos), "salute").expect("rename");

        let new_source = res.new_source.as_deref().expect("new_source");
        assert!(
            new_source.contains("fn salute(") && new_source.contains("g.salute(3)"),
            "decl + UFCS call site both rewritten: {new_source}"
        );
        assert!(
            !new_source.contains("greet"),
            "old name fully removed: {new_source}"
        );
    }

    #[test]
    fn prepare_rename_on_a_ufcs_call_site_returns_the_method_segment_span() {
        let (s, id) = native_session(UFCS_FREE_FN_SRC);
        let call_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(3)").expect("call")).expect("offset");
        let analysis = s.analysis().expect("analysis");

        let range =
            prepare_rename(s.db(), analysis, id, TextSize::from(call_pos)).expect("renameable");

        assert_eq!(
            &UFCS_FREE_FN_SRC[usize::from(range.start())..usize::from(range.end())],
            "greet",
            "the UFCS call's own method span, not the receiver's or the target's declaration"
        );
    }

    #[test]
    fn prepare_rename_on_a_ufcs_field_call_is_not_renameable() {
        // Fixture mirrors `navigation.rs`'s
        // `goto_definition_on_a_ufcs_field_call_finds_no_target`: a struct
        // field has no `DefinitionId`, so it cannot be renamed through this
        // path.
        let src = "\
struct Guest {
  greet: fn(int): int
}

fn main() {
  let g = Guest { greet: \"hi\" };
  let n = g.greet(3);
}
";
        let (s, id) = native_session(src);
        let call_pos = u32::try_from(src.find("greet(3)").expect("call")).expect("offset");
        let analysis = s.analysis().expect("analysis");

        assert!(
            prepare_rename(s.db(), analysis, id, TextSize::from(call_pos)).is_none(),
            "a field call has no DefinitionId to rename"
        );
    }
}
