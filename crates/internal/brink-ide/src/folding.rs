use brink_ir::{Block, Content, ContentPart, HirFile, Stmt};
use rowan::TextRange;

use crate::LineIndex;

/// A foldable range in the document.
pub struct FoldRange {
    pub start_line: u32,
    pub end_line: u32,
    pub collapsed_text: Option<String>,
}

/// Compute folding ranges for a file from its HIR.
pub fn folding_ranges(hir: &HirFile, source: &str) -> Vec<FoldRange> {
    let idx = LineIndex::new(source);
    let mut ranges = Vec::new();

    // Root-level block content
    collect_block_folds(&hir.root_content, source, &idx, &mut ranges);

    for knot in &hir.knots {
        push_fold(knot.ptr.text_range(), None, source, &idx, &mut ranges);
        collect_block_folds(&knot.body, source, &idx, &mut ranges);

        for stitch in &knot.stitches {
            push_fold(stitch.ptr.text_range(), None, source, &idx, &mut ranges);
            collect_block_folds(&stitch.body, source, &idx, &mut ranges);
        }
    }

    collect_doc_comment_folds(source, &mut ranges);

    ranges
}

/// Emit a fold range for each contiguous multi-line `///` doc-comment block,
/// so long docs can collapse independently of the declaration they precede.
/// (The declaration's own fold starts at its header, so it can never hide a
/// doc block sitting above it.)
fn collect_doc_comment_folds(source: &str, out: &mut Vec<FoldRange>) {
    let mut run_start: Option<u32> = None;
    let mut prev_line = 0u32;
    for (i, line) in source.lines().enumerate() {
        let line_no = u32::try_from(i).unwrap_or(u32::MAX);
        if line.trim_start().starts_with("///") {
            run_start.get_or_insert(line_no);
            prev_line = line_no;
        } else if let Some(start) = run_start.take()
            && prev_line > start
        {
            out.push(FoldRange {
                start_line: start,
                end_line: prev_line,
                collapsed_text: None,
            });
        }
    }
    if let Some(start) = run_start
        && prev_line > start
    {
        out.push(FoldRange {
            start_line: start,
            end_line: prev_line,
            collapsed_text: None,
        });
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

    fn ranges_for(src: &str) -> Vec<(u32, u32)> {
        let parsed = brink_syntax::parse(src);
        let (hir, _, _) = brink_ir::hir::lower(brink_ir::FileId(0), &parsed.tree());
        folding_ranges(&hir, src)
            .iter()
            .map(|r| (r.start_line, r.end_line))
            .collect()
    }

    #[test]
    fn multi_line_doc_blocks_get_their_own_fold() {
        let src = "\
/// Damage roll.
/// @param weapon {int}
/// @returns {int}
== function damage(weapon) ==
~ return 1
";
        let ranges = ranges_for(src);
        assert!(
            ranges.contains(&(0, 2)),
            "doc block lines 0-2 fold: {ranges:?}"
        );
        assert!(
            ranges.contains(&(3, 4)),
            "knot fold still anchored on its header: {ranges:?}"
        );
    }

    #[test]
    fn single_line_docs_do_not_fold() {
        let src = "/// one line\n== hub ==\ntext\nmore\n";
        let ranges = ranges_for(src);
        assert!(
            !ranges.iter().any(|&(s, _)| s == 0),
            "single-line doc block has nothing to fold: {ranges:?}"
        );
    }
}
