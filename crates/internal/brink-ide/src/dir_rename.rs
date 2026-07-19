//! Atomic directory rename / move with `INCLUDE`-reference rewriting (#314).
//!
//! Renaming or moving a *directory* relocates every file under it at once. Doing
//! that as a loop over [`rename_file`](crate::file_rename::rename_file) is
//! subtly wrong: each per-file rename resolves its sibling includes against an
//! *intermediate* project state (some files moved, some not), so an
//! intra-folder include can be rewritten twice or against a path that only
//! momentarily exists. [`rename_dir`] instead computes every edit against **one
//! pre-move snapshot**, so the three rewrite classes stay mutually consistent:
//!
//! - **moved-file outbound** — each moved file's own relative includes,
//!   re-expressed from its *new* directory. If the include targets another
//!   moved file, it is relativized to that file's *new* path (the intra-folder
//!   sibling case the looped rename got wrong);
//! - **inbound** — every file *outside* the folder whose `INCLUDE` resolves into
//!   it is re-pointed at the moved target's new path;
//! - **intra-folder siblings** — folded into the moved-file outbound pass, since
//!   a sibling include is just an outbound include whose target also moved.
//!
//! The op is pure — it returns the edits; the caller writes the new files,
//! removes the old ones, and applies the inbound edits. It runs the same
//! op-agnostic safe-by-default breakage gate as #316: the post-move project is
//! re-analyzed as a whole (files at their *new* keys) and any introduced
//! diagnostic (e.g. a divert that would dangle) surfaces in the report.
//!
//! Relative-path math reuses `brink-db`'s [`resolve_include_path`] /
//! [`compute_relative_path`] (proven inverses, shared with #318) — this module
//! does not reimplement path normalization.

use std::collections::BTreeMap;

use brink_db::{compute_relative_path, resolve_include_path};
use brink_ir::InkProvenanceResolver;
use brink_syntax::SyntaxNode;
use brink_syntax::ast::{AstNode as _, IncludeStmt};
use rowan::TextRange;

use crate::rename::FileEdit;
use crate::session::IdeSession;
use crate::structural_result::{IntroducedDiagnostic, introduced_diagnostics};

/// Errors from a directory rename/move.
#[derive(Debug, thiserror::Error)]
pub enum DirRenameError {
    #[error("no files found under directory '{0}'")]
    NotFound(String),
    #[error("a file already exists at '{0}'")]
    DestinationExists(String),
}

/// One moved file's rewritten content at its new path.
///
/// The caller writes `new_source` at `new_path` and removes `old_path`.
/// `new_source` already carries the file's own outbound-include rewrites.
pub struct MovedFile {
    /// The file's project-relative path before the move.
    pub old_path: String,
    /// The file's project-relative path after the move.
    pub new_path: String,
    /// The moved file's full source with its relative includes rewritten for
    /// the new directory (byte-identical to the old source when it had no
    /// relative includes that changed).
    pub new_source: String,
}

/// The result of an atomic directory rename/move (#314) — the multi-file analog
/// of [`StructuralResult`](crate::structural_result::StructuralResult).
///
/// `moved_files` are the relocated files (each carrying its new path + rewritten
/// source); `cross_file_edits` are the inbound `INCLUDE` rewrites in files that
/// stay put. `safe` / `introduced` are the shared safe-by-default breakage gate:
/// `introduced.is_empty()` ⇒ `safe` ⇒ apply directly, otherwise the caller shows
/// the breakage report and applies only on an explicit force.
pub struct DirMoveResult {
    /// Every file relocated by the move, in deterministic (old-path) order.
    pub moved_files: Vec<MovedFile>,
    /// Reference edits in files *outside* the moved directory.
    pub cross_file_edits: Vec<FileEdit>,
    /// True when the move introduces no new diagnostics.
    pub safe: bool,
    /// Diagnostics present after the move but not before. Empty ⇒ `safe`.
    pub introduced: Vec<IntroducedDiagnostic>,
}

/// True when `path` lies under the directory `prefix` — i.e. `path` starts with
/// `prefix/`. A bare-name `prefix` still matches `prefix/child.ink`. The exact
/// file `prefix` itself is *not* under it (dir renames operate on contents).
fn under_dir(path: &str, prefix: &str) -> bool {
    let mut with_slash = String::with_capacity(prefix.len() + 1);
    with_slash.push_str(prefix);
    with_slash.push('/');
    path.starts_with(&with_slash)
}

/// Map a path under `old_prefix` to the same relative position under
/// `new_prefix`. `path` must satisfy [`under_dir(path, old_prefix)`](under_dir).
fn remap_path(path: &str, old_prefix: &str, new_prefix: &str) -> String {
    let rest = &path[old_prefix.len() + 1..]; // skip "old_prefix/"
    if new_prefix.is_empty() {
        rest.to_owned()
    } else {
        format!("{new_prefix}/{rest}")
    }
}

/// Rename (or move) the directory `old_prefix` to `new_prefix`, relocating every
/// file under it and rewriting every `INCLUDE` affected by the move — computed
/// against one pre-move snapshot so all rewrites are mutually consistent.
///
/// Pure: returns the edits, applies nothing. The caller writes each moved file's
/// `new_source` at its `new_path`, removes the old paths, and applies the
/// inbound `cross_file_edits`.
///
/// # Errors
/// [`DirRenameError::NotFound`] if no file lives under `old_prefix`;
/// [`DirRenameError::DestinationExists`] if any moved file's destination path is
/// already occupied by a file that is not itself moving.
pub fn rename_dir(
    session: &IdeSession,
    old_prefix: &str,
    new_prefix: &str,
) -> Result<DirMoveResult, DirRenameError> {
    let old_prefix = old_prefix.trim_end_matches('/');
    let new_prefix = new_prefix.trim_end_matches('/');

    // 1. Resolve the moved set against the pre-move snapshot: old path → new
    //    path, deterministically ordered by old path (BTreeMap keys).
    let mut moved: BTreeMap<String, String> = BTreeMap::new();
    for fid in session.db().file_ids() {
        let Some(path) = session.file_path(fid) else {
            continue;
        };
        if under_dir(path, old_prefix) {
            moved.insert(path.to_owned(), remap_path(path, old_prefix, new_prefix));
        }
    }
    if moved.is_empty() {
        return Err(DirRenameError::NotFound(old_prefix.to_owned()));
    }
    if old_prefix == new_prefix {
        // No-op move: every file keeps its path and content.
        return Ok(no_op_result(session, &moved));
    }

    // Destination collision: a new path already taken by a non-moving file.
    for new_path in moved.values() {
        if let Some(existing) = session.file_id(new_path) {
            let existing_path = session.file_path(existing).unwrap_or_default();
            if !moved.contains_key(existing_path) {
                return Err(DirRenameError::DestinationExists(new_path.clone()));
            }
        }
    }

    // Resolve a target file path to where it will live AFTER the move: moved
    // targets follow their new path, everything else stays put.
    let new_target_of = |target: &str| -> String {
        moved
            .get(target)
            .cloned()
            .unwrap_or_else(|| target.to_owned())
    };

    // 2. Moved-file outbound (+ intra-folder sibling) rewrites: each moved
    //    file's own relative includes, resolved once against its OLD location,
    //    then re-expressed from its NEW location (honouring moved targets).
    let mut moved_files: Vec<MovedFile> = Vec::new();
    for (old_path, new_path) in &moved {
        let Some(old_id) = session.file_id(old_path) else {
            continue;
        };
        let Some(old_source) = session.source(old_id) else {
            continue;
        };
        let mut own_edits: Vec<(usize, usize, String)> = Vec::new();
        if let (Some(hir), Some(root)) = (session.hir(old_id), session.syntax_root(old_id)) {
            for inc in &hir.includes {
                let target = resolve_include_path(old_path, &inc.file_path);
                let new_target = new_target_of(&target);
                let new_rel = compute_relative_path(new_path, &new_target);
                if new_rel == inc.file_path {
                    continue; // unchanged
                }
                if let Some(range) = include_path_range(old_id, inc.ptr, &root) {
                    own_edits.push((
                        usize::from(range.start()),
                        usize::from(range.end()),
                        new_rel,
                    ));
                }
            }
        }
        moved_files.push(MovedFile {
            old_path: old_path.clone(),
            new_path: new_path.clone(),
            new_source: apply_text_edits(old_source, own_edits),
        });
    }

    // 3. Inbound rewrites: files OUTSIDE the folder whose INCLUDE resolves to a
    //    moved file → re-point at the moved target's new path.
    let mut cross_file_edits: Vec<FileEdit> = Vec::new();
    for fid in session.db().file_ids() {
        let Some(fpath) = session.file_path(fid) else {
            continue;
        };
        if moved.contains_key(fpath) {
            continue; // moved files handle their own includes in pass 2
        }
        let (Some(hir), Some(root)) = (session.hir(fid), session.syntax_root(fid)) else {
            continue;
        };
        for inc in &hir.includes {
            let target = resolve_include_path(fpath, &inc.file_path);
            let Some(new_target) = moved.get(&target) else {
                continue; // include does not point into the moved folder
            };
            if let Some(range) = include_path_range(fid, inc.ptr, &root) {
                cross_file_edits.push(FileEdit {
                    file: fid,
                    range,
                    new_text: compute_relative_path(fpath, new_target),
                });
            }
        }
    }

    // 4. Safe-by-default gate (#316): build the whole post-move project (files at
    //    their NEW keys, sources rewritten) and diff diagnostics against the
    //    baseline. A move that would dangle a divert surfaces here.
    let introduced = gate_dir_move(session, &moved, &moved_files, &cross_file_edits);

    Ok(DirMoveResult {
        moved_files,
        cross_file_edits,
        safe: introduced.is_empty(),
        introduced,
    })
}

/// The no-op result for a move whose destination equals its source: every file
/// keeps its path and content, no edits, trivially safe.
fn no_op_result(session: &IdeSession, moved: &BTreeMap<String, String>) -> DirMoveResult {
    let mut moved_files = Vec::new();
    for old_path in moved.keys() {
        let source = session
            .file_id(old_path)
            .and_then(|id| session.source(id))
            .unwrap_or_default()
            .to_owned();
        moved_files.push(MovedFile {
            old_path: old_path.clone(),
            new_path: old_path.clone(),
            new_source: source,
        });
    }
    DirMoveResult {
        moved_files,
        cross_file_edits: Vec::new(),
        safe: true,
        introduced: Vec::new(),
    }
}

/// Build the complete post-move project projection (path → source) and re-analyze
/// it, returning the diagnostics the move would introduce. Moved files appear at
/// their new keys with rewritten sources; outside files carry their inbound
/// rewrites; untouched files pass through verbatim.
fn gate_dir_move(
    session: &IdeSession,
    moved: &BTreeMap<String, String>,
    moved_files: &[MovedFile],
    cross_file_edits: &[FileEdit],
) -> Vec<IntroducedDiagnostic> {
    let Some(baseline) = session.analysis() else {
        return Vec::new();
    };

    // Group inbound edits by file so each outside file is spliced once.
    let mut inbound_by_file: BTreeMap<u32, Vec<&FileEdit>> = BTreeMap::new();
    for e in cross_file_edits {
        inbound_by_file.entry(e.file.0).or_default().push(e);
    }

    let mut projection: BTreeMap<String, String> = BTreeMap::new();
    for fid in session.db().file_ids() {
        let (Some(path), Some(src)) = (session.file_path(fid), session.source(fid)) else {
            continue;
        };
        if moved.contains_key(path) {
            // Relocated files are added from `moved_files` below, at new keys.
            continue;
        }
        let mut s = src.to_owned();
        if let Some(mut edits) = inbound_by_file.remove(&fid.0) {
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
        }
        projection.insert(path.to_owned(), s);
    }
    for mf in moved_files {
        projection.insert(mf.new_path.clone(), mf.new_source.clone());
    }

    let (new_analysis, new_db) = session.analyze_projection(&projection);
    introduced_diagnostics(baseline, &new_analysis, &new_db)
}

/// The exact byte range of an `INCLUDE`'s file-path token, resolved against the
/// file's parse root.
fn include_path_range(
    file: brink_ir::FileId,
    provenance: brink_ir::Provenance,
    root: &SyntaxNode,
) -> Option<TextRange> {
    let resolver = InkProvenanceResolver::new(file, root);
    let stmt: IncludeStmt = resolver.resolve_ast(provenance)?;
    Some(stmt.file_path()?.syntax().text_range())
}

/// Apply `(start, end, text)` byte-range replacements to `src`, descending by
/// start so earlier offsets stay valid.
fn apply_text_edits(src: &str, mut edits: Vec<(usize, usize, String)>) -> String {
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut out = src.to_owned();
    for (start, end, text) in edits {
        if start <= end && end <= out.len() {
            out.replace_range(start..end, &text);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::rename_dir;
    use crate::session::IdeSession;

    fn session(files: &[(&str, &str)]) -> IdeSession {
        let mut s = IdeSession::new();
        for (path, src) in files {
            s.update_and_analyze(path, (*src).to_owned());
        }
        s
    }

    /// The rewritten source of the moved file at `new_path`.
    fn moved_src<'a>(r: &'a super::DirMoveResult, new_path: &str) -> &'a str {
        r.moved_files
            .iter()
            .find(|m| m.new_path == new_path)
            .map(|m| m.new_source.as_str())
            .expect("moved file present")
    }

    #[test]
    fn rename_dir_rewrites_inbound_includes_from_outside() {
        // main.ink (outside) includes into the folder; renaming chapters/ →
        // parts/ must re-point main's INCLUDE.
        let s = session(&[
            ("main.ink", "INCLUDE chapters/one.ink\n-> END\n"),
            ("chapters/one.ink", "=== one ===\n-> END\n"),
        ]);
        let r = rename_dir(&s, "chapters", "parts").unwrap();
        assert_eq!(r.moved_files.len(), 1);
        assert_eq!(r.moved_files[0].new_path, "parts/one.ink");
        assert_eq!(r.cross_file_edits.len(), 1);
        assert_eq!(r.cross_file_edits[0].new_text, "parts/one.ink");
        assert!(
            r.safe,
            "clean rename should be safe: {:?}",
            r.introduced_msgs()
        );
    }

    #[test]
    fn rename_dir_rewrites_moved_files_outbound_includes() {
        // A file inside the folder includes a file OUTSIDE it (../lib.ink). After
        // the folder moves deeper the relative path must be recomputed.
        let s = session(&[
            ("lib.ink", "=== helper ===\n-> END\n"),
            ("chapters/one.ink", "INCLUDE ../lib.ink\n-> END\n"),
        ]);
        let r = rename_dir(&s, "chapters", "book/chapters").unwrap();
        let src = moved_src(&r, "book/chapters/one.ink");
        assert!(
            src.contains("INCLUDE ../../lib.ink"),
            "outbound include not recomputed for deeper move: {src}"
        );
    }

    #[test]
    fn rename_dir_keeps_intra_folder_sibling_includes_consistent() {
        // The looped-rename bug: two siblings in the folder include each other.
        // Against one snapshot the sibling includes stay stable (same relative
        // path) — never rewritten against an intermediate half-moved state.
        let s = session(&[
            ("chapters/one.ink", "INCLUDE two.ink\n=== one ===\n-> END\n"),
            ("chapters/two.ink", "INCLUDE one.ink\n=== two ===\n-> END\n"),
        ]);
        let r = rename_dir(&s, "chapters", "parts").unwrap();
        let one = moved_src(&r, "parts/one.ink");
        let two = moved_src(&r, "parts/two.ink");
        // Both siblings move together, so the bare-name sibling include is
        // unchanged — and definitely not mangled to a `../chapters/…` path.
        assert!(
            one.contains("INCLUDE two.ink"),
            "sibling include should stay bare: {one}"
        );
        assert!(
            two.contains("INCLUDE one.ink"),
            "sibling include should stay bare: {two}"
        );
        assert!(!one.contains("chapters"), "stale folder leaked: {one}");
        assert!(!two.contains("chapters"), "stale folder leaked: {two}");
        assert!(r.safe, "sibling-consistent rename should be safe");
    }

    #[test]
    fn rename_dir_intra_folder_sibling_with_subdir_recomputed() {
        // A sibling include that crosses a sub-directory within the folder is
        // recomputed once against the pre-move snapshot: moving the folder does
        // not change the *relative* geometry between the two moved files, so the
        // include text is preserved exactly.
        let s = session(&[
            ("chapters/a/one.ink", "INCLUDE ../b/two.ink\n-> END\n"),
            ("chapters/b/two.ink", "=== two ===\n-> END\n"),
        ]);
        let r = rename_dir(&s, "chapters", "parts").unwrap();
        let one = moved_src(&r, "parts/a/one.ink");
        assert!(
            one.contains("INCLUDE ../b/two.ink"),
            "intra-folder subdir include should be preserved: {one}"
        );
    }

    #[test]
    fn rename_dir_gate_does_not_report_preexisting_diagnostics() {
        // Safe-by-default gate must *diff* — a diagnostic that was already
        // present before the move (here an unresolved divert to a knot that
        // never existed) is not fabricated as "introduced" by the move. A
        // content-preserving atomic dir move introduces nothing new, so the
        // result is safe even though the project itself is not clean.
        let s = session(&[
            ("main.ink", "-> nowhere\n"),
            ("chapters/one.ink", "=== one ===\n-> END\n"),
        ]);
        // `-> nowhere` is an unresolved divert (E024) present BEFORE and AFTER.
        assert!(
            s.analysis()
                .unwrap()
                .diagnostics
                .iter()
                .any(|d| d.code.as_str() == "E024"),
            "precondition: pre-existing E024 must be present"
        );
        let r = rename_dir(&s, "chapters", "parts").unwrap();
        assert!(
            r.safe,
            "pre-existing diagnostic must not be reported as introduced: {:?}",
            r.introduced_msgs()
        );
        assert!(r.introduced.is_empty());
    }

    #[test]
    fn rename_dir_gate_reports_breakage_when_a_moved_file_drops_a_divert_target() {
        // A genuine introduced-diagnostic path for the shared #316 gate: build
        // the post-move projection so that a divert loses its target, and prove
        // the gate surfaces the newly-introduced E024. We drive the gate helper
        // directly with a projection that *omits* a knot an outside file diverts
        // to — the same diff the op runs, exercised on a breaking projection.
        use std::collections::BTreeMap;

        let mut s = IdeSession::new();
        // Baseline: main diverts to `target`, which exists in chapters/one.ink.
        s.update_and_analyze("main.ink", "-> target\n".to_owned());
        s.update_and_analyze("chapters/one.ink", "=== target ===\n-> END\n".to_owned());
        // Baseline is clean (the divert resolves).
        assert!(
            s.analysis()
                .unwrap()
                .diagnostics
                .iter()
                .all(|d| d.code.as_str() != "E024"),
            "precondition: baseline divert resolves"
        );

        // A projection in which the moved file lost its `target` knot: the divert
        // now dangles. This is exactly the diff shape the gate computes, so the
        // gate must report the newly-introduced E024.
        let mut projection: BTreeMap<String, String> = BTreeMap::new();
        projection.insert("main.ink".to_owned(), "-> target\n".to_owned());
        projection.insert(
            "parts/one.ink".to_owned(),
            "=== other ===\n-> END\n".to_owned(),
        );
        let baseline = s.analysis().unwrap();
        let (new_analysis, new_db) = s.analyze_projection(&projection);
        let introduced =
            crate::structural_result::introduced_diagnostics(baseline, &new_analysis, &new_db);
        assert!(
            introduced.iter().any(|d| d.code.as_str() == "E024"),
            "gate should report the introduced dangling divert: {:?}",
            introduced.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn rename_dir_errors_when_no_files_under_prefix() {
        let s = session(&[("a.ink", "-> END\n")]);
        assert!(rename_dir(&s, "ghost", "x").is_err());
    }

    #[test]
    fn rename_dir_errors_on_destination_collision() {
        let s = session(&[
            ("chapters/one.ink", "-> END\n"),
            ("parts/one.ink", "-> END\n"),
        ]);
        // parts/one.ink already exists and is not itself moving.
        assert!(rename_dir(&s, "chapters", "parts").is_err());
    }

    #[test]
    fn rename_dir_all_edits_against_one_snapshot_are_consistent() {
        // Full scenario: outside referrer + moved file with outbound include +
        // intra-folder sibling, all resolved once. Proves the three classes
        // agree.
        let s = session(&[
            ("main.ink", "INCLUDE chapters/intro.ink\n-> END\n"),
            ("lib.ink", "=== helper ===\n-> END\n"),
            (
                "chapters/intro.ink",
                "INCLUDE ../lib.ink\nINCLUDE scene.ink\n-> END\n",
            ),
            ("chapters/scene.ink", "=== scene ===\n-> END\n"),
        ]);
        let r = rename_dir(&s, "chapters", "book/chapters").unwrap();

        // Inbound: main re-points into the new folder.
        assert_eq!(r.cross_file_edits.len(), 1);
        assert_eq!(r.cross_file_edits[0].new_text, "book/chapters/intro.ink");

        // Outbound: intro's ../lib.ink is now two levels deep.
        let intro = moved_src(&r, "book/chapters/intro.ink");
        assert!(
            intro.contains("INCLUDE ../../lib.ink"),
            "outbound not recomputed: {intro}"
        );
        // Sibling: scene.ink moves with intro, so the bare include is unchanged.
        assert!(
            intro.contains("INCLUDE scene.ink"),
            "sibling include should stay bare: {intro}"
        );
        assert!(
            r.safe,
            "consistent move should be safe: {:?}",
            r.introduced_msgs()
        );
    }

    impl super::DirMoveResult {
        fn introduced_msgs(&self) -> Vec<String> {
            self.introduced.iter().map(|d| d.message.clone()).collect()
        }
    }
}
