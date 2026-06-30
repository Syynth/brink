use brink_ir::{Block, Content, ContentPart, HirFile, Stmt};
use rowan::TextRange;

use crate::LineIndex;

/// A foldable range in the document.
pub struct FoldRange {
    pub start_line: u32,
    pub end_line: u32,
    pub collapsed_text: Option<String>,
    /// Fold from the *start* of `start_line` (hiding the whole line) rather
    /// than from its end. Used for declaration folds that include the doc
    /// block and header; the editor renders the hidden header as the
    /// collapsed placeholder.
    pub from_line_start: bool,
}

/// Compute folding ranges for a file from its HIR.
pub fn folding_ranges(hir: &HirFile, source: &str) -> Vec<FoldRange> {
    let idx = LineIndex::new(source);
    let mut ranges = Vec::new();

    // Leading INCLUDE block (#313 G): collapse a run of two-or-more leading
    // INCLUDEs into one region. The span is derived from the shared
    // `include_block_span` detector so fold and auto-import agree on its
    // bounds. A single INCLUDE is detected by the shared helper but is not
    // worth folding.
    if let Some(span) = crate::include_block::include_block_span(hir, source)
        && span.count >= 2
    {
        ranges.push(FoldRange {
            start_line: span.start_line,
            end_line: span.end_line,
            collapsed_text: Some(format!("INCLUDE … ({} files)", span.count)),
            from_line_start: false,
        });
    }

    // Root-level block content
    collect_block_folds(&hir.root_content, source, &idx, &mut ranges);

    // Doc blocks consumed by a declaration fold (tracked by their first
    // line) — they must not also fold as standalone comment blocks.
    let mut consumed_doc_lines: Vec<u32> = Vec::new();

    for (ki, knot) in hir.knots.iter().enumerate() {
        // Clamp the fold before the next declaration's doc block — the syntax
        // node swallows all trailing trivia up to the next header, and
        // folding a knot must not hide the next knot's docs.
        let next_knot_start = hir.knots.get(ki + 1).map(|n| n.ptr.text_range().start());
        let knot_range = clamp_before_next_docs(source, knot.ptr.text_range(), next_knot_start);
        push_decl_fold(
            knot_range,
            source,
            &idx,
            &mut ranges,
            &mut consumed_doc_lines,
        );
        collect_block_folds(&knot.body, source, &idx, &mut ranges);

        for (si, stitch) in knot.stitches.iter().enumerate() {
            let next_start = knot
                .stitches
                .get(si + 1)
                .map_or(knot_range.end(), |n| n.ptr.text_range().start());
            let stitch_range =
                clamp_before_next_docs(source, stitch.ptr.text_range(), Some(next_start));
            push_decl_fold(
                stitch_range,
                source,
                &idx,
                &mut ranges,
                &mut consumed_doc_lines,
            );
            collect_block_folds(&stitch.body, source, &idx, &mut ranges);
        }
    }

    collect_doc_comment_folds(source, &consumed_doc_lines, &mut ranges);

    ranges
}

/// Push the fold for a knot/stitch declaration. A documented declaration
/// folds as a single region from the first line of its `///` doc block
/// (whole-line fold; the editor renders the hidden header as the collapsed
/// placeholder). An undocumented one folds from its header line as before.
fn push_decl_fold(
    range: TextRange,
    source: &str,
    idx: &LineIndex,
    out: &mut Vec<FoldRange>,
    consumed_doc_lines: &mut Vec<u32>,
) {
    let decl_start = usize::from(range.start());
    let doc_start = crate::doc_extended_start(source, decl_start);
    if doc_start >= decl_start {
        push_fold(range, None, source, idx, out);
        return;
    }

    // Trim trailing whitespace off the fold, mirroring push_fold.
    let end_byte = usize::from(range.end()).min(source.len());
    let trimmed_end = doc_start + source[doc_start..end_byte].trim_end().len();
    if trimmed_end <= doc_start {
        return;
    }

    let (start_line, _) = idx.line_col(rowan::TextSize::from(
        u32::try_from(doc_start).unwrap_or(u32::MAX),
    ));
    let (end_line, _) = idx.line_col(rowan::TextSize::from(
        u32::try_from(trimmed_end).unwrap_or(u32::MAX),
    ));
    consumed_doc_lines.push(start_line);
    if end_line > start_line {
        out.push(FoldRange {
            start_line,
            end_line,
            collapsed_text: None,
            from_line_start: true,
        });
    }
}

/// Clamp a declaration's range end before the next declaration's attached
/// `///` doc block, so folding it never hides the next declaration's docs.
fn clamp_before_next_docs(
    source: &str,
    range: TextRange,
    next_decl_start: Option<rowan::TextSize>,
) -> TextRange {
    let end = next_decl_start.map_or(range.end(), |next| {
        let next_owned = crate::doc_extended_start(source, next.into());
        range.end().min(rowan::TextSize::from(
            u32::try_from(next_owned).unwrap_or(u32::MAX),
        ))
    });
    TextRange::new(range.start().min(end), end)
}

/// Emit a fold range for each contiguous multi-line `///` doc-comment block
/// not already consumed by a declaration fold (knot/stitch doc blocks fold
/// together with their declaration), so long standalone docs — e.g. on VAR /
/// CONST / EXTERNAL declarations — can still collapse on their own.
fn collect_doc_comment_folds(source: &str, consumed_doc_lines: &[u32], out: &mut Vec<FoldRange>) {
    let mut emit = |start: u32, end: u32| {
        if end > start && !consumed_doc_lines.contains(&start) {
            out.push(FoldRange {
                start_line: start,
                end_line: end,
                collapsed_text: None,
                from_line_start: false,
            });
        }
    };
    let mut run_start: Option<u32> = None;
    let mut prev_line = 0u32;
    for (i, line) in source.lines().enumerate() {
        let line_no = u32::try_from(i).unwrap_or(u32::MAX);
        if line.trim_start().starts_with("///") {
            run_start.get_or_insert(line_no);
            prev_line = line_no;
        } else if let Some(start) = run_start.take() {
            emit(start, prev_line);
        }
    }
    if let Some(start) = run_start {
        emit(start, prev_line);
    }
}

fn push_fold(
    range: TextRange,
    collapsed: Option<String>,
    source: &str,
    idx: &LineIndex,
    out: &mut Vec<FoldRange>,
) {
    let start_byte = usize::from(range.start());
    let mut end_byte = usize::from(range.end()).min(source.len());
    let slice = &source[start_byte..end_byte];

    let trimmed_start = start_byte + (slice.len() - slice.trim_start().len());
    let mut trimmed_end = start_byte + slice.trim_end().len();

    // The HIR ptr for Conditional/Sequence covers only the inner
    // CONDITIONAL_WITH_EXPR / MULTILINE_BRANCHES_COND node, not the enclosing
    // `{ ... }`. Extend the fold backward to include `{` and forward to `}`
    // when they sit on separate lines.
    let mut trimmed_start = trimmed_start;
    if collapsed.as_deref() == Some("{...}") {
        let before = &source.as_bytes()[..trimmed_start];
        let mut j = before.len();
        while j > 0 && (before[j - 1] == b' ' || before[j - 1] == b'\t' || before[j - 1] == b'\n') {
            j -= 1;
        }
        if j > 0 && before[j - 1] == b'{' {
            trimmed_start = j - 1;
        }

        let after = &source.as_bytes()[end_byte..];
        let mut i = 0;
        while i < after.len() && (after[i] == b' ' || after[i] == b'\t' || after[i] == b'\n') {
            i += 1;
        }
        if i < after.len() && after[i] == b'}' {
            end_byte += i + 1;
            trimmed_end = end_byte;
        }
    }

    if trimmed_start >= trimmed_end {
        return;
    }

    let (start_line, _) = idx.line_col(rowan::TextSize::from(
        u32::try_from(trimmed_start).unwrap_or(u32::MAX),
    ));
    let (end_line, _) = idx.line_col(rowan::TextSize::from(
        u32::try_from(trimmed_end).unwrap_or(u32::MAX),
    ));
    if end_line > start_line {
        out.push(FoldRange {
            start_line,
            end_line,
            collapsed_text: collapsed,
            from_line_start: false,
        });
    }
}

fn collect_block_folds(block: &Block, source: &str, idx: &LineIndex, out: &mut Vec<FoldRange>) {
    for stmt in &block.stmts {
        collect_stmt_folds(stmt, source, idx, out);
    }
}

fn collect_stmt_folds(stmt: &Stmt, source: &str, idx: &LineIndex, out: &mut Vec<FoldRange>) {
    match stmt {
        Stmt::ChoiceSet(cs) => {
            for choice in &cs.choices {
                push_fold(choice.ptr.text_range(), None, source, idx, out);
                collect_block_folds(&choice.body, source, idx, out);
            }
            collect_block_folds(&cs.continuation, source, idx, out);
        }
        Stmt::LabeledBlock(block) => {
            collect_block_folds(block, source, idx, out);
        }
        Stmt::Conditional(cond) => {
            push_fold(
                cond.ptr.text_range(),
                Some("{...}".to_owned()),
                source,
                idx,
                out,
            );
            for branch in &cond.branches {
                collect_block_folds(&branch.body, source, idx, out);
            }
        }
        Stmt::Sequence(seq) => {
            push_fold(
                seq.ptr.text_range(),
                Some("{...}".to_owned()),
                source,
                idx,
                out,
            );
            for branch in &seq.branches {
                collect_block_folds(branch, source, idx, out);
            }
        }
        Stmt::Content(content) => {
            collect_content_folds(content, source, idx, out);
        }
        _ => {}
    }
}

fn collect_content_folds(
    content: &Content,
    source: &str,
    idx: &LineIndex,
    out: &mut Vec<FoldRange>,
) {
    collect_content_part_folds(&content.parts, source, idx, out);
}

fn collect_content_part_folds(
    parts: &[ContentPart],
    source: &str,
    idx: &LineIndex,
    out: &mut Vec<FoldRange>,
) {
    for part in parts {
        match part {
            ContentPart::InlineConditional(cond) => {
                push_fold(
                    cond.ptr.text_range(),
                    Some("{...}".to_owned()),
                    source,
                    idx,
                    out,
                );
                for branch in &cond.branches {
                    collect_block_folds(&branch.body, source, idx, out);
                }
            }
            ContentPart::InlineSequence(seq) => {
                push_fold(
                    seq.ptr.text_range(),
                    Some("{...}".to_owned()),
                    source,
                    idx,
                    out,
                );
                for branch in &seq.branches {
                    collect_block_folds(branch, source, idx, out);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::folding_ranges;

    /// `(start_line, end_line, from_line_start)` triples for `src`.
    fn ranges_for(src: &str) -> Vec<(u32, u32, bool)> {
        let parsed = brink_syntax::parse(src);
        let (hir, _, _) = brink_ir::hir::lower(brink_ir::FileId(0), &parsed.tree());
        folding_ranges(&hir, src)
            .iter()
            .map(|r| (r.start_line, r.end_line, r.from_line_start))
            .collect()
    }

    #[test]
    fn documented_knot_folds_as_one_region_from_its_docs() {
        let src = "\
/// Damage roll.
/// @param weapon {int}
/// @returns {int}
== function damage(weapon) ==
~ return 1
";
        let ranges = ranges_for(src);
        assert!(
            ranges.contains(&(0, 4, true)),
            "single whole-line fold spanning docs + header + body: {ranges:?}"
        );
        assert!(
            !ranges.contains(&(0, 2, false)),
            "the doc block must not also fold separately: {ranges:?}"
        );
    }

    #[test]
    fn undocumented_knot_folds_from_its_header() {
        let src = "== hub ==\ntext\nmore\n";
        let ranges = ranges_for(src);
        assert!(ranges.contains(&(0, 2, false)), "{ranges:?}");
    }

    #[test]
    fn knot_fold_stops_before_next_knots_docs() {
        let src = "\
=== carrying ===
~ return 1

/// Uniform random.
/// @returns {int}
=== roll ===
~ return 0
";
        let ranges = ranges_for(src);
        // carrying's fold (anchored line 0) must end before roll's doc block
        // (line 3), not swallow it as trailing trivia.
        let carrying = ranges
            .iter()
            .find(|&&(s, _, _)| s == 0)
            .copied()
            .expect("carrying fold");
        assert!(
            carrying.1 < 3,
            "carrying fold must not hide roll's docs: {ranges:?}"
        );
        // roll folds as one region from its doc block.
        assert!(ranges.contains(&(3, 6, true)), "{ranges:?}");
    }

    #[test]
    fn standalone_doc_blocks_still_fold_separately() {
        // Docs on a VAR have no declaration fold — the block folds on its own.
        let src = "\
/// Player health,
/// clamped at zero.
VAR health = 100
== hub ==
text
";
        let ranges = ranges_for(src);
        assert!(ranges.contains(&(0, 1, false)), "{ranges:?}");
    }

    #[test]
    fn single_line_standalone_docs_do_not_fold() {
        let src = "/// one line\nVAR x = 1\n== hub ==\ntext\nmore\n";
        let ranges = ranges_for(src);
        assert!(
            !ranges.iter().any(|&(s, _, _)| s == 0),
            "a single-line standalone doc has nothing to fold: {ranges:?}"
        );
    }

    /// `(start_line, end_line, collapsed_text)` triples for `src`.
    fn folds_with_text(src: &str) -> Vec<(u32, u32, Option<String>)> {
        let parsed = brink_syntax::parse(src);
        let (hir, _, _) = brink_ir::hir::lower(brink_ir::FileId(0), &parsed.tree());
        super::folding_ranges(&hir, src)
            .into_iter()
            .map(|r| (r.start_line, r.end_line, r.collapsed_text))
            .collect()
    }

    #[test]
    fn include_block_folds_when_two_or_more() {
        let src = "INCLUDE a.ink\nINCLUDE b.ink\nINCLUDE c.ink\n== hub ==\ntext\n";
        let folds = folds_with_text(src);
        assert!(
            folds.contains(&(0, 2, Some("INCLUDE … (3 files)".to_owned()))),
            "{folds:?}"
        );
    }

    #[test]
    fn single_include_does_not_fold() {
        let src = "INCLUDE a.ink\n== hub ==\ntext\n";
        let folds = folds_with_text(src);
        assert!(
            !folds.iter().any(
                |(s, _, t)| *s == 0 && t.as_deref().is_some_and(|t| t.starts_with("INCLUDE …"))
            ),
            "a single INCLUDE must not fold: {folds:?}"
        );
    }
}
