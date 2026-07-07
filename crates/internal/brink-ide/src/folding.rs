use brink_ir::{
    Block, Content, ContentContext, ContentPart, HirFile, HirVisitor, Stmt, walk_block,
};
use rowan::TextRange;

use crate::LineIndex;
use crate::line_context::LineContext;

/// The fold's kind (#365 — Celeris §5.5): every fold the editor exposes is
/// tagged so hosts can select which kinds auto-collapse per view mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldKind {
    /// Everything this module emitted before #365 (decls, doc comments,
    /// conditionals, sequences, choice sets). User-invoked in every mode;
    /// NEVER auto-collapsed by a host's mode entry.
    Structural,
    /// A maximal run of >=2 consecutive machinery-natured lines (logic `~`,
    /// VAR/CONST/LIST decls, standalone diverts, conditional/sequence
    /// scaffold lines). Run-based over the line classification — never
    /// HIR-block-based, so scaffold lines interleaved with narrative content
    /// never form a run.
    Machinery,
    /// A maximal run of >=2 consecutive narrative-natured lines (character/
    /// dialogue/parenthetical/prose) — the symmetric fold for logic-focused
    /// viewing.
    Narrative,
}

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
    /// The fold's kind (#365).
    pub kind: FoldKind,
}

/// Compute folding ranges for a file from its HIR.
///
/// All ranges from this pass are [`FoldKind::Structural`] — the
/// machinery/narrative run-based folds are computed separately by
/// [`machinery_and_narrative_folds`], since they require the per-line
/// `nature` facet (base classification, or a registered dialect's) rather
/// than HIR structure.
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
            kind: FoldKind::Structural,
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
            kind: FoldKind::Structural,
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
                kind: FoldKind::Structural,
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
            kind: FoldKind::Structural,
        });
    }
}

fn collect_block_folds(block: &Block, source: &str, idx: &LineIndex, out: &mut Vec<FoldRange>) {
    let mut collector = FoldCollector { source, idx, out };
    walk_block(block, &mut collector);
}

/// Emits a structural fold for every choice, conditional, and sequence via the
/// shared HIR visitor. Inline conditionals/sequences fold only when they sit in
/// body content — not in a choice's inline text — matching the pre-visitor
/// walk, which never descended a choice's start/bracket/inner content.
struct FoldCollector<'a> {
    source: &'a str,
    idx: &'a LineIndex,
    out: &'a mut Vec<FoldRange>,
}

impl HirVisitor for FoldCollector<'_> {
    fn enter_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::ChoiceSet(cs) => {
                for choice in &cs.choices {
                    push_fold(
                        choice.ptr.text_range(),
                        None,
                        self.source,
                        self.idx,
                        self.out,
                    );
                }
            }
            Stmt::Conditional(cond) => push_fold(
                cond.ptr.text_range(),
                Some("{...}".to_owned()),
                self.source,
                self.idx,
                self.out,
            ),
            Stmt::Sequence(seq) => push_fold(
                seq.ptr.text_range(),
                Some("{...}".to_owned()),
                self.source,
                self.idx,
                self.out,
            ),
            _ => {}
        }
    }

    fn enter_content(&mut self, content: &Content, ctx: ContentContext) {
        if ctx != ContentContext::Body {
            return;
        }
        for part in &content.parts {
            match part {
                ContentPart::InlineConditional(cond) => push_fold(
                    cond.ptr.text_range(),
                    Some("{...}".to_owned()),
                    self.source,
                    self.idx,
                    self.out,
                ),
                ContentPart::InlineSequence(seq) => push_fold(
                    seq.ptr.text_range(),
                    Some("{...}".to_owned()),
                    self.source,
                    self.idx,
                    self.out,
                ),
                _ => {}
            }
        }
    }
}

// ── Machinery/narrative fold runs (#365) ───────────────────────────────
//
// Run-based over the per-line classification (`LineContext`/dialect
// `nature`), never HIR-block-based: a conditional whose scaffold lines
// (`{ cond:`, `}`) are machinery-natured but whose branch bodies are prose
// must NOT fold as one machinery region just because it's one HIR node —
// the narrative lines in between break the run, exactly as they would for
// any other machinery/narrative sequence. This is what lets a mostly-
// narrative conditional (key case: a multi-line conditional whose branches
// are dialogue) stay unfolded while a pure-routing block (assignments/
// diverts only) collapses.

/// The 3-way nature of a line, for fold-run purposes. Mirrors
/// `brink_ir::ElementNature` but is computed per-line here from the
/// structural `LineElement` (when no dialect classified the line) or the
/// registered dialect's declared `nature` (when it did) — so a line's fold
/// nature always traces back to one authoritative source, never a
/// re-hardcoded pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineNature {
    Narrative,
    Machinery,
    /// Neither run type: blank lines, and structural lines that are not
    /// machinery scaffold (headers, choices, gathers, comments, tags,
    /// includes/externals).
    Structural,
}

/// Compute each line's [`LineNature`] from its `LineContext` plus the
/// literal source text (needed for two things `LineContext` doesn't itself
/// disambiguate: a *standalone* divert vs. a tunnel/thread call, and
/// conditional/sequence *scaffold* lines — the `{ cond:` / `}` lines the HIR
/// walk leaves structurally unclassified).
fn line_natures(source: &str, ctx: &[LineContext], scaffold_lines: &[bool]) -> Vec<LineNature> {
    use crate::line_context::LineElement;

    source
        .split('\n')
        .enumerate()
        .map(|(i, line)| {
            // A truly-blank line (no non-whitespace text) always breaks a
            // run, regardless of what the HIR's `element` says — a
            // multi-line `Content` node's span can cover an interior blank
            // line as `Narrative` (it's part of the same paragraph node),
            // but for fold-run purposes a blank line is never narrative.
            // Mirrors `line_context.rs::apply_dialect`'s `is_blank` check.
            if line.trim().is_empty() {
                return LineNature::Structural;
            }

            let Some(c) = ctx.get(i) else {
                return LineNature::Structural;
            };

            // A dialect classification, if present, is authoritative for
            // this line's nature — never re-derived from `LineElement`.
            if let Some(d) = &c.dialect {
                return match d.nature {
                    brink_ir::ElementNature::Narrative => LineNature::Narrative,
                    brink_ir::ElementNature::Machinery => LineNature::Machinery,
                    brink_ir::ElementNature::Structural => LineNature::Structural,
                };
            }

            match c.element {
                LineElement::Narrative => LineNature::Narrative,
                LineElement::Logic | LineElement::VarDecl => LineNature::Machinery,
                LineElement::Divert => {
                    let trimmed = line.trim_start();
                    let is_tunnel_or_thread = trimmed.starts_with("<-")
                        || (trimmed.starts_with("->") && trimmed.matches("->").count() > 1);
                    if is_tunnel_or_thread {
                        LineNature::Structural
                    } else {
                        LineNature::Machinery
                    }
                }
                _ => {
                    if scaffold_lines.get(i).copied().unwrap_or(false) && !line.trim().is_empty() {
                        LineNature::Machinery
                    } else {
                        LineNature::Structural
                    }
                }
            }
        })
        .collect()
}

/// Mark the opening/closing scaffold lines of every conditional and
/// sequence: the `{ cond:` / `{` header line and the closing `}` line,
/// located from the same HIR `ptr.text_range()` + brace-extension the
/// `{...}` fold uses (`push_fold`'s brace search), so the two never
/// disagree about where a conditional's braces sit. Only these two lines
/// per construct are scaffold — intermediate branch separators (`- else:`)
/// are left at their existing (structural) classification, since the HIR
/// gives no reliable per-branch source span to anchor them to.
fn mark_conditional_scaffold(hir: &HirFile, source: &str, idx: &LineIndex, out: &mut Vec<bool>) {
    let mut marker = ScaffoldMarker { source, idx, out };
    brink_ir::hir::visit::visit(hir, &mut marker);
}

/// Marks the opening/closing brace lines of every conditional and sequence via
/// the shared HIR visitor. Inline conditionals/sequences are marked only in
/// body content — not a choice's inline text — matching the pre-visitor walk.
struct ScaffoldMarker<'a> {
    source: &'a str,
    idx: &'a LineIndex,
    out: &'a mut Vec<bool>,
}

impl HirVisitor for ScaffoldMarker<'_> {
    fn enter_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Conditional(cond) => {
                mark_brace_span_scaffold(cond.ptr.text_range(), self.source, self.idx, self.out);
            }
            Stmt::Sequence(seq) => {
                mark_brace_span_scaffold(seq.ptr.text_range(), self.source, self.idx, self.out);
            }
            _ => {}
        }
    }

    fn enter_content(&mut self, content: &Content, ctx: ContentContext) {
        if ctx != ContentContext::Body {
            return;
        }
        for part in &content.parts {
            match part {
                ContentPart::InlineConditional(cond) => {
                    mark_brace_span_scaffold(
                        cond.ptr.text_range(),
                        self.source,
                        self.idx,
                        self.out,
                    );
                }
                ContentPart::InlineSequence(seq) => {
                    mark_brace_span_scaffold(seq.ptr.text_range(), self.source, self.idx, self.out);
                }
                _ => {}
            }
        }
    }
}

/// Mark the header and footer lines of one `{ ... }` construct as scaffold,
/// extending the HIR's inner-node range to the enclosing braces exactly as
/// `push_fold`'s `{...}` brace search does.
fn mark_brace_span_scaffold(range: TextRange, source: &str, idx: &LineIndex, out: &mut Vec<bool>) {
    let start_byte = usize::from(range.start());
    let end_byte = usize::from(range.end()).min(source.len());
    if start_byte > end_byte {
        return;
    }

    let mut header_start = start_byte;
    let before = &source.as_bytes()[..header_start];
    let mut j = before.len();
    while j > 0 && (before[j - 1] == b' ' || before[j - 1] == b'\t' || before[j - 1] == b'\n') {
        j -= 1;
    }
    if j > 0 && before[j - 1] == b'{' {
        header_start = j - 1;
    }

    let mut footer_end = end_byte;
    let after = &source.as_bytes()[end_byte..];
    let mut i = 0;
    while i < after.len() && (after[i] == b' ' || after[i] == b'\t' || after[i] == b'\n') {
        i += 1;
    }
    if i < after.len() && after[i] == b'}' {
        footer_end = end_byte + i + 1;
    }

    let (header_line, _) = idx.line_col(rowan::TextSize::from(
        u32::try_from(header_start).unwrap_or(u32::MAX),
    ));
    let (footer_line, _) = idx.line_col(rowan::TextSize::from(
        u32::try_from(footer_end.saturating_sub(1)).unwrap_or(u32::MAX),
    ));

    mark_line(header_line, out);
    mark_line(footer_line, out);
}

fn mark_line(line: u32, out: &mut Vec<bool>) {
    let line = line as usize;
    if line >= out.len() {
        out.resize(line + 1, false);
    }
    out[line] = true;
}

/// Compute the machinery and narrative fold runs (#365): maximal runs of
/// `>= 2` consecutive same-nature lines. `ctx` should be produced by
/// [`crate::line_context::line_contexts`] or
/// [`crate::line_context::line_contexts_with_dialect`] for the same `hir`/
/// `source`, so dialect-classified lines (when a dialect is registered)
/// carry their declared `nature` into the run computation.
#[must_use]
pub fn machinery_and_narrative_folds(
    hir: &HirFile,
    source: &str,
    ctx: &[LineContext],
) -> Vec<FoldRange> {
    let idx = LineIndex::new(source);
    let mut scaffold = vec![false; ctx.len()];
    mark_conditional_scaffold(hir, source, &idx, &mut scaffold);
    let natures = line_natures(source, ctx, &scaffold);

    let mut ranges = Vec::new();
    let mut run_start: Option<(u32, LineNature)> = None;

    let mut flush = |run_start: &mut Option<(u32, LineNature)>, end_line: u32| {
        if let Some((start, nature)) = run_start.take()
            && end_line > start
        {
            let kind = match nature {
                LineNature::Machinery => FoldKind::Machinery,
                LineNature::Narrative => FoldKind::Narrative,
                LineNature::Structural => return,
            };
            ranges.push(FoldRange {
                start_line: start,
                end_line,
                collapsed_text: None,
                from_line_start: false,
                kind,
            });
        }
    };

    for (i, nature) in natures.iter().enumerate() {
        let line_no = u32::try_from(i).unwrap_or(u32::MAX);
        match (*nature, run_start) {
            (LineNature::Structural, _) => {
                flush(&mut run_start, line_no.saturating_sub(1));
                run_start = None;
            }
            (n, Some((_, current))) if n == current => {}
            (n, _) => {
                flush(&mut run_start, line_no.saturating_sub(1));
                run_start = Some((line_no, n));
            }
        }
    }
    if let Some(last) = natures.len().checked_sub(1) {
        flush(&mut run_start, u32::try_from(last).unwrap_or(u32::MAX));
    }

    ranges
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

    #[test]
    fn structural_folds_are_tagged_structural() {
        let src = "== hub ==\ntext\nmore\n";
        let parsed = brink_syntax::parse(src);
        let (hir, _, _) = brink_ir::hir::lower(brink_ir::FileId(0), &parsed.tree());
        let ranges = super::folding_ranges(&hir, src);
        assert!(!ranges.is_empty());
        assert!(
            ranges.iter().all(|r| r.kind == super::FoldKind::Structural),
            "every fold from folding_ranges() is Structural"
        );
    }
}

// ── Machinery/narrative fold-run tests (#365) ──────────────────────────
#[cfg(test)]
mod fold_kind_tests {
    use super::{FoldKind, machinery_and_narrative_folds};
    use crate::line_context::line_contexts;

    fn kinds_for(src: &str) -> Vec<(u32, u32, FoldKind)> {
        let parsed = brink_syntax::parse(src);
        let (hir, _, _) = brink_ir::hir::lower(brink_ir::FileId(0), &parsed.tree());
        let ctx = line_contexts(&hir, src, &parsed.syntax());
        machinery_and_narrative_folds(&hir, src, &ctx)
            .into_iter()
            .map(|r| (r.start_line, r.end_line, r.kind))
            .collect()
    }

    #[test]
    fn two_consecutive_logic_lines_fold_as_machinery() {
        let src = "=== start ===\n~ temp x = 1\n~ temp y = 2\nHello\n";
        let ranges = kinds_for(src);
        assert!(ranges.contains(&(1, 2, FoldKind::Machinery)), "{ranges:?}");
    }

    #[test]
    fn single_logic_line_does_not_fold() {
        let src = "=== start ===\n~ temp x = 1\nHello\nmore text\n";
        let ranges = kinds_for(src);
        assert!(
            !ranges
                .iter()
                .any(|&(s, _, k)| s == 1 && k == FoldKind::Machinery),
            "a lone machinery line must not fold: {ranges:?}"
        );
    }

    #[test]
    fn two_consecutive_narrative_lines_fold_as_narrative() {
        let src = "=== start ===\nHello there friend.\nHow are you today?\n~ temp x = 1\n";
        let ranges = kinds_for(src);
        assert!(ranges.contains(&(1, 2, FoldKind::Narrative)), "{ranges:?}");
    }

    #[test]
    fn var_decl_and_logic_form_one_machinery_run() {
        let src = "VAR x = 1\n~ temp y = 2\n== hub ==\ntext\n";
        let ranges = kinds_for(src);
        assert!(ranges.contains(&(0, 1, FoldKind::Machinery)), "{ranges:?}");
    }

    #[test]
    fn standalone_divert_joins_a_machinery_run() {
        let src = "=== start ===\n~ temp x = 1\n-> other\n=== other ===\ntext\n";
        let ranges = kinds_for(src);
        assert!(ranges.contains(&(1, 2, FoldKind::Machinery)), "{ranges:?}");
    }

    #[test]
    fn lone_standalone_divert_does_not_fold() {
        let src = "=== start ===\nHello.\n-> other\n=== other ===\ntext\n";
        let ranges = kinds_for(src);
        assert!(
            !ranges
                .iter()
                .any(|&(s, _, k)| s == 2 && k == FoldKind::Machinery),
            "{ranges:?}"
        );
    }

    #[test]
    fn tunnel_call_is_not_machinery() {
        // A tunnel call `-> knot ->` is not a "standalone divert" per spec —
        // it must not join a machinery run on its own.
        let src = "=== start ===\n~ temp x = 1\n-> tunnel ->\nHello.\n";
        let ranges = kinds_for(src);
        assert!(
            !ranges
                .iter()
                .any(|&(s, e, k)| s <= 2 && e >= 2 && k == FoldKind::Machinery),
            "tunnel call line must not be swept into a machinery run: {ranges:?}"
        );
    }

    #[test]
    fn thread_start_is_not_machinery() {
        // A thread start `<- knot` must not be treated as a standalone
        // divert either (house rule: threads `<-` are not diverts `->`).
        let src = "=== start ===\n~ temp x = 1\n<- thread_knot\nHello.\n";
        let ranges = kinds_for(src);
        assert!(
            !ranges
                .iter()
                .any(|&(s, e, k)| s <= 2 && e >= 2 && k == FoldKind::Machinery),
            "thread start line must not be swept into a machinery run: {ranges:?}"
        );
    }

    #[test]
    fn pure_routing_conditional_folds_as_machinery() {
        // A conditional whose scaffold + branch bodies are all machinery
        // (assignments only) folds as machinery runs.
        let src = "=== start ===\n{ x > 5:\n~ y = 1\n- else:\n~ y = 2\n}\nHello.\n";
        let ranges = kinds_for(src);
        assert!(
            ranges.iter().any(|&(_, _, k)| k == FoldKind::Machinery),
            "pure-routing conditional must produce at least one machinery run: {ranges:?}"
        );
    }

    #[test]
    fn narrative_bearing_conditional_does_not_fold_as_machinery() {
        // The solstice_busy key case (issue #365): a mostly-narrative
        // multi-line conditional must NOT be treated as machinery, even
        // though its scaffold lines (`{ cond:`, `}`) are machinery-natured.
        // The narrative branch bodies break the run.
        let src = "\
=== start ===
{ busy:
Sorry, I'm quite busy today.
- else:
Come on in, take a seat.
}
";
        let ranges = kinds_for(src);
        assert!(
            !ranges.iter().any(|&(_, _, k)| k == FoldKind::Machinery),
            "a narrative-bearing conditional must not fold as machinery: {ranges:?}"
        );
    }

    #[test]
    fn gather_line_breaks_a_machinery_run() {
        // Gather+divert form (house rule): a real gather line must not be
        // swallowed into an adjacent machinery run.
        let src = "=== start ===\n* [Go]\n~ temp x = 1\n- (g)\n~ temp y = 2\n-> END\n";
        let ranges = kinds_for(src);
        // The gather line itself (a real weave gather, not conditional
        // scaffold) is Structural and must break any run spanning it.
        assert!(
            !ranges
                .iter()
                .any(|&(s, e, k)| k == FoldKind::Machinery && s <= 3 && e >= 3),
            "a real gather line must break the machinery run: {ranges:?}"
        );
    }

    #[test]
    fn blank_line_breaks_a_narrative_run() {
        let src = "=== start ===\nHello there.\n\nGoodbye now.\nSee you soon.\n";
        let ranges = kinds_for(src);
        assert!(
            !ranges
                .iter()
                .any(|&(s, e, k)| k == FoldKind::Narrative && s <= 1 && e >= 3),
            "blank line must break the narrative run: {ranges:?}"
        );
        assert!(ranges.contains(&(3, 4, FoldKind::Narrative)), "{ranges:?}");
    }

    #[test]
    fn choice_with_inline_alternative_is_not_machinery() {
        // A choice whose bracket text holds an inline alternative
        // (`* [Take the {red|blue} pill]`) must NOT be scaffold-marked: the
        // inline sequence sits in the choice's *inline text*, which folding
        // never treats as conditional/sequence scaffold. Two such choices must
        // therefore not collapse into a machinery run. Guards the
        // ContentContext gate in ScaffoldMarker (the shared visitor descends
        // choice inline content, which the old hand-rolled walk did not).
        let src = "\
=== start ===
* [Take the {red|blue} pill]
* [Take the {big|small} dose]
-> END
";
        let ranges = kinds_for(src);
        assert!(
            !ranges.iter().any(|&(_, _, k)| k == FoldKind::Machinery),
            "choice lines with inline alternatives must not fold as machinery: {ranges:?}"
        );
    }

    #[test]
    fn dialect_classified_cue_and_dialogue_form_a_narrative_run() {
        // Dialect nature is authoritative: a character cue + chained
        // dialogue line are both Narrative-natured (at-cue preset) and fold
        // as one narrative run, exactly like plain prose would.
        let src = "=== start ===\n@Alice:<>\nHello there.\n";
        let parsed = brink_syntax::parse(src);
        let (hir, _, _) = brink_ir::hir::lower(brink_ir::FileId(0), &parsed.tree());
        let dialect = brink_ir::ResolvedDialect::compile(&brink_ir::DialogueDialect::default())
            .expect("at-cue preset compiles");
        let ctx =
            crate::line_context::line_contexts_with_dialect(&hir, src, &parsed.syntax(), &dialect);
        let ranges: Vec<(u32, u32, FoldKind)> = machinery_and_narrative_folds(&hir, src, &ctx)
            .into_iter()
            .map(|r| (r.start_line, r.end_line, r.kind))
            .collect();
        assert!(ranges.contains(&(1, 2, FoldKind::Narrative)), "{ranges:?}");
    }
}
