use brink_ir::HirFile;
use rowan::TextRange;

use crate::LineIndex;
use crate::hir_projection::{Projection, SpanKind};
use crate::line_context::LineContext;

/// The fold's kind (#365 — Celeris §5.5): every fold the editor exposes is
/// tagged so hosts can select which kinds auto-collapse per view mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldKind {
    /// Structure-anchored folds: decls, doc comments, INCLUDE blocks,
    /// conditionals/sequences, and (since #476) choice branches and gather
    /// continuations from the projection's container extents. User-invoked
    /// in every mode; NEVER auto-collapsed by a host's mode entry.
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
#[derive(Debug)]
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

/// Compute folding ranges for a file from its HIR and projection (#476).
///
/// `projection` supplies the weave and construct folds — Choice/Gather
/// container extents and Conditional/Sequence construct extents
/// ([`crate::hir_projection::project_hir_structural`] suffices; identity is
/// not used). The HIR drives what the projection doesn't model as containers:
/// the INCLUDE block and knot/stitch declaration folds (doc-block handling).
///
/// All ranges from this pass are [`FoldKind::Structural`] — the
/// machinery/narrative run-based folds are computed separately by
/// [`machinery_and_narrative_folds`], since they require the per-line
/// `nature` facet (base classification, or a registered dialect's) rather
/// than HIR structure.
pub fn folding_ranges(hir: &HirFile, source: &str, projection: &Projection) -> Vec<FoldRange> {
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

    // Leading IMPORT block (M-4, modules-spec §9): collapse a run of
    // two-or-more leading module `IMPORT`s into one region, mirroring the
    // INCLUDE block fold. The span is derived from the shared
    // `import_block_span` detector so fold and auto-import agree on its
    // bounds. A single IMPORT is detected by the shared helper but is not
    // worth folding.
    if let Some(span) = crate::import_block::import_block_span(hir, source)
        && span.count >= 2
    {
        ranges.push(FoldRange {
            start_line: span.start_line,
            end_line: span.end_line,
            collapsed_text: Some(format!("IMPORT … ({} modules)", span.count)),
            from_line_start: false,
            kind: FoldKind::Structural,
        });
    }

    // Weave + construct folds from the projection (#476). A Choice
    // container's extent is the full branch (choice line ∪ body, §5.1): the
    // fold anchors at the end of the choice line and hides the branch. A
    // Gather container folds its continuation from the gather line — but
    // only when the extent actually anchors on a gather line: an unlabeled
    // gather whose own line is prose has ptr-less line content, so its
    // extent starts at the first *located* statement, which can be a
    // different construct entirely (even a sibling choice line, where the
    // stray fold would shadow the choice's own). Until lowering stamps ptrs
    // on accumulated content, such gathers get no fold rather than a wrong
    // one. Conditional/Sequence construct spans reproduce the pre-#476
    // conditional folds exactly: same ptr ranges, same Body gating, and the
    // "{...}" sentinel drives `push_fold`'s brace extension. Single-line
    // extents fold nothing (bodiless choices, bare labeled gathers).
    for span in &projection.spans {
        match span.kind {
            SpanKind::Choice if span.handle.is_some() => {
                push_fold(span.range, None, source, &idx, &mut ranges);
            }
            SpanKind::Gather
                if span.handle.is_some() && anchors_on_gather_line(span.range, source, &idx) =>
            {
                push_fold(span.range, None, source, &idx, &mut ranges);
            }
            SpanKind::Conditional | SpanKind::Sequence => {
                push_fold(
                    span.range,
                    Some("{...}".to_owned()),
                    source,
                    &idx,
                    &mut ranges,
                );
            }
            _ => {}
        }
    }

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
        }
    }

    collect_doc_comment_folds(source, &consumed_doc_lines, &mut ranges);

    ranges
}

/// Folding ranges for `~ { … }` multi-line logic blocks and their nested
/// control bodies — `if`/`else if`/`else`, `while`, `for`
/// (docs/t1b-surface-spec.md §2, brink extension; #589).
///
/// A separate pass from [`folding_ranges`]: the HIR projection
/// ([`crate::hir_projection`]) doesn't model `LogicBlock`/`BlockStmt` at all
/// (T1b-2 added them to the HIR after the projection's phase-1 coverage was
/// fixed — #454/#476 predate T1b), so this walks the HIR directly via
/// [`brink_ir::hir::visit`] instead of going through `projection.spans`,
/// mirroring [`machinery_and_narrative_folds`]'s separate-pass shape.
///
/// No dialect gate: `brink-syntax` always parses the superset grammar and
/// `brink-ir` always lowers it to this same HIR shape regardless of dialect
/// (docs/t1b-surface-spec.md §1) — the dialect gate only rejects the
/// construct at *analysis*, never at parse/HIR — so a `~ { … }` block folds
/// identically in a strict-ink file (where it's flagged `E051`) as in a
/// brink one.
///
/// Every nested `if`/`while`/`for` folds as its own region (from its own
/// HIR `ptr` extent, which already spans the construct's own braces —
/// `while cond { … }`/`for x in y { … }`/`if cond { … } else { … }` — so no
/// brace-extension is needed, unlike the Conditional/Sequence `{...}` folds
/// in `folding_ranges`), so an editor can collapse an inner `if` without
/// collapsing its enclosing block.
#[must_use]
pub fn block_folds(hir: &HirFile, source: &str) -> Vec<FoldRange> {
    let idx = LineIndex::new(source);
    let mut ranges = Vec::new();
    let mut collector = BlockFoldCollector {
        source,
        idx: &idx,
        ranges: &mut ranges,
    };
    brink_ir::hir::visit::visit(hir, &mut collector);
    ranges
}

struct BlockFoldCollector<'a> {
    source: &'a str,
    idx: &'a LineIndex,
    ranges: &'a mut Vec<FoldRange>,
}

impl brink_ir::hir::HirVisitor for BlockFoldCollector<'_> {
    fn enter_stmt(&mut self, stmt: &brink_ir::hir::Stmt) {
        if let brink_ir::hir::Stmt::LogicBlock(lb) = stmt {
            push_fold(
                lb.ptr.text_range(),
                None,
                self.source,
                self.idx,
                self.ranges,
            );
            for bs in &lb.stmts {
                collect_block_stmt_folds(bs, self.source, self.idx, self.ranges);
            }
        }
    }
}

/// Fold a nested control-flow `BlockStmt` (`if`/`while`/`for`) and recurse
/// into its body for further nesting. Non-control statements (assignments,
/// temp decls, returns, breaks/continues, expression statements) have
/// nothing to fold — they're always single-line.
fn collect_block_stmt_folds(
    bs: &brink_ir::hir::BlockStmt,
    source: &str,
    idx: &LineIndex,
    out: &mut Vec<FoldRange>,
) {
    match bs {
        brink_ir::hir::BlockStmt::If(if_stmt) => collect_if_folds(if_stmt, source, idx, out),
        brink_ir::hir::BlockStmt::While(w) => {
            push_fold(w.ptr.text_range(), None, source, idx, out);
            for inner in &w.body {
                collect_block_stmt_folds(inner, source, idx, out);
            }
        }
        brink_ir::hir::BlockStmt::For(f) => {
            push_fold(f.ptr.text_range(), None, source, idx, out);
            for inner in &f.body {
                collect_block_stmt_folds(inner, source, idx, out);
            }
        }
        // `await <cond>` is a single-line suspension point — no fold region.
        brink_ir::hir::BlockStmt::TempDecl(_)
        | brink_ir::hir::BlockStmt::Assignment(_)
        | brink_ir::hir::BlockStmt::Return(_)
        | brink_ir::hir::BlockStmt::Break(_)
        | brink_ir::hir::BlockStmt::Continue(_)
        | brink_ir::hir::BlockStmt::ExprStmt(_)
        | brink_ir::hir::BlockStmt::Await(_) => {}
    }
}

/// Fold one `if`/`else if`/`else` chain: the outer `if`'s own extent folds
/// as one region (condition through the chain's final `}`, matching how a
/// choice branch or gather continuation folds from its own anchor line —
/// #476's precedent), and each nested `else if` additionally folds as its
/// own (shorter) region, so collapsing an inner clause doesn't require
/// collapsing the whole chain. Recurses into every body (including a plain
/// `else { … }`'s statements) for further nested control bodies.
fn collect_if_folds(
    if_stmt: &brink_ir::hir::IfStmt,
    source: &str,
    idx: &LineIndex,
    out: &mut Vec<FoldRange>,
) {
    push_fold(if_stmt.ptr.text_range(), None, source, idx, out);
    for inner in &if_stmt.body {
        collect_block_stmt_folds(inner, source, idx, out);
    }
    match &if_stmt.else_branch {
        Some(brink_ir::hir::ElseBranch::ElseIf(inner)) => {
            collect_if_folds(inner, source, idx, out);
        }
        Some(brink_ir::hir::ElseBranch::Else(stmts)) => {
            for inner in stmts {
                collect_block_stmt_folds(inner, source, idx, out);
            }
        }
        None => {}
    }
}

/// Whether a Gather container's extent starts on an actual gather line
/// (trimmed text starting with `-`, not `->`) — the guard that keeps a
/// mis-anchored extent (ptr-less gather-line prose, see the caller's
/// comment) from emitting a fold on some other construct's line.
fn anchors_on_gather_line(range: TextRange, source: &str, idx: &LineIndex) -> bool {
    let (line, _) = idx.line_col(range.start());
    let Some(text) = source.split('\n').nth(line as usize) else {
        return false;
    };
    let trimmed = text.trim_start();
    trimmed.starts_with('-') && !trimmed.starts_with("->")
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
                    // Standalone-ness is a structural fact on the context
                    // (#480) — tunnels/threads are Structural, plain and
                    // terminal diverts are Machinery.
                    if c.standalone {
                        LineNature::Machinery
                    } else {
                        LineNature::Structural
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
/// located from the projection's construct-extent spans
/// ([`SpanKind::Conditional`]/[`SpanKind::Sequence`] — already Body-gated,
/// so inline logic in a choice's own text is never scaffold) + the same
/// brace-extension the `{...}` fold uses (`push_fold`'s brace search), so
/// the two never disagree about where a conditional's braces sit. Only
/// these two lines per construct are scaffold — intermediate branch
/// separators (`- else:`) are left at their existing (structural)
/// classification. The HIR now carries a real per-branch source span
/// (`CondBranch`/`SequenceBranch::ptr`, issue #404), so the data this
/// function would need to anchor separator lines individually exists; the
/// remaining blocker is discriminating a bare separator line (safe to
/// force into machinery scaffold) from ink's same-line-body form
/// (`- 1: Foo.`, or the one-line-per-alternative sequence idiom), which a
/// naive "mark every branch's separator line as scaffold" would
/// misclassify as machinery. See issue #404's follow-up comment.
fn mark_conditional_scaffold(
    projection: &Projection,
    source: &str,
    idx: &LineIndex,
    out: &mut Vec<bool>,
) {
    for span in &projection.spans {
        if matches!(span.kind, SpanKind::Conditional | SpanKind::Sequence) {
            mark_brace_span_scaffold(span.range, source, idx, out);
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
/// `>= 2` consecutive same-nature lines. `projection` is the file's HIR
/// projection ([`crate::hir_projection::project_hir_structural`] suffices —
/// no identity needed) and `ctx` should be produced by
/// [`crate::line_context::line_contexts`] or
/// [`crate::line_context::line_contexts_with_dialect`] for the same file,
/// so dialect-classified lines (when a dialect is registered) carry their
/// declared `nature` into the run computation.
#[must_use]
pub fn machinery_and_narrative_folds(
    projection: &Projection,
    source: &str,
    ctx: &[LineContext],
) -> Vec<FoldRange> {
    let idx = LineIndex::new(source);
    let mut scaffold = vec![false; ctx.len()];
    mark_conditional_scaffold(projection, source, &idx, &mut scaffold);
    let natures = line_natures(source, ctx, &scaffold);

    // A run is additionally bounded by weave containers (#479): a fold never
    // crosses into or out of a choice branch or gather continuation, keeping
    // run folds aligned with the rails. Only Choice/Gather bound —
    // conditional/sequence branch containers deliberately do NOT: a
    // pure-routing conditional's scaffold + arms must fold as one machinery
    // region (the #365 key case), and an inline construct's single-line
    // container must not fragment the narrative run hosting it. In practice
    // weave transition lines are Structural (choice/gather lines) and break
    // runs anyway; the bound is the safety net that keeps that true by
    // construction rather than by coincidence.
    let weave_bounds: Vec<Option<u32>> = projection
        .lines
        .iter()
        .map(|stack| {
            stack
                .containers
                .iter()
                .rev()
                .find(|c| matches!(c.kind, SpanKind::Choice | SpanKind::Gather))
                .map(|c| c.handle)
        })
        .collect();
    let bound_at = |i: usize| weave_bounds.get(i).copied().flatten();

    // Choice container handle -> the line its own choice text starts on
    // (#417). A Choice container's extent is choice-line ∪ body (§5.1); the
    // body's first line is always `choice_start + 1`. Used below to detect
    // when a narrative run *is* a choice's body (run start == body start),
    // so the fold can be re-anchored on the choice line itself rather than
    // starting inside the body.
    let mut choice_start_lines: std::collections::HashMap<u32, u32> =
        std::collections::HashMap::new();
    for span in &projection.spans {
        if span.kind == SpanKind::Choice
            && let Some(handle) = span.handle
        {
            let (line, _) = idx.line_col(span.range.start());
            choice_start_lines.entry(handle).or_insert(line);
        }
    }

    let mut ranges = Vec::new();
    // (run's first content line, nature, weave bound, choice line to extend
    // the fold's anchor to — #417 point 1).
    let mut run_start: Option<(u32, LineNature, Option<u32>, Option<u32>)> = None;

    let mut flush = |run_start: &mut Option<(u32, LineNature, Option<u32>, Option<u32>)>,
                     end_line: u32| {
        if let Some((start, nature, _, extend_to)) = run_start.take()
            && end_line > start
        {
            let kind = match nature {
                LineNature::Machinery => FoldKind::Machinery,
                LineNature::Narrative => FoldKind::Narrative,
                LineNature::Structural => return,
            };
            // Narrative folds anchor on the whole line (#417 point 3):
            // the anchor's visible content must not double with the
            // pill, so the fold hides from the line's start and the
            // pill IS the line — same shape as the decl-fold
            // placeholder. When the run is exactly a choice's body
            // (point 1), that anchor line is the choice line itself
            // rather than the run's own first line.
            let (start_line, from_line_start) = match kind {
                FoldKind::Narrative => (extend_to.unwrap_or(start), true),
                _ => (start, false),
            };
            ranges.push(FoldRange {
                start_line,
                end_line,
                collapsed_text: None,
                from_line_start,
                kind,
            });
        }
    };

    for (i, nature) in natures.iter().enumerate() {
        let line_no = u32::try_from(i).unwrap_or(u32::MAX);
        let bound = bound_at(i);
        match (*nature, run_start) {
            (LineNature::Structural, _) => {
                flush(&mut run_start, line_no.saturating_sub(1));
                run_start = None;
            }
            (n, Some((_, current, b, _))) if n == current && bound == b => {}
            (n, _) => {
                flush(&mut run_start, line_no.saturating_sub(1));
                let extend_to = if n == LineNature::Narrative {
                    bound
                        .and_then(|h| choice_start_lines.get(&h).copied())
                        .filter(|&choice_line| choice_line + 1 == line_no)
                } else {
                    None
                };
                run_start = Some((line_no, n, bound, extend_to));
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
        let projection = crate::hir_projection::project_hir_structural(&hir, src);
        folding_ranges(&hir, src, &projection)
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
        let projection = crate::hir_projection::project_hir_structural(&hir, src);
        super::folding_ranges(&hir, src, &projection)
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
    fn import_block_folds_when_two_or_more() {
        let src = "IMPORT { ambush } FROM quest_3\nIMPORT quest_4\n== hub ==\ntext\n";
        let folds = folds_with_text(src);
        assert!(
            folds.contains(&(0, 1, Some("IMPORT … (2 modules)".to_owned()))),
            "{folds:?}"
        );
    }

    #[test]
    fn single_import_does_not_fold() {
        let src = "IMPORT quest_3\n== hub ==\ntext\n";
        let folds = folds_with_text(src);
        assert!(
            !folds.iter().any(
                |(s, _, t)| *s == 0 && t.as_deref().is_some_and(|t| t.starts_with("IMPORT …"))
            ),
            "a single IMPORT must not fold: {folds:?}"
        );
    }

    #[test]
    fn include_and_import_blocks_fold_independently() {
        let src = "\
INCLUDE a.ink
INCLUDE b.ink
IMPORT quest_3
IMPORT quest_4
== hub ==
";
        let folds = folds_with_text(src);
        assert!(
            folds.contains(&(0, 1, Some("INCLUDE … (2 files)".to_owned()))),
            "include fold present: {folds:?}"
        );
        assert!(
            folds.contains(&(2, 3, Some("IMPORT … (2 modules)".to_owned()))),
            "import fold present: {folds:?}"
        );
    }

    #[test]
    fn structural_folds_are_tagged_structural() {
        let src = "== hub ==\ntext\nmore\n";
        let parsed = brink_syntax::parse(src);
        let (hir, _, _) = brink_ir::hir::lower(brink_ir::FileId(0), &parsed.tree());
        let projection = crate::hir_projection::project_hir_structural(&hir, src);
        let ranges = super::folding_ranges(&hir, src, &projection);
        assert!(!ranges.is_empty());
        assert!(
            ranges.iter().all(|r| r.kind == super::FoldKind::Structural),
            "every fold from folding_ranges() is Structural"
        );
    }

    // ── Weave folds (#476) ──────────────────────────────────────────

    #[test]
    fn choice_branches_fold_from_their_choice_line() {
        let src = "\
=== start ===
* Take the sword
  The blade hums.
  More body text.
  -> armory
* Leave it
  You walk away.
- done either way
-> END
=== armory ===
-> END
";
        let ranges = ranges_for(src);
        // Each choice folds from its own line to the end of its branch,
        // keeping the choice line visible (from_line_start = false).
        assert!(
            ranges.contains(&(1, 4, false)),
            "first choice folds its branch: {ranges:?}"
        );
        assert!(
            ranges.contains(&(5, 6, false)),
            "second choice folds its branch: {ranges:?}"
        );
    }

    #[test]
    fn labeled_gather_folds_its_continuation() {
        let src = "\
=== start ===
* Choice
  Body.
- (done) either way
Continuation prose.
-> END
";
        let ranges = ranges_for(src);
        assert!(
            ranges.contains(&(3, 5, false)),
            "labeled gather folds its continuation from the gather line: {ranges:?}"
        );
    }

    #[test]
    fn unlabeled_prose_gather_fold_anchors_late_known_limitation() {
        // An unlabeled gather whose own line is prose has ptr-less line
        // content, so its container extent — and therefore the fold — starts
        // at the first *located* statement, not the gather line. Pinned as a
        // known limitation (#476); the fix is upstream in lowering (stamping
        // ptrs on accumulated content), not in folding.
        let src = "\
=== start ===
* Choice
  Body.
- done either way
Continuation prose.
-> END
";
        let ranges = ranges_for(src);
        assert!(
            !ranges.iter().any(|&(s, _, _)| s == 3),
            "no fold anchors on the unlabeled prose gather line (yet): {ranges:?}"
        );
    }

    #[test]
    fn nested_choices_fold_nested() {
        let src = "\
=== start ===
* Outer
  * * Inner
      Inner body.
  * * Other inner
- done
";
        let ranges = ranges_for(src);
        assert!(
            ranges.contains(&(1, 4, false)),
            "outer choice folds through its nested weave: {ranges:?}"
        );
        assert!(
            ranges.contains(&(2, 3, false)),
            "inner choice folds its own body: {ranges:?}"
        );
    }

    #[test]
    fn bodiless_choice_does_not_fold() {
        let src = "=== start ===\n* [Go] -> hub\n* [Stay]\n=== hub ===\n-> END\n";
        let ranges = ranges_for(src);
        assert!(
            !ranges.iter().any(|&(s, _, _)| s == 1 || s == 2),
            "single-line choices must not fold: {ranges:?}"
        );
    }

    #[test]
    fn bare_labeled_gather_does_not_fold() {
        let src = "=== start ===\n* Choice\n  Body.\n- (g)\n";
        let ranges = ranges_for(src);
        assert!(
            !ranges.iter().any(|&(s, _, _)| s == 3),
            "a bare labeled gather is single-line — nothing to fold: {ranges:?}"
        );
    }
}

// ── `~ { … }` block + nested control-body fold tests (#589) ────────────
#[cfg(test)]
mod block_fold_tests {
    use super::block_folds;

    /// `(start_line, end_line)` pairs for `src`.
    fn folds_for(src: &str) -> Vec<(u32, u32)> {
        let parsed = brink_syntax::parse(src);
        let (hir, _, _) = brink_ir::hir::lower(brink_ir::FileId(0), &parsed.tree());
        block_folds(&hir, src)
            .iter()
            .map(|r| (r.start_line, r.end_line))
            .collect()
    }

    #[test]
    fn multiline_block_folds_from_its_opening_brace_line() {
        let src = "=== start ===\n~ {\n    temp x = 1\n    x = x + 1\n}\n-> END\n";
        let ranges = folds_for(src);
        // Line 1 is `~ {`, line 4 is the closing `}`.
        assert!(ranges.contains(&(1, 4)), "{ranges:?}");
    }

    #[test]
    fn single_line_block_does_not_fold() {
        let src = "=== start ===\n~ { temp x = 1 }\n-> END\n";
        let ranges = folds_for(src);
        assert!(ranges.is_empty(), "nothing to fold on one line: {ranges:?}");
    }

    #[test]
    fn nested_for_loop_folds_separately_from_the_enclosing_block() {
        let src = "\
=== start ===
~ {
    temp total = 0
    for item in #[1, 2, 3] {
        total = total + item
    }
    if total > 3 {
        score = total
    }
}
-> END
";
        let ranges = folds_for(src);
        // `~ {` is line 1, block closes on line 9.
        assert!(ranges.contains(&(1, 9)), "outer block: {ranges:?}");
        // `for item in #[1, 2, 3] {` is line 3, its own `}` is line 5.
        assert!(ranges.contains(&(3, 5)), "for loop body: {ranges:?}");
        // `if total > 3 {` is line 6, its own `}` is line 8.
        assert!(ranges.contains(&(6, 8)), "if body: {ranges:?}");
    }

    #[test]
    fn nested_while_loop_folds_as_its_own_region() {
        let src = "\
=== start ===
~ {
    temp n = 0
    while n < 3 {
        n = n + 1
    }
}
-> END
";
        let ranges = folds_for(src);
        assert!(ranges.contains(&(3, 5)), "while body: {ranges:?}");
    }

    #[test]
    fn else_if_chain_folds_each_clause_separately() {
        let src = "\
=== start ===
~ {
    if a > 3 {
        x = 1
    } else if a > 1 {
        x = 2
    } else {
        x = 3
    }
}
-> END
";
        let ranges = folds_for(src);
        // The outer `if` folds the whole chain (its own line through the
        // final `}`, line 8).
        assert!(ranges.contains(&(2, 8)), "whole if/else chain: {ranges:?}");
        // The `else if` clause folds its own (shorter) region too.
        assert!(ranges.contains(&(4, 8)), "else-if clause: {ranges:?}");
    }

    #[test]
    fn block_inside_a_choice_body_still_folds() {
        // The visitor reaches a `~ { … }` wherever it sits in the tree, not
        // just top-level knot bodies.
        let src = "\
=== start ===
* Take it
  ~ {
      temp x = 1
      x = x + 1
  }
- done
";
        let ranges = folds_for(src);
        assert!(
            !ranges.is_empty(),
            "block inside a choice body folds too: {ranges:?}"
        );
    }
}

// ── Machinery/narrative fold-run tests (#365) ──────────────────────────
#[cfg(test)]
mod fold_kind_tests {
    use super::{FoldKind, FoldRange, machinery_and_narrative_folds};
    use crate::line_context::line_contexts;

    fn kinds_for(src: &str) -> Vec<(u32, u32, FoldKind)> {
        ranges_full_for(src)
            .into_iter()
            .map(|r| (r.start_line, r.end_line, r.kind))
            .collect()
    }

    fn ranges_full_for(src: &str) -> Vec<FoldRange> {
        let parsed = brink_syntax::parse(src);
        let (hir, _, _) = brink_ir::hir::lower(brink_ir::FileId(0), &parsed.tree());
        let projection = crate::hir_projection::project_hir_structural(&hir, src);
        let ctx = line_contexts(src, &parsed.syntax(), &projection);
        machinery_and_narrative_folds(&projection, src, &ctx)
    }

    /// Fold ranges for `src` with the at-cue dialect resolved and applied —
    /// shared by the #417 choice-body-anchor tests, which need cues/dialogue
    /// to classify as `Narrative` inside a choice body.
    fn dialect_ranges_for(src: &str) -> Vec<FoldRange> {
        let parsed = brink_syntax::parse(src);
        let (hir, _, _) = brink_ir::hir::lower(brink_ir::FileId(0), &parsed.tree());
        let dialect = brink_ir::ResolvedDialect::compile(&brink_ir::DialogueDialect::default())
            .expect("at-cue preset compiles");
        let projection = crate::hir_projection::project_hir_structural(&hir, src);
        let ctx = crate::line_context::line_contexts_with_dialect(
            src,
            &parsed.syntax(),
            &projection,
            &dialect,
        );
        machinery_and_narrative_folds(&projection, src, &ctx)
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
    fn choice_with_nested_inline_logic_is_not_machinery() {
        // Transitive ContentContext: an inline sequence nested *inside* an
        // inline conditional that is itself in a choice's bracket text
        // (`* [take {a: {b|c}}]`) must still be treated as choice inline text,
        // not scaffold — the gate is transitive, not just immediate. Two such
        // choices must not collapse into a machinery run. Guards Findings 1/2
        // from the adversarial review of the visitor migration.
        let src = "\
=== start ===
* [take {a: {b|c}}]
* [drop {d: {e|f}}]
-> END
";
        let ranges = kinds_for(src);
        assert!(
            !ranges.iter().any(|&(_, _, k)| k == FoldKind::Machinery),
            "nested inline logic in choice text must not fold as machinery: {ranges:?}"
        );
    }

    // ── #479 weave bounding: what it must NOT change ────────────────

    #[test]
    fn pure_routing_conditional_still_folds_across_branches() {
        // The weave bound (#479) covers Choice/Gather containers only —
        // conditional branch containers must NOT bound runs, or the #365
        // key case (a pure-routing block folding as one machinery region,
        // scaffold + arms together) regresses.
        let src = "=== start ===\n{ x > 5:\n~ y = 1\n- else:\n~ y = 2\n}\nHello.\n";
        let ranges = kinds_for(src);
        assert!(
            ranges.contains(&(1, 5, FoldKind::Machinery)),
            "scaffold + arms fold as one machinery run: {ranges:?}"
        );
    }

    #[test]
    fn inline_alternative_does_not_fragment_a_narrative_run() {
        // An inline `{a|b}` puts a single-line SequenceBranch container on
        // its host line — the weave bound must ignore it (Choice/Gather
        // only), or every inline alternative would split the narrative run
        // hosting it.
        let src = "=== start ===\nFirst prose line.\nHas {red|blue} inline.\nLast prose line.\n";
        let ranges = kinds_for(src);
        assert!(
            ranges.contains(&(1, 3, FoldKind::Narrative)),
            "one unbroken narrative run across the inline alternative: {ranges:?}"
        );
    }

    #[test]
    fn machinery_run_does_not_cross_out_of_a_choice_body() {
        // The weave bound proper: machinery inside a choice body and
        // machinery in the gather continuation never join, even when the
        // line between them is somehow not a break. (Today the gather line
        // itself is Structural and already breaks the run — this pins the
        // bound so that stays true by construction.)
        let src = "=== start ===\n* [Go]\n  ~ temp a = 1\n  ~ temp b = 2\n- (g)\n~ temp c = 3\n~ temp d = 4\n-> END\n";
        let ranges = kinds_for(src);
        assert!(
            ranges.contains(&(2, 3, FoldKind::Machinery)),
            "body machinery folds within the body: {ranges:?}"
        );
        assert!(
            !ranges
                .iter()
                .any(|&(s, e, k)| k == FoldKind::Machinery && s <= 3 && e >= 5),
            "no run spans from the body into the continuation: {ranges:?}"
        );
    }

    #[test]
    fn dialect_classified_cue_and_dialogue_form_a_narrative_run() {
        // Dialect nature is authoritative: a character cue + chained
        // dialogue line are both Narrative-natured (at-cue preset) and fold
        // as one narrative run, exactly like plain prose would.
        let src = "=== start ===\n@Alice:<>\nHello there.\n";
        let ranges: Vec<(u32, u32, FoldKind)> = dialect_ranges_for(src)
            .into_iter()
            .map(|r| (r.start_line, r.end_line, r.kind))
            .collect();
        assert!(ranges.contains(&(1, 2, FoldKind::Narrative)), "{ranges:?}");
    }

    // ── #417: choice-body anchor + whole-line narrative pill ────────────

    #[test]
    fn narrative_run_folds_from_the_anchor_lines_start() {
        // Point 3: a narrative fold must hide its own anchor line (the
        // run's first line), not leave it visible ahead of the pill — the
        // pill IS the line, mirroring the decl-fold placeholder shape.
        let src = "=== start ===\nHello there friend.\nHow are you today?\n~ temp x = 1\n";
        let ranges = ranges_full_for(src);
        let narrative = ranges
            .iter()
            .find(|r| r.kind == FoldKind::Narrative)
            .expect("narrative run present");
        assert_eq!((narrative.start_line, narrative.end_line), (1, 2));
        assert!(
            narrative.from_line_start,
            "narrative fold must hide the whole anchor line: {ranges:?}"
        );
    }

    #[test]
    fn narrative_run_that_is_a_choice_body_anchors_on_the_choice_line() {
        // Point 1: when a narrative run IS a choice's body (run start ==
        // body start), the fold anchors on the CHOICE line, hiding the
        // whole body beneath it — not starting inside the body on the cue
        // line (the jackie_call fixture from #413/#417).
        let src = "=== start ===\n* [Talk]\n  @Jackie:<>\n  Hello there.\n- (g)\n-> END\n";
        let ranges = dialect_ranges_for(src);
        let narrative = ranges
            .iter()
            .find(|r| r.kind == FoldKind::Narrative)
            .expect("narrative run present");
        assert_eq!(
            narrative.start_line, 1,
            "anchors on the choice line (`* [Talk]`), not the cue line: {ranges:?}"
        );
        assert_eq!(narrative.end_line, 3);
        assert!(narrative.from_line_start, "{ranges:?}");
    }

    #[test]
    fn narrative_run_not_at_the_start_of_a_choice_body_is_not_extended() {
        // A narrative run that does NOT start at the body's first line
        // (some machinery precedes it) must not be re-anchored on the
        // choice line — only an exact run-start == body-start match
        // extends per point 1. It still folds from its own line's start
        // per point 3.
        let src = "=== start ===\n* [Talk]\n  ~ temp a = 1\n  @Jackie:<>\n  Hello there.\n- (g)\n-> END\n";
        let ranges = dialect_ranges_for(src);
        let narrative = ranges
            .iter()
            .find(|r| r.kind == FoldKind::Narrative)
            .expect("narrative run present");
        assert_eq!(
            narrative.start_line, 3,
            "run starts on the cue line, not extended to the choice line: {ranges:?}"
        );
        assert_eq!(narrative.end_line, 4);
        assert!(narrative.from_line_start, "{ranges:?}");
    }
}
