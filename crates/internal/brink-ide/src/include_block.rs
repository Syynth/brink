//! Shared detector for the leading `INCLUDE` block (#312 + #313).
//!
//! Both the include-block fold ([`crate::folding`]) and the auto-import
//! INCLUDE-insertion edit ([`crate::auto_import`]) need to agree on *where*
//! the file's leading run of `INCLUDE` statements lives. [`include_block_span`]
//! is the single source of truth: it derives the span from `hir.includes`
//! (already lowered `IncludeSite`s) mapped through the [`LineIndex`].
//!
//! A "leading INCLUDE block" is the contiguous run of `INCLUDE` statements at
//! the top of the file, tolerating:
//!
//! - blank lines interleaved between the includes (and above the run);
//! - a single leading `///` / `//` comment / front-matter block above the run.
//!
//! Anything else (a knot header, narrative text, a `VAR`/`CONST`/`LIST`
//! declaration, etc.) terminates the leading run: only `INCLUDE`s that sit in
//! that top-of-file region count.

use brink_ir::HirFile;

use crate::LineIndex;

/// The leading `INCLUDE` block of a file.
///
/// `start_line` / `end_line` are 0-based and span only the `INCLUDE`
/// statement lines (the first include line to the last include line in the
/// leading run); any leading comment block above the run is *not* included in
/// the span. `count` is how many `INCLUDE` statements are in the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncludeBlockSpan {
    pub start_line: u32,
    pub end_line: u32,
    pub count: usize,
}

/// Detect the contiguous leading run of `INCLUDE` statements.
///
/// Returns `None` when the file has no `INCLUDE` in its leading region (i.e.
/// any `INCLUDE`s are preceded by non-comment, non-blank content).
#[must_use]
pub fn include_block_span(hir: &HirFile, source: &str) -> Option<IncludeBlockSpan> {
    if hir.includes.is_empty() {
        return None;
    }

    let idx = LineIndex::new(source);

    // Lines (0-based) of every INCLUDE statement, sorted ascending. Multiple
    // includes never share a line, but sorting keeps us robust regardless of
    // `hir.includes` ordering.
    let mut include_lines: Vec<u32> = hir
        .includes
        .iter()
        .map(|inc| idx.line_col(inc.ptr.text_range().start()).0)
        .collect();
    include_lines.sort_unstable();

    // The end of the leading comment/blank region: the first line that is
    // neither blank nor a `//` / `///` comment. The leading INCLUDE run may
    // only begin at or after the comment block, and must start at the very
    // first line of "real" content.
    let lines: Vec<&str> = source.lines().collect();
    let first_content_line = lines
        .iter()
        .position(|line| {
            let t = line.trim_start();
            !t.is_empty() && !t.starts_with("//")
        })
        .unwrap_or(lines.len());

    // The leading run is only a leading run if the very first piece of real
    // content is an INCLUDE.
    let first_include = include_lines[0] as usize;
    if first_include != first_content_line {
        return None;
    }

    // Walk the includes in order, extending the run as long as every line
    // between consecutive includes is blank (interleaved blank lines are
    // tolerated; anything else terminates the run).
    let mut end_line = include_lines[0];
    let mut count = 1usize;
    for &line in &include_lines[1..] {
        let gap_all_blank = ((end_line + 1)..line)
            .all(|l| lines.get(l as usize).is_none_or(|s| s.trim().is_empty()));
        if !gap_all_blank {
            break;
        }
        end_line = line;
        count += 1;
    }

    Some(IncludeBlockSpan {
        start_line: include_lines[0],
        end_line,
        count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span_for(src: &str) -> Option<IncludeBlockSpan> {
        let parsed = brink_syntax::parse(src);
        let (hir, _, _) = brink_ir::hir::lower(brink_ir::FileId(0), &parsed.tree());
        include_block_span(&hir, src)
    }

    #[test]
    fn multi_include_run() {
        let src = "INCLUDE a.ink\nINCLUDE b.ink\nINCLUDE c.ink\n== hub ==\ntext\n";
        let span = span_for(src).expect("leading include block");
        assert_eq!(span.start_line, 0);
        assert_eq!(span.end_line, 2);
        assert_eq!(span.count, 3);
    }

    #[test]
    fn single_include_still_detected() {
        let src = "INCLUDE a.ink\n== hub ==\ntext\n";
        let span = span_for(src).expect("single include still detected");
        assert_eq!(span.start_line, 0);
        assert_eq!(span.end_line, 0);
        assert_eq!(span.count, 1);
    }

    #[test]
    fn blank_lines_between_includes_tolerated() {
        let src = "INCLUDE a.ink\n\nINCLUDE b.ink\n\n\nINCLUDE c.ink\n== hub ==\n";
        let span = span_for(src).expect("blank-separated run");
        assert_eq!(span.start_line, 0);
        assert_eq!(span.end_line, 5);
        assert_eq!(span.count, 3);
    }

    #[test]
    fn leading_comment_block_tolerated() {
        let src = "\
// Front matter.
// Author: someone.
INCLUDE a.ink
INCLUDE b.ink
== hub ==
";
        let span = span_for(src).expect("run after comment block");
        assert_eq!(span.start_line, 2);
        assert_eq!(span.end_line, 3);
        assert_eq!(span.count, 2);
    }

    #[test]
    fn leading_doc_comment_block_tolerated() {
        let src = "\
/// Title.
/// More.

INCLUDE a.ink
INCLUDE b.ink
== hub ==
";
        let span = span_for(src).expect("run after doc-comment block");
        assert_eq!(span.start_line, 3);
        assert_eq!(span.end_line, 4);
        assert_eq!(span.count, 2);
    }

    #[test]
    fn run_terminates_at_non_include_content() {
        // The third INCLUDE follows a knot, so it is not part of the leading
        // run.
        let src = "INCLUDE a.ink\nINCLUDE b.ink\n== hub ==\nINCLUDE c.ink\n";
        let span = span_for(src).expect("leading run before the knot");
        assert_eq!(span.start_line, 0);
        assert_eq!(span.end_line, 1);
        assert_eq!(span.count, 2);
    }

    #[test]
    fn include_not_at_top_is_not_a_leading_block() {
        let src = "VAR x = 1\nINCLUDE a.ink\n";
        assert_eq!(span_for(src), None);
    }

    #[test]
    fn no_includes_yields_none() {
        let src = "== hub ==\ntext\n";
        assert_eq!(span_for(src), None);
    }
}
