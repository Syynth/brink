//! Whole-file rename / move with `INCLUDE`-reference rewriting (#164 Stage 3).
//!
//! Renaming or moving a file changes its project key, which breaks two kinds
//! of `INCLUDE`:
//!
//! - **inbound** — every other file that `INCLUDE`s the renamed file now points
//!   at a path that no longer exists;
//! - **outbound** — the renamed file's own relative `INCLUDE`s were written
//!   against its *old* directory; moving it to a new directory changes what
//!   those relative paths resolve to.
//!
//! [`rename_file`] computes both as a [`StructuralResult`]: outbound edits are
//! folded into `new_source` (the moved file's content, otherwise unchanged),
//! inbound edits become `cross_file_edits` (the wasm layer resolves each to a
//! full file source). The op runs the op-agnostic breakage gate (#316): a
//! rename whose `INCLUDE` rewrites would dangle (e.g. a missing destination)
//! surfaces the introduced diagnostics. Relative-path math reuses `brink-db`'s
//! [`resolve_include_path`] / [`compute_relative_path`] (inverses of each other).

use brink_db::{compute_relative_path, resolve_include_path};
use brink_syntax::SyntaxNode;
use brink_syntax::ast::{AstNode as _, AstPtr, IncludeStmt};
use rowan::TextRange;

use crate::rename::FileEdit;
use crate::session::IdeSession;
use crate::structural_result::{StructuralResult, gate_with_source};

/// Errors from a file rename/move.
#[derive(Debug, thiserror::Error)]
pub enum RenameError {
    #[error("file '{0}' not found")]
    NotFound(String),
    #[error("a file already exists at '{0}'")]
    DestinationExists(String),
}

/// Rename (or move) the file at `old` to `new`, rewriting every `INCLUDE` that
/// resolves to it plus the moved file's own relative includes.
///
/// # Errors
/// [`RenameError::NotFound`] if `old` is not loaded; [`RenameError::DestinationExists`]
/// if a different file already occupies `new`.
pub fn rename_file(
    session: &IdeSession,
    old: &str,
    new: &str,
) -> Result<StructuralResult, RenameError> {
    let old_id = session
        .file_id(old)
        .ok_or_else(|| RenameError::NotFound(old.to_owned()))?;
    let old_source = session
        .source(old_id)
        .ok_or_else(|| RenameError::NotFound(old.to_owned()))?;

    if old == new {
        // No-op: the file keeps its content and no includes change.
        return Ok(StructuralResult::safe_source(old_source.to_owned()));
    }
    if session.file_id(new).is_some() {
        return Err(RenameError::DestinationExists(new.to_owned()));
    }

    // Inbound: every other file's INCLUDEs that resolve to `old` → re-point at
    // `new` (relative to that file's own directory).
    let mut cross_file_edits: Vec<FileEdit> = Vec::new();
    for fid in session.db().file_ids() {
        if fid == old_id {
            continue;
        }
        let (Some(fpath), Some(hir), Some(root)) = (
            session.file_path(fid),
            session.hir(fid),
            session.syntax_root(fid),
        ) else {
            continue;
        };
        for inc in &hir.includes {
            if resolve_include_path(fpath, &inc.file_path) != old {
                continue;
            }
            if let Some(range) = include_path_range(&inc.ptr, &root) {
                cross_file_edits.push(FileEdit {
                    file: fid,
                    range,
                    new_text: compute_relative_path(fpath, new),
                });
            }
        }
    }

    // Outbound: the moved file's own relative includes, re-expressed from the
    // new directory. Resolve each against the OLD location (where they were
    // valid), then relativize from `new`.
    let mut own_edits: Vec<(usize, usize, String)> = Vec::new();
    if let (Some(hir), Some(root)) = (session.hir(old_id), session.syntax_root(old_id)) {
        for inc in &hir.includes {
            let target = resolve_include_path(old, &inc.file_path);
            let new_rel = compute_relative_path(new, &target);
            if new_rel == inc.file_path {
                continue; // unchanged (e.g. rename in place keeps the directory)
            }
            if let Some(range) = include_path_range(&inc.ptr, &root) {
                own_edits.push((
                    usize::from(range.start()),
                    usize::from(range.end()),
                    new_rel,
                ));
            }
        }
    }

    let new_source = apply_text_edits(old_source, own_edits);

    // Breakage gate (#316): overlay the inbound INCLUDE rewrites (and the moved
    // file's rewritten content, keyed at its current path) and re-analyze. A
    // rename whose rewrites leave an INCLUDE pointing at a non-existent file, or
    // otherwise break resolution, surfaces here. The path relocation itself is
    // an INCLUDE-graph change the overlay can't fully model, so this gate is
    // conservative — it never fabricates breakage for a clean rename.
    let introduced = gate_with_source(session, old, &new_source, &cross_file_edits);

    Ok(StructuralResult {
        new_source: Some(new_source),
        cross_file_edits,
        safe: introduced.is_empty(),
        introduced,
    })
}

/// The exact byte range of an `INCLUDE`'s file-path token (not the whole
/// statement), resolved against the file's parse root.
fn include_path_range(ptr: &AstPtr<IncludeStmt>, root: &SyntaxNode) -> Option<TextRange> {
    let stmt = ptr.resolve(root)?;
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
    use super::rename_file;
    use crate::session::IdeSession;

    fn session(files: &[(&str, &str)]) -> IdeSession {
        let mut s = IdeSession::new();
        for (path, src) in files {
            s.update_and_analyze(path, (*src).to_owned());
        }
        s
    }

    /// The rewritten primary-file source of a structural result (`unwrap`ped).
    fn ns(result: &crate::structural_result::StructuralResult) -> &str {
        result.new_source.as_deref().expect("new_source")
    }

    #[test]
    fn rename_in_place_rewrites_inbound_filename() {
        let s = session(&[
            ("main.ink", "INCLUDE lib.ink\n-> END\n"),
            ("lib.ink", "=== helper ===\n-> END\n"),
        ]);
        let result = rename_file(&s, "lib.ink", "util.ink").unwrap();
        // No outbound includes — the moved file's content is untouched.
        assert_eq!(ns(&result), "=== helper ===\n-> END\n");
        // main.ink's INCLUDE token is re-pointed at the new bare name.
        assert_eq!(result.cross_file_edits.len(), 1);
        assert_eq!(result.cross_file_edits[0].new_text, "util.ink");
    }

    #[test]
    fn move_into_subdir_rewrites_inbound_and_outbound() {
        let s = session(&[
            ("main.ink", "INCLUDE intro.ink\n-> END\n"),
            ("intro.ink", "INCLUDE lib.ink\n=== intro ===\n-> END\n"),
            ("lib.ink", "=== helper ===\n-> END\n"),
        ]);
        let result = rename_file(&s, "intro.ink", "scenes/intro.ink").unwrap();
        // Outbound: intro's own include of lib.ink, now one level deeper → ../lib.ink.
        assert!(
            ns(&result).contains("INCLUDE ../lib.ink"),
            "outbound not rewritten: {}",
            ns(&result)
        );
        // Inbound: main.ink now includes scenes/intro.ink.
        assert_eq!(result.cross_file_edits.len(), 1);
        assert_eq!(result.cross_file_edits[0].new_text, "scenes/intro.ink");
    }

    #[test]
    fn move_shallower_rewrites_outbound_to_bare_names() {
        // Regression (#318): moving a file UP a directory (chapters/main.ink →
        // main.ink) must rewrite its own relative includes from `../host.ink`
        // to the bare `host.ink`, not leave them pointing at the old dir.
        let s = session(&[
            (
                "chapters/main.ink",
                "INCLUDE ../host.ink\nINCLUDE ../phone.ink\n-> END\n",
            ),
            ("host.ink", "=== host ===\n-> END\n"),
            ("phone.ink", "=== phone ===\n-> END\n"),
        ]);
        let result = rename_file(&s, "chapters/main.ink", "main.ink").unwrap();
        assert!(
            ns(&result).contains("INCLUDE host.ink"),
            "host include not rewritten to bare name: {}",
            ns(&result)
        );
        assert!(
            ns(&result).contains("INCLUDE phone.ink"),
            "phone include not rewritten to bare name: {}",
            ns(&result)
        );
        assert!(
            !ns(&result).contains("chapters/host.ink"),
            "stale chapters/ prefix leaked: {}",
            ns(&result)
        );
        assert!(
            !ns(&result).contains("../host.ink"),
            "stale ../ prefix leaked: {}",
            ns(&result)
        );
    }

    #[test]
    fn move_deeper_then_shallower_round_trips_outbound() {
        // Regression (#318): a deeper move followed by the inverse shallower
        // move restores the original include text exactly.
        let original = "INCLUDE lib.ink\n=== intro ===\n-> END\n";
        let s = session(&[
            ("intro.ink", original),
            ("lib.ink", "=== helper ===\n-> END\n"),
        ]);

        // Deeper: intro.ink → scenes/intro.ink, so `lib.ink` → `../lib.ink`.
        let deeper = rename_file(&s, "intro.ink", "scenes/intro.ink").unwrap();
        assert!(
            ns(&deeper).contains("INCLUDE ../lib.ink"),
            "deeper move did not rewrite outbound: {}",
            ns(&deeper)
        );

        // Rebuild the session as if the deeper move had been applied, then move
        // back to the root.
        let deeper_source = ns(&deeper).to_owned();
        let s2 = session(&[
            ("scenes/intro.ink", &deeper_source),
            ("lib.ink", "=== helper ===\n-> END\n"),
        ]);
        let shallower = rename_file(&s2, "scenes/intro.ink", "intro.ink").unwrap();
        assert_eq!(
            ns(&shallower),
            original,
            "round-trip did not restore original include text",
        );
    }

    #[test]
    fn rename_noop_when_old_equals_new() {
        let s = session(&[("a.ink", "INCLUDE b.ink\n-> END\n"), ("b.ink", "-> END\n")]);
        let r = rename_file(&s, "a.ink", "a.ink").unwrap();
        assert!(r.cross_file_edits.is_empty());
        assert_eq!(ns(&r), "INCLUDE b.ink\n-> END\n");
    }

    #[test]
    fn rename_errors_when_destination_exists() {
        let s = session(&[("a.ink", "-> END\n"), ("b.ink", "-> END\n")]);
        assert!(rename_file(&s, "a.ink", "b.ink").is_err());
    }

    #[test]
    fn rename_errors_when_source_missing() {
        let s = session(&[("a.ink", "-> END\n")]);
        assert!(rename_file(&s, "ghost.ink", "x.ink").is_err());
    }
}
