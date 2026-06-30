//! Auto-import: ensure one file `INCLUDE`s another (#312 F core).
//!
//! Given a `current` file and a `target` file, [`ensure_include`] reports
//! whether `target` is already reachable from `current` via the forward
//! `INCLUDE` graph and, when it is not, produces the deterministic `TextEdit`
//! that inserts `INCLUDE <relpath>` into `current`.
//!
//! The insertion point is located by the shared
//! [`include_block_span`](crate::include_block::include_block_span):
//!
//! - if `current` already has a leading `INCLUDE` block, the new line is
//!   appended directly after the block's last `INCLUDE`;
//! - otherwise the line is inserted at the top of the file, after any leading
//!   `//` / `///` comment / front-matter block.
//!
//! The op is **idempotent**: when `target` is already reachable from `current`
//! (directly or transitively) no edit is produced. The relative path is
//! computed with [`compute_relative_path`] and a leading `./` is normalized
//! away to match how hand-written includes look.

use brink_db::compute_relative_path;

use crate::include_block::include_block_span;
use crate::line_convert::TextEdit;
use crate::session::IdeSession;

/// Errors from an auto-import request.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AutoImportError {
    /// The `current` file is not loaded in the session.
    #[error("current file '{0}' not found")]
    CurrentNotFound(String),
    /// The `target` file is not loaded in the session.
    #[error("target file '{0}' not found")]
    TargetNotFound(String),
}

/// The result of an auto-import request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoImport {
    /// Whether `target` was already reachable from `current` before this op.
    pub already_reachable: bool,
    /// The edit that inserts the new `INCLUDE`, or `None` when the import is
    /// already satisfied (idempotent no-op).
    pub edit: Option<TextEdit>,
}

/// Ensure `current` `INCLUDE`s `target` (directly or transitively).
///
/// Returns whether `target` was already reachable and, if not, the
/// `INCLUDE`-insertion edit to apply to `current`'s source.
///
/// # Errors
/// [`AutoImportError::CurrentNotFound`] / [`AutoImportError::TargetNotFound`]
/// when either path is not loaded in the session.
pub fn ensure_include(
    session: &IdeSession,
    current: &str,
    target: &str,
) -> Result<AutoImport, AutoImportError> {
    let current_id = session
        .file_id(current)
        .ok_or_else(|| AutoImportError::CurrentNotFound(current.to_owned()))?;
    let target_id = session
        .file_id(target)
        .ok_or_else(|| AutoImportError::TargetNotFound(target.to_owned()))?;

    // Idempotent: reachability already satisfied (entry is reachable from
    // itself, so importing a file into itself is also a no-op).
    if session.db().reachable_from(current_id).contains(&target_id) {
        return Ok(AutoImport {
            already_reachable: true,
            edit: None,
        });
    }

    let source = session
        .source(current_id)
        .ok_or_else(|| AutoImportError::CurrentNotFound(current.to_owned()))?;
    let hir = session
        .hir(current_id)
        .ok_or_else(|| AutoImportError::CurrentNotFound(current.to_owned()))?;

    let rel = relative_include(current, target);
    let edit = insertion_edit(hir, source, &rel);

    Ok(AutoImport {
        already_reachable: false,
        edit: Some(edit),
    })
}

/// Lower-level variant that does not consult reachability — used where the
/// caller already knows an edit is wanted (and by tests).
#[must_use]
pub fn include_insertion_edit(hir: &brink_ir::HirFile, source: &str, rel_path: &str) -> TextEdit {
    insertion_edit(hir, source, rel_path)
}

/// Compute the relative `INCLUDE` target from `current` to `target`,
/// normalizing a leading `./` (a same-directory include is written bare).
fn relative_include(current: &str, target: &str) -> String {
    let rel = compute_relative_path(current, target);
    rel.strip_prefix("./").unwrap_or(&rel).to_owned()
}

/// Build the byte-range edit that inserts `INCLUDE <rel_path>` into `source`.
fn insertion_edit(hir: &brink_ir::HirFile, source: &str, rel_path: &str) -> TextEdit {
    let line = format!("INCLUDE {rel_path}");
    let byte = insertion_byte(hir, source);
    // The insertion byte is normally the start of a line, so a trailing `\n`
    // makes the INCLUDE its own line. But when `current` lacks a trailing
    // newline and the insertion point is the end of the file (e.g. a
    // single-INCLUDE or comment-only file), `byte` lands mid-line — directly
    // after the last character. Prepend a `\n` in that case so the new INCLUDE
    // is not concatenated onto the preceding line.
    let needs_leading_newline = byte > 0 && source.as_bytes().get(byte - 1) != Some(&b'\n');
    let insert = if needs_leading_newline {
        format!("\n{line}\n")
    } else {
        format!("{line}\n")
    };
    let at = u32::try_from(byte).unwrap_or(u32::MAX);
    TextEdit {
        from: at,
        to: at,
        insert,
    }
}

/// The byte offset (always at the start of a line) at which to insert the new
/// `INCLUDE` line.
fn insertion_byte(hir: &brink_ir::HirFile, source: &str) -> usize {
    if let Some(span) = include_block_span(hir, source) {
        // Append after the block's last INCLUDE line.
        line_start_byte(source, span.end_line + 1)
    } else {
        // No leading block: insert at the top, after any leading comment /
        // front-matter block.
        let after_comments = leading_comment_block_end(source);
        line_start_byte(source, after_comments)
    }
}

/// The 0-based line index of the first line that is **not** part of a leading
/// `//` / `///` comment / blank-line block.
fn leading_comment_block_end(source: &str) -> u32 {
    let mut last_comment_plus_one = 0u32;
    for (line, raw) in source.lines().enumerate() {
        let line = u32::try_from(line).unwrap_or(u32::MAX);
        let t = raw.trim_start();
        if t.starts_with("//") {
            last_comment_plus_one = line + 1;
        } else if !t.is_empty() {
            // First real, non-comment content terminates the leading block.
            break;
        }
    }
    last_comment_plus_one
}

/// Byte offset of the start of 0-based `line`. Lines past the end clamp to the
/// end of `source`.
fn line_start_byte(source: &str, line: u32) -> usize {
    if line == 0 {
        return 0;
    }
    let mut seen = 0u32;
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            seen += 1;
            if seen == line {
                return i + 1;
            }
        }
    }
    source.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hir_of(src: &str) -> brink_ir::HirFile {
        let parsed = brink_syntax::parse(src);
        let (hir, _, _) = brink_ir::hir::lower(brink_ir::FileId(0), &parsed.tree());
        hir
    }

    fn applied(src: &str, edit: &TextEdit) -> String {
        let mut out = String::new();
        out.push_str(&src[..edit.from as usize]);
        out.push_str(&edit.insert);
        out.push_str(&src[edit.to as usize..]);
        out
    }

    #[test]
    fn appends_into_existing_block() {
        let src = "INCLUDE a.ink\nINCLUDE b.ink\n== hub ==\ntext\n";
        let hir = hir_of(src);
        let edit = include_insertion_edit(&hir, src, "c.ink");
        let after = applied(src, &edit);
        assert_eq!(
            after,
            "INCLUDE a.ink\nINCLUDE b.ink\nINCLUDE c.ink\n== hub ==\ntext\n"
        );
    }

    #[test]
    fn inserts_at_top_when_no_block() {
        let src = "== hub ==\ntext\n";
        let hir = hir_of(src);
        let edit = include_insertion_edit(&hir, src, "c.ink");
        let after = applied(src, &edit);
        assert_eq!(after, "INCLUDE c.ink\n== hub ==\ntext\n");
    }

    #[test]
    fn inserts_after_leading_comment_block_when_no_includes() {
        let src = "// Front matter.\n// Author.\n== hub ==\n";
        let hir = hir_of(src);
        let edit = include_insertion_edit(&hir, src, "c.ink");
        let after = applied(src, &edit);
        assert_eq!(
            after,
            "// Front matter.\n// Author.\nINCLUDE c.ink\n== hub ==\n"
        );
    }

    #[test]
    fn relative_path_normalizes_dot_slash() {
        // Same-directory target: compute_relative_path yields a bare name, but
        // guard the `./` normalization regardless.
        assert_eq!(relative_include("a/main.ink", "a/utils.ink"), "utils.ink");
        assert_eq!(super::relative_include("./x.ink", "./y.ink"), "y.ink");
    }

    #[test]
    fn insertion_is_deterministic() {
        let src = "INCLUDE a.ink\n== hub ==\n";
        let hir = hir_of(src);
        let e1 = include_insertion_edit(&hir, src, "b.ink");
        let e2 = include_insertion_edit(&hir, src, "b.ink");
        assert_eq!(e1, e2);
    }

    #[test]
    fn single_include_without_trailing_newline_stays_on_its_own_line() {
        // The whole file is one INCLUDE and has no trailing newline; the
        // insertion point is the end of the file (mid-line). The new INCLUDE
        // must not be concatenated onto the existing one.
        let src = "INCLUDE a.ink";
        let hir = hir_of(src);
        let edit = include_insertion_edit(&hir, src, "c.ink");
        let after = applied(src, &edit);
        assert_eq!(after, "INCLUDE a.ink\nINCLUDE c.ink\n");
    }

    #[test]
    fn comment_only_without_trailing_newline_stays_on_its_own_line() {
        // A comment-only file with no trailing newline: the new INCLUDE must
        // land on its own line, not be swallowed into the comment.
        let src = "// hdr";
        let hir = hir_of(src);
        let edit = include_insertion_edit(&hir, src, "c.ink");
        let after = applied(src, &edit);
        assert_eq!(after, "// hdr\nINCLUDE c.ink\n");
    }

    #[test]
    fn multi_include_block_without_trailing_newline_stays_on_its_own_line() {
        let src = "INCLUDE a.ink\nINCLUDE b.ink";
        let hir = hir_of(src);
        let edit = include_insertion_edit(&hir, src, "c.ink");
        let after = applied(src, &edit);
        assert_eq!(after, "INCLUDE a.ink\nINCLUDE b.ink\nINCLUDE c.ink\n");
    }

    fn session_with(files: &[(&str, &str)]) -> IdeSession {
        let mut session = IdeSession::new();
        // First pass: register every file so each path has a `FileId`.
        for (path, src) in files {
            session.update_source(path, (*src).to_string());
        }
        // Second pass: re-update so every `INCLUDE` edge resolves now that all
        // target files exist in the db (edges only bind to existing ids).
        for (path, src) in files {
            session.update_and_analyze(path, (*src).to_string());
        }
        session
    }

    #[test]
    fn ensure_include_idempotent_when_directly_reachable() {
        let session = session_with(&[
            ("main.ink", "INCLUDE util.ink\n== hub ==\n"),
            ("util.ink", "== helper ==\n"),
        ]);
        let result = ensure_include(&session, "main.ink", "util.ink").expect("ensure");
        assert_eq!(
            result,
            AutoImport {
                already_reachable: true,
                edit: None,
            }
        );
    }

    #[test]
    fn ensure_include_idempotent_when_transitively_reachable() {
        let session = session_with(&[
            ("main.ink", "INCLUDE mid.ink\n== hub ==\n"),
            ("mid.ink", "INCLUDE leaf.ink\n== m ==\n"),
            ("leaf.ink", "== l ==\n"),
        ]);
        let result = ensure_include(&session, "main.ink", "leaf.ink").expect("ensure");
        assert!(result.already_reachable);
        assert_eq!(result.edit, None);
    }

    #[test]
    fn ensure_include_idempotent_for_self() {
        let session = session_with(&[("main.ink", "== hub ==\n")]);
        let result = ensure_include(&session, "main.ink", "main.ink").expect("ensure");
        assert!(result.already_reachable);
        assert_eq!(result.edit, None);
    }

    #[test]
    fn ensure_include_produces_edit_when_not_reachable() {
        let session = session_with(&[
            ("main.ink", "INCLUDE a.ink\n== hub ==\n"),
            ("a.ink", "== a ==\n"),
            ("b.ink", "== b ==\n"),
        ]);
        let result = ensure_include(&session, "main.ink", "b.ink").expect("ensure");
        assert!(!result.already_reachable);
        let edit = result.edit.expect("edit");
        let after = applied("INCLUDE a.ink\n== hub ==\n", &edit);
        assert_eq!(after, "INCLUDE a.ink\nINCLUDE b.ink\n== hub ==\n");
    }

    #[test]
    fn ensure_include_current_not_found() {
        let session = session_with(&[("target.ink", "== t ==\n")]);
        let err = ensure_include(&session, "missing.ink", "target.ink").unwrap_err();
        assert_eq!(err, AutoImportError::CurrentNotFound("missing.ink".into()));
    }

    #[test]
    fn ensure_include_target_not_found() {
        let session = session_with(&[("main.ink", "== hub ==\n")]);
        let err = ensure_include(&session, "main.ink", "missing.ink").unwrap_err();
        assert_eq!(err, AutoImportError::TargetNotFound("missing.ink".into()));
    }
}
