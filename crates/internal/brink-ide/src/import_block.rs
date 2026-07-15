//! Shared detector for the leading `IMPORT` block (M-4, docs/modules-spec.md
//! §2/§9).
//!
//! Both the import-block fold ([`crate::folding`]) and the auto-import
//! `IMPORT`-insertion quick-fix ([`crate::import_fix`]) need to agree on
//! *where* the file's run of module `IMPORT` statements lives.
//! [`import_block_span`] is the single source of truth: it derives the span
//! from `hir.imports` (already lowered [`brink_ir::Import`]s) mapped through
//! the [`LineIndex`].
//!
//! Imports conventionally sit at the top of a file, directly below any
//! `INCLUDE` block and `#@module` header. A "leading `IMPORT` block" is the
//! contiguous run of `IMPORT` statements in that top region, tolerating:
//!
//! - blank lines interleaved between the imports (and above the run);
//! - any leading `//` / `///` comment / front-matter block, `#@module`
//!   directive, and `INCLUDE` statements above the run.
//!
//! Anything else (a knot header, narrative text, a `VAR`/`CONST`/`LIST`
//! declaration, etc.) terminates the leading region: only `IMPORT`s that sit
//! in that top-of-file region count.

use brink_ir::HirFile;

use crate::LineIndex;

/// The leading `IMPORT` block of a file.
///
/// `start_line` / `end_line` are 0-based and span only the `IMPORT` statement
/// lines (the first import line to the last import line in the leading run);
/// any comment / `#@module` / `INCLUDE` region above the run is *not* included
/// in the span. `count` is how many `IMPORT` statements are in the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportBlockSpan {
    pub start_line: u32,
    pub end_line: u32,
    pub count: usize,
}

/// Detect the contiguous leading run of `IMPORT` statements.
///
/// Returns `None` when the file has no `IMPORT` in its leading region (i.e.
/// any `IMPORT`s are preceded by non-comment, non-blank, non-`INCLUDE`,
/// non-`#@module` content).
#[must_use]
pub fn import_block_span(hir: &HirFile, source: &str) -> Option<ImportBlockSpan> {
    if hir.imports.is_empty() {
        return None;
    }

    let idx = LineIndex::new(source);

    // Lines (0-based) of every IMPORT statement, sorted ascending. Multiple
    // imports never share a line, but sorting keeps us robust regardless of
    // `hir.imports` ordering.
    let mut import_lines: Vec<u32> = hir
        .imports
        .iter()
        .map(|imp| idx.line_col(imp.range.start()).0)
        .collect();
    import_lines.sort_unstable();

    let lines: Vec<&str> = source.lines().collect();

    // The end of the leading "header" region: the first line that is not a
    // permissible header line (blank / comment / `#@module` directive /
    // `INCLUDE`). The leading IMPORT run may only begin at or after this, and
    // must start at the very first line of "real" (non-header) content.
    let first_content_line = lines
        .iter()
        .position(|line| !is_header_line(line))
        .map_or(lines.len(), |p| p);

    // The leading run is only a leading run if the very first piece of real
    // (non-header) content is an IMPORT.
    let first_import = import_lines[0] as usize;
    if first_import != first_content_line {
        return None;
    }

    // Walk the imports in order, extending the run as long as every line
    // between consecutive imports is blank (interleaved blank lines are
    // tolerated; anything else terminates the run).
    let mut end_line = import_lines[0];
    let mut count = 1usize;
    for &line in &import_lines[1..] {
        let gap_all_blank = ((end_line + 1)..line)
            .all(|l| lines.get(l as usize).is_none_or(|s| s.trim().is_empty()));
        if !gap_all_blank {
            break;
        }
        end_line = line;
        count += 1;
    }

    Some(ImportBlockSpan {
        start_line: import_lines[0],
        end_line,
        count,
    })
}

/// A header line that may legally precede the leading `IMPORT` run: blank, a
/// `//` / `///` comment, a `#@module` directive, or an `INCLUDE` statement.
fn is_header_line(line: &str) -> bool {
    let t = line.trim_start();
    t.is_empty() || t.starts_with("//") || t.starts_with("#@module") || t.starts_with("INCLUDE")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span_for(src: &str) -> Option<ImportBlockSpan> {
        let parsed = brink_syntax::parse(src);
        let (hir, _, _) = brink_ir::hir::lower(brink_ir::FileId(0), &parsed.tree());
        import_block_span(&hir, src)
    }

    #[test]
    fn multi_import_run() {
        let src = "IMPORT a\nIMPORT b\nIMPORT c\n== hub ==\ntext\n";
        let span = span_for(src).expect("leading import block");
        assert_eq!(span.start_line, 0);
        assert_eq!(span.end_line, 2);
        assert_eq!(span.count, 3);
    }

    #[test]
    fn single_import_still_detected() {
        let src = "IMPORT a\n== hub ==\ntext\n";
        let span = span_for(src).expect("single import still detected");
        assert_eq!(span.start_line, 0);
        assert_eq!(span.end_line, 0);
        assert_eq!(span.count, 1);
    }

    #[test]
    fn blank_lines_between_imports_tolerated() {
        let src = "IMPORT a\n\nIMPORT b\n\n\nIMPORT c\n== hub ==\n";
        let span = span_for(src).expect("blank-separated run");
        assert_eq!(span.start_line, 0);
        assert_eq!(span.end_line, 5);
        assert_eq!(span.count, 3);
    }

    #[test]
    fn imports_below_module_and_include_header() {
        let src = "\
#@module(quest)
// a comment
INCLUDE shared.ink
IMPORT { ambush } FROM quest_3
IMPORT quest_4
== hub ==
";
        let span = span_for(src).expect("run after module/include header");
        assert_eq!(span.start_line, 3);
        assert_eq!(span.end_line, 4);
        assert_eq!(span.count, 2);
    }

    #[test]
    fn run_terminates_at_non_import_content() {
        // The third IMPORT follows a knot, so it is not part of the leading
        // run.
        let src = "IMPORT a\nIMPORT b\n== hub ==\nIMPORT c\n";
        let span = span_for(src).expect("leading run before the knot");
        assert_eq!(span.start_line, 0);
        assert_eq!(span.end_line, 1);
        assert_eq!(span.count, 2);
    }

    #[test]
    fn import_after_var_is_not_a_leading_block() {
        let src = "VAR x = 1\nIMPORT a\n";
        assert_eq!(span_for(src), None);
    }

    #[test]
    fn no_imports_yields_none() {
        let src = "== hub ==\ntext\n";
        assert_eq!(span_for(src), None);
    }
}
