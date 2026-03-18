//! Per-line structural context derived from the HIR.
//!
//! `line_contexts()` returns one `LineContext` per source line, giving the
//! editor authoritative information about element type, weave position,
//! and inline structure — replacing the regex-based `classifyLine` in TS.

use brink_ir::{Block, ChoiceSetContext, Content, ContentPart, HirFile, Stmt};
use brink_syntax::SyntaxNode;
use serde::Serialize;

use crate::LineIndex;

// ── Types ───────────────────────────────────────────────────────────

/// The top-level structural element on a source line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineElement {
    KnotHeader,
    StitchHeader,
    Narrative,
    Choice,
    Gather,
    Divert,
    Logic,
    VarDecl,
    Comment,
    Include,
    External,
    Tag,
    Blank,
}

/// Position within the weave structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WeavePosition {
    /// Weave nesting depth (1-based for weave elements, 0 for top-level).
    pub depth: u32,
    /// What kind of weave element this line belongs to.
    pub element: WeaveElement,
}

/// The weave role of a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WeaveElement {
    /// Not inside any weave structure.
    TopLevel,
    /// A choice line (`*` or `+`).
    ChoiceLine {
        /// Whether this is a sticky (`+`) choice.
        sticky: bool,
    },
    /// Body text following a choice (indented content in the choice's body block).
    ChoiceBody,
    /// Content after a gather point (the continuation block).
    GatherContinuation,
    /// Inside a conditional branch body.
    ConditionalBranch,
    /// Inside a sequence branch body.
    SequenceBranch,
}

/// Full per-line context.
#[derive(Debug, Clone, Serialize)]
pub struct LineContext {
    /// The structural element type for this line.
    pub element: LineElement,
    /// Weave position (depth + role).
    pub weave: WeavePosition,
    /// Whether this line has tags (from HIR).
    pub has_tags: bool,
    /// Whether this line is inside a block comment.
    pub block_comment: bool,
}

impl Default for LineContext {
    fn default() -> Self {
        Self {
            element: LineElement::Blank,
            weave: WeavePosition {
                depth: 0,
                element: WeaveElement::TopLevel,
            },
            has_tags: false,
            block_comment: false,
        }
    }
}

// ── Public API ──────────────────────────────────────────────────────

/// Compute per-line context from the HIR and source text.
///
/// Returns one `LineContext` per source line. The `root` syntax node is
/// used for block-comment detection; the HIR provides all structural info.
pub fn line_contexts(hir: &HirFile, source: &str, root: &SyntaxNode) -> Vec<LineContext> {
    let line_count = source.lines().count().max(1);
    // Handle trailing newline: if source ends with '\n', there's an extra empty line
    let actual_lines = if source.ends_with('\n') {
        line_count + 1
    } else {
        line_count
    };
    let mut ctx = vec![LineContext::default(); actual_lines];
    let idx = LineIndex::new(source);

    // ── Pass 1: classify from source text (comments, block comments) ──
    detect_comments(source, &mut ctx);

    // ── Pass 2: detect block comments from syntax tree ──
    detect_block_comments(root, &idx, &mut ctx);

    // ── Pass 3: walk HIR structure ──

    // Top-level declarations
    for var in &hir.variables {
        set_element_at_range(&idx, var.ptr.text_range(), LineElement::VarDecl, &mut ctx);
    }
    for con in &hir.constants {
        set_element_at_range(&idx, con.ptr.text_range(), LineElement::VarDecl, &mut ctx);
    }
    for list in &hir.lists {
        set_element_at_range(&idx, list.ptr.text_range(), LineElement::VarDecl, &mut ctx);
    }
    for ext in &hir.externals {
        set_element_at_range(&idx, ext.ptr.text_range(), LineElement::External, &mut ctx);
    }
    for inc in &hir.includes {
        set_element_at_range(&idx, inc.ptr.text_range(), LineElement::Include, &mut ctx);
    }

    let top_level = WeavePosition {
        depth: 0,
        element: WeaveElement::TopLevel,
    };

    // Root content block
    walk_block(&hir.root_content, &idx, &mut ctx, top_level);

    // Knots and stitches
    for knot in &hir.knots {
        let knot_line = idx.line_col(knot.ptr.text_range().start()).0 as usize;
        if knot_line < ctx.len() {
            ctx[knot_line].element = LineElement::KnotHeader;
        }

        walk_block(&knot.body, &idx, &mut ctx, top_level);

        for stitch in &knot.stitches {
            let stitch_line = idx.line_col(stitch.ptr.text_range().start()).0 as usize;
            if stitch_line < ctx.len() {
                ctx[stitch_line].element = LineElement::StitchHeader;
            }

            walk_block(&stitch.body, &idx, &mut ctx, top_level);
        }
    }

    ctx
}

// ── HIR walking ─────────────────────────────────────────────────────

fn walk_block(block: &Block, idx: &LineIndex, ctx: &mut [LineContext], weave: WeavePosition) {
    for stmt in &block.stmts {
        walk_stmt(stmt, idx, ctx, weave);
    }
}

fn walk_stmt(stmt: &Stmt, idx: &LineIndex, ctx: &mut [LineContext], weave: WeavePosition) {
    match stmt {
        Stmt::Content(content) => {
            set_content_lines(content, idx, ctx, LineElement::Narrative, weave);
        }
        Stmt::Divert(divert) => {
            if let Some(ptr) = &divert.ptr {
                set_line(
                    idx,
                    ctx,
                    ptr.text_range().start(),
                    LineElement::Divert,
                    weave,
                );
            }
        }
        Stmt::TunnelCall(tc) => {
            set_line(
                idx,
                ctx,
                tc.ptr.text_range().start(),
                LineElement::Divert,
                weave,
            );
        }
        Stmt::ThreadStart(ts) => {
            set_line(
                idx,
                ctx,
                ts.ptr.text_range().start(),
                LineElement::Divert,
                weave,
            );
        }
        Stmt::TempDecl(td) => {
            set_line(
                idx,
                ctx,
                td.ptr.text_range().start(),
                LineElement::Logic,
                weave,
            );
        }
        Stmt::Assignment(a) => {
            set_line(
                idx,
                ctx,
                a.ptr.text_range().start(),
                LineElement::Logic,
                weave,
            );
        }
        Stmt::Return(r) => {
            if let Some(ptr) = &r.ptr {
                set_line(
                    idx,
                    ctx,
                    ptr.text_range().start(),
                    LineElement::Logic,
                    weave,
                );
            }
        }
        Stmt::ChoiceSet(cs) => walk_choice_set(cs, idx, ctx, weave),
        Stmt::LabeledBlock(block) => walk_labeled_block(block, idx, ctx, weave),
        Stmt::Conditional(cond) => {
            for branch in &cond.branches {
                walk_block(
                    &branch.body,
                    idx,
                    ctx,
                    WeavePosition {
                        depth: weave.depth,
                        element: WeaveElement::ConditionalBranch,
                    },
                );
            }
        }
        Stmt::Sequence(seq) => {
            for branch in &seq.branches {
                walk_block(
                    branch,
                    idx,
                    ctx,
                    WeavePosition {
                        depth: weave.depth,
                        element: WeaveElement::SequenceBranch,
                    },
                );
            }
        }
        Stmt::ExprStmt(_) | Stmt::EndOfLine => {}
    }
}

fn walk_choice_set(
    cs: &brink_ir::ChoiceSet,
    idx: &LineIndex,
    ctx: &mut [LineContext],
    weave: WeavePosition,
) {
    let depth = if cs.context == ChoiceSetContext::Inline {
        weave.depth
    } else {
        cs.depth
    };

    for choice in &cs.choices {
        let choice_line = idx.line_col(choice.ptr.text_range().start()).0 as usize;
        if choice_line < ctx.len() {
            ctx[choice_line].element = LineElement::Choice;
            ctx[choice_line].weave = WeavePosition {
                depth,
                element: WeaveElement::ChoiceLine {
                    sticky: choice.is_sticky,
                },
            };
            ctx[choice_line].has_tags = !choice.tags.is_empty();
        }

        walk_block(
            &choice.body,
            idx,
            ctx,
            WeavePosition {
                depth,
                element: WeaveElement::ChoiceBody,
            },
        );
    }

    // Continuation (gather)
    if !cs.continuation.stmts.is_empty() || cs.continuation.label.is_some() {
        walk_block(
            &cs.continuation,
            idx,
            ctx,
            WeavePosition {
                depth,
                element: WeaveElement::GatherContinuation,
            },
        );

        if cs.continuation.label.is_some()
            && let Some(first_stmt) = cs.continuation.stmts.first()
            && let Some(line) = stmt_start_line(first_stmt, idx)
            && line < ctx.len()
        {
            ctx[line].element = LineElement::Gather;
        }
    }
}

fn walk_labeled_block(
    block: &Block,
    idx: &LineIndex,
    ctx: &mut [LineContext],
    weave: WeavePosition,
) {
    if block.label.is_some()
        && let Some(first_stmt) = block.stmts.first()
        && let Some(line) = stmt_start_line(first_stmt, idx)
        && line < ctx.len()
    {
        ctx[line].element = LineElement::Gather;
    }
    walk_block(block, idx, ctx, weave);
}

// ── Helpers ─────────────────────────────────────────────────────────

fn set_line(
    idx: &LineIndex,
    ctx: &mut [LineContext],
    offset: rowan::TextSize,
    element: LineElement,
    weave: WeavePosition,
) {
    let line = idx.line_col(offset).0 as usize;
    if line < ctx.len() {
        ctx[line].element = element;
        ctx[line].weave = weave;
    }
}

fn set_element_at_range(
    idx: &LineIndex,
    range: rowan::TextRange,
    element: LineElement,
    ctx: &mut [LineContext],
) {
    let line = idx.line_col(range.start()).0 as usize;
    if line < ctx.len() {
        ctx[line].element = element;
    }
}

fn set_content_lines(
    content: &Content,
    idx: &LineIndex,
    ctx: &mut [LineContext],
    element: LineElement,
    weave: WeavePosition,
) {
    if let Some(ptr) = &content.ptr {
        let start_line = idx.line_col(ptr.text_range().start()).0 as usize;
        let end_line = idx.line_col(ptr.text_range().end()).0 as usize;
        for line in start_line..=end_line {
            if line < ctx.len() && ctx[line].element == LineElement::Blank {
                ctx[line].element = element;
                ctx[line].weave = weave;
            }
        }
        if !content.tags.is_empty() && start_line < ctx.len() {
            ctx[start_line].has_tags = true;
        }
    }

    // Recurse into inline content parts for nested conditionals/sequences
    for part in &content.parts {
        match part {
            ContentPart::InlineConditional(cond) => {
                for branch in &cond.branches {
                    walk_block(
                        &branch.body,
                        idx,
                        ctx,
                        WeavePosition {
                            depth: weave.depth,
                            element: WeaveElement::ConditionalBranch,
                        },
                    );
                }
            }
            ContentPart::InlineSequence(seq) => {
                for branch in &seq.branches {
                    walk_block(
                        branch,
                        idx,
                        ctx,
                        WeavePosition {
                            depth: weave.depth,
                            element: WeaveElement::SequenceBranch,
                        },
                    );
                }
            }
            _ => {}
        }
    }
}

fn stmt_start_line(stmt: &Stmt, idx: &LineIndex) -> Option<usize> {
    use brink_syntax::ast::{AstPtr, SyntaxNodePtr};

    let range = match stmt {
        Stmt::Content(c) => c.ptr.as_ref().map(SyntaxNodePtr::text_range),
        Stmt::Divert(d) => d.ptr.as_ref().map(SyntaxNodePtr::text_range),
        Stmt::TunnelCall(tc) => Some(tc.ptr.text_range()),
        Stmt::ThreadStart(ts) => Some(ts.ptr.text_range()),
        Stmt::TempDecl(td) => Some(td.ptr.text_range()),
        Stmt::Assignment(a) => Some(a.ptr.text_range()),
        Stmt::Return(r) => r.ptr.as_ref().map(AstPtr::text_range),
        Stmt::ChoiceSet(cs) => cs.choices.first().map(|c| c.ptr.text_range()),
        Stmt::LabeledBlock(_)
        | Stmt::Conditional(_)
        | Stmt::Sequence(_)
        | Stmt::ExprStmt(_)
        | Stmt::EndOfLine => None,
    };
    range.map(|r| idx.line_col(r.start()).0 as usize)
}

/// Detect single-line comments and tag lines from source text.
fn detect_comments(source: &str, ctx: &mut [LineContext]) {
    for (i, line) in source.lines().enumerate() {
        if i >= ctx.len() {
            break;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            ctx[i].element = LineElement::Comment;
        } else if trimmed.starts_with('#')
            && !trimmed.is_empty()
            && ctx[i].element == LineElement::Blank
        {
            ctx[i].element = LineElement::Tag;
        }
    }
}

/// Detect block comments (`/* ... */`) from the syntax tree.
fn detect_block_comments(root: &SyntaxNode, idx: &LineIndex, ctx: &mut [LineContext]) {
    use brink_syntax::SyntaxKind;

    for token in root.descendants_with_tokens() {
        if let Some(token) = token.as_token()
            && token.kind() == SyntaxKind::BLOCK_COMMENT
        {
            let range = token.text_range();
            let start_line = idx.line_col(range.start()).0 as usize;
            let end_line = idx.line_col(range.end()).0 as usize;
            for line in start_line..=end_line {
                if line < ctx.len() {
                    ctx[line].element = LineElement::Comment;
                    ctx[line].block_comment = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::{FileId, hir};

    fn make_contexts(source: &str) -> Vec<LineContext> {
        let parse = brink_syntax::parse(source);
        let file_id = FileId(0);
        let ast = parse.tree();
        let (hir, _, _) = hir::lower(file_id, &ast);
        line_contexts(&hir, source, &parse.syntax())
    }

    #[test]
    fn knot_and_stitch_headers() {
        let source = "=== my_knot ===\n= my_stitch\nHello\n";
        let ctx = make_contexts(source);
        assert_eq!(ctx[0].element, LineElement::KnotHeader);
        assert_eq!(ctx[1].element, LineElement::StitchHeader);
        assert_eq!(ctx[2].element, LineElement::Narrative);
    }

    #[test]
    fn choice_depth_from_hir() {
        let source = "=== start ===\n* Choice one\n* * Nested choice\n";
        let ctx = make_contexts(source);
        assert_eq!(ctx[1].element, LineElement::Choice);
        assert_eq!(ctx[1].weave.depth, 1);
        // Nested choice at depth 2
        assert_eq!(ctx[2].element, LineElement::Choice);
        assert_eq!(ctx[2].weave.depth, 2);
    }

    #[test]
    fn divert_and_logic() {
        let source = "=== start ===\n~ temp x = 5\n-> END\n";
        let ctx = make_contexts(source);
        assert_eq!(ctx[1].element, LineElement::Logic);
        assert_eq!(ctx[2].element, LineElement::Divert);
    }

    #[test]
    fn var_and_include() {
        let source = "VAR x = 5\nINCLUDE other.ink\n";
        let ctx = make_contexts(source);
        assert_eq!(ctx[0].element, LineElement::VarDecl);
        assert_eq!(ctx[1].element, LineElement::Include);
    }

    #[test]
    fn comments() {
        let source = "// A comment\nHello\n";
        let ctx = make_contexts(source);
        assert_eq!(ctx[0].element, LineElement::Comment);
    }

    #[test]
    fn blank_lines() {
        let source = "\n\nHello\n";
        let ctx = make_contexts(source);
        assert_eq!(ctx[0].element, LineElement::Blank);
        assert_eq!(ctx[1].element, LineElement::Blank);
    }

    #[test]
    fn sticky_choice() {
        let source = "=== start ===\n+ Sticky choice\n";
        let ctx = make_contexts(source);
        assert_eq!(ctx[1].element, LineElement::Choice);
        assert!(matches!(
            ctx[1].weave.element,
            WeaveElement::ChoiceLine { sticky: true }
        ));
    }
}
