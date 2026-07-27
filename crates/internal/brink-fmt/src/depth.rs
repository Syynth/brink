//! Depth/indent computation: build a map from source line number to
//! indentation depth by walking the lowered HIR tree.
//!
//! This stage runs first in the [`crate::format`] pipeline — its output
//! (`depth_map`) is consumed by line classification, which folds the depth
//! into each [`crate::classify::ClassifiedLine`].

// ── Line starts helper ──────────────────────────────────────────────

pub(crate) fn build_line_starts(source: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(source.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}

/// Find the line number for a byte offset.
pub(crate) fn line_for_offset(line_starts: &[usize], offset: usize) -> usize {
    match line_starts.binary_search(&offset) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    }
}

// ── HIR-based depth map ─────────────────────────────────────────────

/// Build a map from line number → indentation depth by walking the HIR tree.
pub(crate) fn build_depth_map(
    source: &str,
    line_starts: &[usize],
    hir_file: &brink_ir::HirFile,
) -> Vec<u32> {
    let line_count = line_starts.len();
    let mut depth_map = vec![0u32; line_count];

    // Root content (before first knot) — depth 0.
    walk_block_for_depth(&hir_file.root_content, 0, line_starts, &mut depth_map);

    // Knots.
    for knot in &hir_file.knots {
        // Knot header is at depth 0 (handled by classifier).
        // Knot body content is at depth 1.
        walk_block_for_depth(&knot.body, 1, line_starts, &mut depth_map);

        // Stitches.
        for stitch in &knot.stitches {
            // Stitch header at depth 1 (inside knot).
            set_depth_for_range(stitch.ptr.text_range(), 1, line_starts, &mut depth_map);
            // Stitch body content is at depth 2.
            walk_block_for_depth(&stitch.body, 2, line_starts, &mut depth_map);
        }
    }

    // Declarations are always depth 0 — already initialized.
    // Comments inherit the depth of their surrounding context; for now we'll
    // let lines that aren't touched by HIR keep their existing depth (0) and
    // let the classifier handle comment lines using the depth_map context.

    // Propagate depth to lines between HIR-annotated lines: if a line hasn't
    // been set (still 0) but is between two lines with the same depth, inherit.
    // This handles blank lines and comment lines inside knot bodies.
    propagate_depth(source, line_starts, &mut depth_map);

    depth_map
}

/// Walk a Block recursively, setting depth for lines that correspond to
/// source spans in the HIR.
fn walk_block_for_depth(
    block: &brink_ir::Block,
    depth: u32,
    line_starts: &[usize],
    depth_map: &mut [u32],
) {
    walk_block_for_depth_ctx(block, depth, depth, line_starts, depth_map);
}

fn walk_block_for_depth_ctx(
    block: &brink_ir::Block,
    depth: u32,
    gather_depth: u32,
    line_starts: &[usize],
    depth_map: &mut [u32],
) {
    if let Some(label) = &block.label {
        set_depth_for_range(label.range, depth, line_starts, depth_map);
    }
    for stmt in &block.stmts {
        walk_stmt_for_depth(stmt, depth, gather_depth, line_starts, depth_map);
    }
}

fn walk_stmt_for_depth(
    stmt: &brink_ir::Stmt,
    depth: u32,
    gather_depth: u32,
    line_starts: &[usize],
    depth_map: &mut [u32],
) {
    match stmt {
        brink_ir::Stmt::Content(content) => {
            if let Some(ptr) = &content.ptr {
                set_depth_for_range(ptr.text_range(), depth, line_starts, depth_map);
            }
        }
        brink_ir::Stmt::Divert(divert) => {
            if let Some(ptr) = &divert.ptr {
                set_depth_for_range(ptr.text_range(), depth, line_starts, depth_map);
            }
        }
        brink_ir::Stmt::TunnelCall(tc) => {
            set_depth_for_range(tc.ptr.text_range(), depth, line_starts, depth_map);
        }
        brink_ir::Stmt::ThreadStart(ts) => {
            set_depth_for_range(ts.ptr.text_range(), depth, line_starts, depth_map);
        }
        brink_ir::Stmt::TempDecl(td) => {
            set_depth_for_range(td.ptr.text_range(), depth, line_starts, depth_map);
        }
        brink_ir::Stmt::Assignment(a) => {
            set_depth_for_range(a.ptr.text_range(), depth, line_starts, depth_map);
        }
        brink_ir::Stmt::Return(r) => {
            if let Some(ptr) = &r.ptr {
                set_depth_for_range(ptr.text_range(), depth, line_starts, depth_map);
            }
        }
        brink_ir::Stmt::ChoiceSet(cs) => {
            for choice in &cs.choices {
                set_depth_for_range(choice.ptr.text_range(), depth, line_starts, depth_map);
                walk_block_for_depth(&choice.body, depth + 1, line_starts, depth_map);
            }
            // Continuation gather pops back to the gather depth that started
            // this weave. The gather line (label or first stmt if unlabeled)
            // is at gather_depth; subsequent body content is indented deeper.
            let gather_line = cs
                .continuation
                .label
                .as_ref()
                .map(|l| {
                    let offset: usize = l.range.start().into();
                    line_for_offset(line_starts, offset)
                })
                .or_else(|| {
                    cs.continuation
                        .stmts
                        .first()
                        .and_then(|s| stmt_start_line(s, line_starts))
                });
            if let Some(label) = &cs.continuation.label {
                set_depth_for_range(label.range, gather_depth, line_starts, depth_map);
            }
            for stmt in &cs.continuation.stmts {
                let stmt_line = stmt_start_line(stmt, line_starts);
                // Stmts on the same line as the gather marker stay at
                // gather_depth (e.g. `- -> waited`); others indent.
                let d = if gather_line.is_some() && stmt_line == gather_line {
                    gather_depth
                } else {
                    gather_depth + 1
                };
                walk_stmt_for_depth(stmt, d, gather_depth, line_starts, depth_map);
            }
        }
        brink_ir::Stmt::LabeledBlock(block) => {
            // Gather line at current depth; body content indented one level.
            if let Some(label) = &block.label {
                set_depth_for_range(label.range, depth, line_starts, depth_map);
            }
            for stmt in &block.stmts {
                walk_stmt_for_depth(stmt, depth + 1, depth, line_starts, depth_map);
            }
        }
        brink_ir::Stmt::Conditional(cond) => {
            set_depth_for_range(cond.ptr.text_range(), depth, line_starts, depth_map);
            for branch in &cond.branches {
                walk_block_for_depth(&branch.body, depth + 1, line_starts, depth_map);
            }
        }
        brink_ir::Stmt::Sequence(seq) => {
            set_depth_for_range(seq.ptr.text_range(), depth, line_starts, depth_map);
            for branch in &seq.branches {
                walk_block_for_depth(&branch.body, depth + 1, line_starts, depth_map);
            }
        }
        // T1b `~ { … }` blocks (docs/t1b-surface-spec.md §2, brink
        // extension): the `~ {` line's own depth (i.e. where the block sits
        // relative to the surrounding knot/choice/gather structure) is
        // inherited from context by `propagate_depth`, same as
        // `ExprStmt`/`EndOfLine` — HIR doesn't tag it directly. Indentation
        // of the block's *internals* (nested statements, `if`/`while`/`for`
        // bodies) is computed separately at the CST level by
        // `render_logic_block` (#573), not through this depth map.
        // `~ await <cond>` (docs/flow-suspension-spec.md §3) is a logic line
        // like `TempDecl`/`Assignment` — its own line depth is inherited from
        // context, so tag its range at the current depth.
        brink_ir::Stmt::Await(a) => {
            set_depth_for_range(a.ptr.text_range(), depth, line_starts, depth_map);
        }
        brink_ir::Stmt::ExprStmt(_) | brink_ir::Stmt::EndOfLine | brink_ir::Stmt::LogicBlock(_) => {
        }
    }
}

/// Get the source line of the first token in a statement, if available.
fn stmt_start_line(stmt: &brink_ir::Stmt, line_starts: &[usize]) -> Option<usize> {
    let range = match stmt {
        brink_ir::Stmt::Content(c) => c.ptr.as_ref()?.text_range(),
        brink_ir::Stmt::Divert(d) => d.ptr.as_ref()?.text_range(),
        brink_ir::Stmt::TunnelCall(tc) => tc.ptr.text_range(),
        brink_ir::Stmt::ThreadStart(ts) => ts.ptr.text_range(),
        brink_ir::Stmt::TempDecl(td) => td.ptr.text_range(),
        brink_ir::Stmt::Assignment(a) => a.ptr.text_range(),
        brink_ir::Stmt::Return(r) => r.ptr.as_ref()?.text_range(),
        brink_ir::Stmt::ChoiceSet(cs) => cs.choices.first()?.ptr.text_range(),
        brink_ir::Stmt::LabeledBlock(b) => b.label.as_ref()?.range,
        brink_ir::Stmt::Conditional(c) => c.ptr.text_range(),
        brink_ir::Stmt::Sequence(s) => s.ptr.text_range(),
        brink_ir::Stmt::ExprStmt(_) | brink_ir::Stmt::EndOfLine => return None,
        brink_ir::Stmt::LogicBlock(lb) => lb.ptr.text_range(),
        brink_ir::Stmt::Await(a) => a.ptr.text_range(),
    };
    let offset: usize = range.start().into();
    Some(line_for_offset(line_starts, offset))
}

fn set_depth_for_range(
    range: rowan::TextRange,
    depth: u32,
    line_starts: &[usize],
    depth_map: &mut [u32],
) {
    let offset: usize = range.start().into();
    let line = line_for_offset(line_starts, offset);
    if line < depth_map.len() {
        depth_map[line] = depth_map[line].max(depth);
    }
}

/// Propagate depth to lines that weren't explicitly annotated by the HIR walk.
///
/// Lines with depth 0 that sit between HIR-annotated lines inherit from their
/// context. This covers blank lines, comments, `ExprStmt` (bare `~ fn()` calls),
/// and any other lines the HIR walker doesn't directly tag.
fn propagate_depth(source: &str, line_starts: &[usize], depth_map: &mut [u32]) {
    let is_top_level_line = |i: usize| -> bool {
        let line_start = line_starts[i];
        let line_end = if i + 1 < line_starts.len() {
            line_starts[i + 1]
        } else {
            source.len()
        };
        let trimmed = source[line_start..line_end].trim();
        trimmed.starts_with("===")
            || trimmed.starts_with("VAR ")
            || trimmed.starts_with("CONST ")
            || trimmed.starts_with("LIST ")
            || trimmed.starts_with("INCLUDE ")
            || trimmed.starts_with("EXTERNAL ")
    };

    // Forward pass: inherit depth from the nearest preceding annotated line.
    // Reset when crossing a top-level line so root-scope comments/blanks
    // don't inherit depth from inside a knot body.
    let mut last_depth = 0u32;
    #[expect(
        clippy::needless_range_loop,
        reason = "mutating depth_map[i] based on prior state"
    )]
    for i in 0..depth_map.len() {
        if is_top_level_line(i) {
            last_depth = 0;
        } else if depth_map[i] > 0 {
            last_depth = depth_map[i];
        } else if last_depth > 0 {
            depth_map[i] = last_depth;
        }
    }

    // Backward pass: only fill lines still at depth 0 (forward couldn't
    // reach them — e.g. lines before the first annotated line in a block).
    // Reset when crossing a top-level line.
    let mut next_depth = 0u32;
    for i in (0..depth_map.len()).rev() {
        if is_top_level_line(i) {
            next_depth = 0;
        } else if depth_map[i] > 0 {
            next_depth = depth_map[i];
        } else if next_depth > 0 {
            depth_map[i] = next_depth;
        }
    }
}
