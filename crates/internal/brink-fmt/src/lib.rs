//! Source code formatter for inkle's ink narrative scripting language.
//!
//! Parses the input with `brink_syntax::parse`, lowers to HIR for structural
//! nesting information, then walks the CST to classify each source line and
//! reformats according to consistent rules. HIR provides the correct
//! indentation depth for every source line.

use brink_ir::hir;
use brink_syntax::ParseError;
use brink_syntax::SyntaxElement;
use brink_syntax::SyntaxNode;
use brink_syntax::syntax_kind::SyntaxKind;
use rowan::NodeOrToken;

// ── Public API ──────────────────────────────────────────────────────

/// How to indent nested constructs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndentStyle {
    Tabs,
    Spaces(u32),
}

/// Configuration for the formatter.
#[derive(Debug, Clone)]
pub struct FormatConfig {
    pub indent: IndentStyle,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            indent: IndentStyle::Spaces(2),
        }
    }
}

/// Format an entire ink source string. Returns the formatted source.
#[must_use]
pub fn format(source: &str, config: &FormatConfig) -> String {
    let parse = brink_syntax::parse(source);
    let root = parse.syntax();

    // Lower to HIR to get structural nesting information.
    let file_id = brink_ir::FileId(0);
    let tree = parse.tree();
    let (hir_file, _, _) = hir::lower(file_id, &tree);

    // Build a depth map from HIR: line number → indentation depth.
    let line_starts = build_line_starts(source);
    let depth_map = build_depth_map(source, &line_starts, &hir_file);

    let lines = classify_lines(source, &root, parse.errors(), &depth_map);
    render(source, &lines, config)
}

// ── Line starts helper ──────────────────────────────────────────────

fn build_line_starts(source: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(source.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}

/// Find the line number for a byte offset.
fn line_for_offset(line_starts: &[usize], offset: usize) -> usize {
    match line_starts.binary_search(&offset) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    }
}

// ── HIR-based depth map ─────────────────────────────────────────────

/// Build a map from line number → indentation depth by walking the HIR tree.
fn build_depth_map(source: &str, line_starts: &[usize], hir_file: &brink_ir::HirFile) -> Vec<u32> {
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
                walk_block_for_depth(branch, depth + 1, line_starts, depth_map);
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

// ── Line classification ─────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum LineKind {
    KnotHeader,
    StitchHeader,
    Choice {
        depth: u32,
    },
    Gather {
        depth: u32,
    },
    Logic,
    Content,
    Tag,
    Blank,
    Declaration,
    Comment,
    BlockComment,
    Other,
    /// A T1b `~ { … }` multi-line block (docs/t1b-surface-spec.md §2, brink
    /// extension). This line is the block's opening `~ {` line; every
    /// physical line the block spans (through the trailing newline after
    /// the closing `}`) becomes [`LineKind::Skip`], and `cst_node`
    /// carries the `LOGIC_LINE` CST node so `render()` can walk its
    /// `STMT_BLOCK` and reindent the internals (#573).
    LogicBlock,
    /// A T1b `~ { … }` multi-line block whose CST subtree contains a parse
    /// error (#603) — mid-edit or otherwise malformed input. Reindenting it
    /// with `render_logic_block` assumes well-formed structure and can
    /// corrupt it (a trailing comment swallowing the next line's `{`,
    /// spurious blank lines and broken idempotence around a multi-line call,
    /// a mangled lone `else`/brace line). This line and every physical line
    /// through the trailing newline after the block's closing `}` (or EOF,
    /// if the `}` itself is what's missing) are widened into a single
    /// byte-for-byte verbatim span — the pre-#602 behavior — for this block
    /// only; well-formed blocks still go through [`LineKind::LogicBlock`].
    LogicBlockVerbatim,
    /// A STRUCT declaration (TM-4b, docs/typed-mode-spec.md §6). This line
    /// is the opening `STRUCT Name = #{` line; every physical line the struct
    /// spans (through the trailing newline after the closing `}`) becomes
    /// [`LineKind::Skip`], and `struct_decl_node` carries the `STRUCT_DECL`
    /// CST node so `render()` can walk its fields and reindent the internals.
    StructDecl,
    /// A line already emitted as part of a preceding [`LineKind::LogicBlock`],
    /// [`LineKind::LogicBlockVerbatim`], or [`LineKind::StructDecl`] span —
    /// renders nothing.
    Skip,
}

#[derive(Debug)]
struct ClassifiedLine {
    kind: LineKind,
    /// Byte offset of the start of this line in the source.
    start: usize,
    /// Byte offset one past the end of this line (excluding newline).
    end: usize,
    /// Indentation depth from HIR structure.
    depth: u32,
    /// The `LOGIC_LINE` CST node for a [`LineKind::LogicBlock`] line, or
    /// the `STRUCT_DECL` CST node for a [`LineKind::StructDecl`] line —
    /// `None` for every other kind.
    cst_node: Option<SyntaxNode>,
}

/// Classify every line in the source by walking the CST, using HIR depth map.
fn classify_lines(
    source: &str,
    root: &SyntaxNode,
    errors: &[ParseError],
    depth_map: &[u32],
) -> Vec<ClassifiedLine> {
    let line_starts = build_line_starts(source);
    let line_count = line_starts.len();

    // Initialize all lines as Blank.
    let mut lines: Vec<ClassifiedLine> = (0..line_count)
        .map(|i| {
            let start = line_starts[i];
            let end = if i + 1 < line_count {
                let next_start = line_starts[i + 1];
                if next_start > 0 && source.as_bytes().get(next_start - 1) == Some(&b'\n') {
                    let before_nl = next_start - 1;
                    if before_nl > 0 && source.as_bytes().get(before_nl - 1) == Some(&b'\r') {
                        before_nl - 1
                    } else {
                        before_nl
                    }
                } else {
                    next_start
                }
            } else {
                source.len()
            };
            ClassifiedLine {
                kind: LineKind::Blank,
                start,
                end,
                depth: depth_map.get(i).copied().unwrap_or(0),
                cst_node: None,
            }
        })
        .collect();

    // Mark lines that are inside block comments.
    mark_block_comments(root, &line_starts, &mut lines);

    // Walk CST to classify line kinds (but depth comes from HIR).
    classify_node(root, &line_starts, errors, &mut lines);

    // Check for lines that are still Blank but have non-whitespace content.
    for line in &mut lines {
        if line.kind == LineKind::Blank {
            let text = &source[line.start..line.end];
            if !text.trim().is_empty() {
                line.kind = LineKind::Other;
            }
        }
    }

    lines
}

/// Mark lines inside block comments as `BlockComment`.
fn mark_block_comments(root: &SyntaxNode, line_starts: &[usize], lines: &mut [ClassifiedLine]) {
    for token in root.descendants_with_tokens() {
        if let NodeOrToken::Token(tok) = token
            && tok.kind() == SyntaxKind::BLOCK_COMMENT
        {
            let start_line = line_for_offset(line_starts, tok.text_range().start().into());
            let end_line = line_for_offset(line_starts, tok.text_range().end().into());
            let last = end_line.min(lines.len() - 1);
            for line in &mut lines[start_line..=last] {
                line.kind = LineKind::BlockComment;
            }
        }
    }
}

/// Walk CST to classify line kinds. Depth is already set from HIR.
fn classify_node(
    node: &SyntaxNode,
    line_starts: &[usize],
    errors: &[ParseError],
    lines: &mut [ClassifiedLine],
) {
    for child in node.children() {
        let start_offset: usize = child.text_range().start().into();
        let line_idx = line_for_offset(line_starts, start_offset);

        if line_idx < lines.len() && lines[line_idx].kind == LineKind::BlockComment {
            continue;
        }

        match child.kind() {
            SyntaxKind::KNOT_HEADER => {
                if line_idx < lines.len() {
                    lines[line_idx].kind = LineKind::KnotHeader;
                    lines[line_idx].depth = 0;
                }
            }
            SyntaxKind::STITCH_HEADER => {
                if line_idx < lines.len() {
                    lines[line_idx].kind = LineKind::StitchHeader;
                    // Depth comes from HIR depth map (propagated from context).
                }
            }
            SyntaxKind::CHOICE => {
                let depth = choice_depth(&child);
                if line_idx < lines.len() {
                    lines[line_idx].kind = LineKind::Choice { depth };
                    // HIR depth already set for choices.
                }
                classify_node(&child, line_starts, errors, lines);
            }
            SyntaxKind::GATHER => {
                let depth = gather_depth(&child);
                if line_idx < lines.len() {
                    lines[line_idx].kind = LineKind::Gather { depth };
                }
                classify_node(&child, line_starts, errors, lines);
            }
            SyntaxKind::LOGIC_LINE => {
                // T1b `~ { … }` multi-line blocks (docs/t1b-surface-spec.md
                // §2, brink extension) span several physical lines —
                // reindent them as a unit via `render_logic_block` (#573)
                // rather than classifying each inner physical line. But
                // first: if this block's own subtree contains a parse error
                // (#603 — mid-edit or otherwise malformed input),
                // `render_logic_block` assumes well-formed structure and can
                // corrupt it, so bail to the pre-#602 verbatim pass-through
                // for this block only.
                if child.children().any(|c| c.kind() == SyntaxKind::STMT_BLOCK) {
                    if subtree_has_parse_error(child.text_range(), errors) {
                        mark_verbatim_span(&child, line_starts, lines);
                    } else {
                        mark_logic_block_span(&child, line_starts, lines);
                    }
                } else if line_idx < lines.len() {
                    lines[line_idx].kind = LineKind::Logic;
                }
            }
            SyntaxKind::CONTENT_LINE => {
                if line_idx < lines.len() {
                    lines[line_idx].kind = LineKind::Content;
                }
            }
            SyntaxKind::TAG_LINE => {
                if line_idx < lines.len() {
                    lines[line_idx].kind = LineKind::Tag;
                }
            }
            SyntaxKind::VAR_DECL
            | SyntaxKind::CONST_DECL
            | SyntaxKind::LIST_DECL
            | SyntaxKind::INCLUDE_STMT
            | SyntaxKind::EXTERNAL_DECL => {
                if line_idx < lines.len() {
                    lines[line_idx].kind = LineKind::Declaration;
                    lines[line_idx].depth = 0;
                }
            }
            // TM-4b (docs/typed-mode-spec.md §6): `STRUCT` decl body can span
            // multiple lines. Format like blocks with proper field indentation.
            SyntaxKind::STRUCT_DECL => {
                if subtree_has_parse_error(child.text_range(), errors) {
                    mark_verbatim_span(&child, line_starts, lines);
                } else {
                    mark_struct_decl_span(&child, line_starts, lines);
                }
            }
            SyntaxKind::EMPTY_LINE => {
                if line_idx < lines.len() {
                    lines[line_idx].kind = LineKind::Blank;
                }
            }
            SyntaxKind::KNOT_DEF
            | SyntaxKind::KNOT_BODY
            | SyntaxKind::STITCH_DEF
            | SyntaxKind::STITCH_BODY
            | SyntaxKind::SOURCE_FILE => {
                classify_node(&child, line_starts, errors, lines);
            }
            _ => {
                if is_comment_only(&child) && line_idx < lines.len() {
                    lines[line_idx].kind = LineKind::Comment;
                }
                classify_node(&child, line_starts, errors, lines);
            }
        }
    }
}

/// Mark every physical line spanned by `node` (a T1b `~ { … }` multi-line
/// block's `LOGIC_LINE`) as belonging to a single reindented unit: the first
/// line becomes [`LineKind::LogicBlock`] carrying `node` itself (so `render()`
/// can walk its `STMT_BLOCK` and reindent), and every subsequent physical
/// line spanned (through the trailing newline after the closing `}`, which
/// the parser always includes in the `LOGIC_LINE` node) becomes
/// [`LineKind::Skip`] so it renders nothing of its own.
fn mark_logic_block_span(node: &SyntaxNode, line_starts: &[usize], lines: &mut [ClassifiedLine]) {
    let range = node.text_range();
    let start_line = line_for_offset(line_starts, range.start().into());
    let end_line = line_for_offset(line_starts, range.end().into());
    if start_line >= lines.len() {
        return;
    }
    lines[start_line].kind = LineKind::LogicBlock;
    lines[start_line].cst_node = Some(node.clone());
    let last = end_line.min(lines.len() - 1);
    for line in &mut lines[start_line + 1..=last] {
        line.kind = LineKind::Skip;
    }
}

/// Does any parse error touch or fall inside `range`? Used to decide whether
/// a `~ { … }` block's subtree is well-formed enough for `render_logic_block`
/// to reindent (#603).
///
/// This checks [`brink_syntax::Parse::errors`] rather than scanning the
/// subtree for `ERROR` CST nodes: not every recovery path wraps a node.
/// `Parser::expect` (a missing expected token, e.g. an absent closing `}`
/// or `IDENT`) records a `ParseError` with a zero-length range at the point
/// the token was expected, without inserting an `ERROR` node — scanning for
/// `ERROR` nodes alone would miss it. `TextRange::intersect` treats a
/// zero-length range that merely touches `range`'s boundary as a hit too
/// (`Some` with an empty range), which is what we want here: a missing `}`
/// at the very end of the block is still a reason not to trust its
/// structure.
fn subtree_has_parse_error(range: rowan::TextRange, errors: &[ParseError]) -> bool {
    errors.iter().any(|e| range.intersect(e.range).is_some())
}

/// Mark every physical line spanned by `node` (a T1b `~ { … }` multi-line
/// block's `LOGIC_LINE` whose CST subtree contains a parse error, #603) as a
/// verbatim pass-through: the first line's `start`/`end` are widened to cover
/// the node's *entire* text range (through the trailing newline after the
/// closing `}`, which the parser always includes in the `LOGIC_LINE` node —
/// or through EOF, if the `}` itself is what's missing), and every
/// subsequent line spanned becomes [`LineKind::Skip`] so it renders nothing
/// of its own. This is the pre-#602 behavior for `~ { … }` blocks, applied
/// here only when the block's own subtree isn't well-formed enough to trust
/// `render_logic_block`'s structural assumptions.
fn mark_verbatim_span(node: &SyntaxNode, line_starts: &[usize], lines: &mut [ClassifiedLine]) {
    let range = node.text_range();
    let start_line = line_for_offset(line_starts, range.start().into());
    let end_line = line_for_offset(line_starts, range.end().into());
    if start_line >= lines.len() {
        return;
    }
    lines[start_line].kind = LineKind::LogicBlockVerbatim;
    // Anchor to the physical line start, not the `~` offset — otherwise an
    // indented block's first line is dedented while its body keeps its
    // indentation (review finding on #610).
    lines[start_line].start = line_starts[start_line];
    lines[start_line].end = range.end().into();
    lines[start_line].cst_node = None;
    let last = end_line.min(lines.len() - 1);
    for line in &mut lines[start_line + 1..=last] {
        line.kind = LineKind::Skip;
    }
}

/// Mark every physical line spanned by `node` (a `STRUCT_DECL`) as belonging to
/// a single reindented unit: the first line becomes [`LineKind::StructDecl`]
/// carrying `node` itself (so `render()` can walk its field declarations and
/// reindent), and every subsequent physical line spanned (through the trailing
/// newline after the closing `}`) becomes [`LineKind::Skip`] so it renders
/// nothing of its own.
fn mark_struct_decl_span(node: &SyntaxNode, line_starts: &[usize], lines: &mut [ClassifiedLine]) {
    let range = node.text_range();
    let start_line = line_for_offset(line_starts, range.start().into());
    let end_line = line_for_offset(line_starts, range.end().into());
    if start_line >= lines.len() {
        return;
    }
    lines[start_line].kind = LineKind::StructDecl;
    lines[start_line].cst_node = Some(node.clone());
    let last = end_line.min(lines.len() - 1);
    for line in &mut lines[start_line + 1..=last] {
        line.kind = LineKind::Skip;
    }
}

/// Check if a node contains only whitespace and line comments.
fn is_comment_only(node: &SyntaxNode) -> bool {
    let mut has_comment = false;
    for elem in node.children_with_tokens() {
        match elem {
            NodeOrToken::Token(tok) => match tok.kind() {
                SyntaxKind::LINE_COMMENT => has_comment = true,
                SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => {}
                _ => return false,
            },
            NodeOrToken::Node(_) => return false,
        }
    }
    has_comment
}

/// Count the number of bullet tokens (`*` or `+`) in a CHOICE node.
fn choice_depth(node: &SyntaxNode) -> u32 {
    for child in node.children() {
        if child.kind() == SyntaxKind::CHOICE_BULLETS {
            let n = child
                .children_with_tokens()
                .filter(|t| matches!(t.kind(), SyntaxKind::STAR | SyntaxKind::PLUS))
                .count();
            #[expect(clippy::cast_possible_truncation, reason = "choice depth fits in u32")]
            return n as u32;
        }
    }
    1
}

/// Count the number of dash tokens in a GATHER node.
fn gather_depth(node: &SyntaxNode) -> u32 {
    for child in node.children() {
        if child.kind() == SyntaxKind::GATHER_DASHES {
            let n = child
                .children_with_tokens()
                .filter(|t| t.kind() == SyntaxKind::MINUS)
                .count();
            #[expect(clippy::cast_possible_truncation, reason = "gather depth fits in u32")]
            return n as u32;
        }
    }
    1
}

// ── Rendering ───────────────────────────────────────────────────────

fn indent_str(config: &FormatConfig, depth: u32) -> String {
    if depth == 0 {
        return String::new();
    }
    match &config.indent {
        IndentStyle::Tabs => "\t".repeat(depth as usize),
        IndentStyle::Spaces(n) => " ".repeat((depth * n) as usize),
    }
}

#[expect(clippy::too_many_lines)]
fn render(source: &str, lines: &[ClassifiedLine], config: &FormatConfig) -> String {
    let mut out = String::with_capacity(source.len());
    let mut consecutive_blanks: u32 = 0;

    for (i, line) in lines.iter().enumerate() {
        let raw = &source[line.start..line.end];

        match &line.kind {
            LineKind::Blank => {
                consecutive_blanks += 1;
                if consecutive_blanks <= 1 {
                    out.push('\n');
                }
                continue;
            }
            LineKind::BlockComment => {
                consecutive_blanks = 0;
                out.push_str(raw.trim_end());
                out.push('\n');
                continue;
            }
            // Already emitted as part of a preceding `LogicBlock` or
            // `LogicBlockVerbatim` span (T1b `~ { … }` block,
            // docs/t1b-surface-spec.md §2) — nothing to do, and it must not
            // reset `consecutive_blanks` or count as a line of its own.
            LineKind::Skip => continue,
            _ => {}
        }

        // Ensure blank line before knot/stitch headers.
        if matches!(line.kind, LineKind::KnotHeader | LineKind::StitchHeader)
            && i > 0
            && consecutive_blanks == 0
        {
            let prev_kind = &lines[i - 1].kind;
            if !matches!(
                prev_kind,
                LineKind::Blank | LineKind::Comment | LineKind::BlockComment
            ) {
                out.push('\n');
            }
        }

        consecutive_blanks = 0;

        match &line.kind {
            LineKind::KnotHeader => {
                out.push_str(&format_knot_header(raw));
                out.push('\n');
            }
            LineKind::StitchHeader => {
                let indent = indent_str(config, line.depth);
                out.push_str(&indent);
                out.push_str(&format_stitch_header(raw));
                out.push('\n');
            }
            LineKind::Choice { depth } => {
                let indent = indent_str(config, line.depth);
                out.push_str(&indent);
                out.push_str(&format_choice(raw, *depth));
                out.push('\n');
            }
            LineKind::Gather { .. } => {
                let indent = indent_str(config, line.depth);
                out.push_str(&indent);
                out.push_str(&format_gather(raw));
                out.push('\n');
            }
            LineKind::Logic => {
                let indent = indent_str(config, line.depth);
                out.push_str(&indent);
                out.push_str(&format_logic(raw));
                out.push('\n');
            }
            LineKind::Content | LineKind::Tag | LineKind::Comment | LineKind::Other => {
                let indent = indent_str(config, line.depth);
                out.push_str(&indent);
                out.push_str(raw.trim());
                out.push('\n');
            }
            LineKind::Declaration => {
                out.push_str(raw.trim_end());
                out.push('\n');
            }
            // Reindent the whole `~ { … }` block as a unit: `cst_node`
            // is the `LOGIC_LINE` CST node (see `mark_logic_block_span`);
            // `render_logic_block` walks its `STMT_BLOCK` recursively,
            // computing indentation independently of `raw`.
            LineKind::LogicBlock => {
                if let Some(node) = &line.cst_node {
                    let base_indent = indent_str(config, line.depth);
                    out.push_str(&render_logic_block(node, &base_indent));
                }
            }
            // `raw` already spans the whole node through its own trailing
            // newline (see `mark_verbatim_span`) — push it byte-for-byte,
            // with no extra trim/indent/newline. This block's subtree has a
            // parse error (#603), so its structure can't be trusted enough
            // to reindent.
            LineKind::LogicBlockVerbatim => {
                out.push_str(raw);
            }
            // Reindent the whole STRUCT declaration as a unit: `cst_node`
            // is the `STRUCT_DECL` CST node (see `mark_struct_decl_span`);
            // `render_struct_decl` walks its field declarations and reindents
            // the body with proper formatting (TM-4b, docs/typed-mode-spec.md §6).
            LineKind::StructDecl => {
                if let Some(node) = &line.cst_node {
                    let base_indent = indent_str(config, line.depth);
                    out.push_str(&render_struct_decl(node, &base_indent, config));
                }
            }
            LineKind::Blank | LineKind::BlockComment | LineKind::Skip => unreachable!(),
        }
    }

    // Ensure single trailing newline (but keep empty input empty).
    while out.ends_with("\n\n") {
        out.pop();
    }
    if out.chars().all(|c| c == '\n') && source.trim().is_empty() {
        return String::new();
    }
    if !out.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }

    out
}

// ── Per-line formatters ─────────────────────────────────────────────

/// Format a knot header: `=== name ===` or `=== function name(params) ===`
fn format_knot_header(raw: &str) -> String {
    let trimmed = raw.trim();
    let inner = trimmed.trim_start_matches('=').trim_end_matches('=').trim();

    if inner.is_empty() {
        return "===".to_owned();
    }

    let normalized: String = collapse_whitespace(inner);
    format!("=== {normalized} ===")
}

/// Collapse runs of whitespace into single spaces.
fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_ws {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(c);
            prev_ws = false;
        }
    }
    out
}

/// Format a stitch header: `= name`
fn format_stitch_header(raw: &str) -> String {
    let trimmed = raw.trim();
    let inner = trimmed.trim_start_matches('=').trim_end_matches('=').trim();

    if inner.is_empty() {
        return "=".to_owned();
    }

    let normalized = collapse_whitespace(inner);
    format!("= {normalized}")
}

/// Format a choice line: `{bullets} {rest}`
fn format_choice(raw: &str, depth: u32) -> String {
    let trimmed = raw.trim();

    let mut chars = trimmed.chars().peekable();
    let mut bullet_count = 0u32;
    let mut bullet_char = '*';
    while let Some(&c) = chars.peek() {
        if c == '*' || c == '+' {
            bullet_char = c;
            bullet_count += 1;
            chars.next();
        } else if c.is_whitespace() && bullet_count > 0 {
            // Peek ahead: if the next non-whitespace is another bullet, skip.
            let rest: String = chars.clone().collect();
            let next_non_ws = rest.trim_start().chars().next();
            if next_non_ws == Some('*') || next_non_ws == Some('+') {
                chars.next();
            } else {
                break;
            }
        } else {
            break;
        }
    }

    if bullet_count == 0 {
        bullet_count = depth;
    }

    let rest: String = chars.collect();
    let rest = rest.trim_start();

    let bullets: String = std::iter::repeat_n(bullet_char, bullet_count as usize).collect();

    if rest.is_empty() {
        bullets
    } else {
        format!("{bullets} {rest}")
    }
}

/// Format a gather line: `{dashes} {rest}`
fn format_gather(raw: &str) -> String {
    let trimmed = raw.trim();

    let mut chars = trimmed.chars().peekable();
    let mut dash_count = 0u32;
    while let Some(&c) = chars.peek() {
        if c == '-' {
            dash_count += 1;
            chars.next();
        } else {
            break;
        }
    }

    if dash_count == 0 {
        dash_count = 1;
    }

    let rest: String = chars.collect();
    let rest = rest.trim_start();

    let dashes: String = std::iter::repeat_n('-', dash_count as usize).collect();

    if rest.is_empty() {
        dashes
    } else {
        format!("{dashes} {rest}")
    }
}

/// Format a logic line: `~ {rest}`
fn format_logic(raw: &str) -> String {
    let trimmed = raw.trim();
    let rest = trimmed.strip_prefix('~').unwrap_or(trimmed).trim();

    if rest.is_empty() {
        "~".to_owned()
    } else {
        format!("~ {rest}")
    }
}

// ── T1b `~ { … }` block rendering (docs/t1b-surface-spec.md §2, #573) ──
//
// Indentation-aware reformatting of multi-line logic block internals: block
// body one level in from the `~` line, nested `if`/`while`/`for` bodies one
// further level each, opening brace on its statement's line, closing brace
// on its own line at the parent's depth, one statement per line, comments
// and blank lines preserved in place. This operates purely on the CST — it
// never touches HIR — since indentation here is a syntactic property and
// T1b blocks are dialect-gated before lowering ever sees them.
//
// The nesting step is a fixed 4 spaces per level regardless of the file's
// configured `FormatConfig::indent` (which continues to govern the *outer*
// placement of the `~` line itself, e.g. inside a knot or choice body) — the
// ruled acceptance criteria for #573 specifies "4-space indent per nesting
// level" for block internals as its own convention, distinct from the
// surrounding weave's indent style.

/// One nesting step inside a `~ { … }` block, appended to the block's own
/// outer `base_indent` (which already accounts for knot/choice/gather depth
/// via `FormatConfig::indent`).
fn block_indent(base_indent: &str, level: u32) -> String {
    format!("{base_indent}{}", " ".repeat((level * 4) as usize))
}

/// Join a stream of CST elements into single-line text: real token text is
/// emitted byte-for-byte (so string-literal content is never touched), and
/// any run of `WHITESPACE`/`NEWLINE` trivia between two real tokens collapses
/// to exactly one space. Mid-statement comments (rare — e.g. inside a
/// parenthesized expression) are kept verbatim with a single space on each
/// side. Node boundaries are transparent; only tokens contribute text.
fn join_token_text(elems: impl Iterator<Item = SyntaxElement>) -> String {
    let mut out = String::new();
    let mut pending_space = false;
    for elem in elems {
        let NodeOrToken::Token(tok) = elem else {
            continue;
        };
        match tok.kind() {
            SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => {
                if !out.is_empty() {
                    pending_space = true;
                }
            }
            SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT => {
                if pending_space || !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(tok.text());
                pending_space = true;
            }
            _ => {
                if pending_space {
                    out.push(' ');
                    pending_space = false;
                }
                out.push_str(tok.text());
            }
        }
    }
    out
}

/// Render a single-line block statement (`TEMP_DECL`, `ASSIGNMENT`,
/// `RETURN_STMT`, `BREAK_STMT`, `CONTINUE_STMT`, `EXPR_STMT`, or any other
/// node kind reached defensively) as flat, single-spaced text — every token
/// under `node`, recursively, joined per `join_token_text`.
fn render_flat_stmt_text(node: &SyntaxNode) -> String {
    join_token_text(node.descendants_with_tokens())
}

/// Reconstruct the header expression text of an `IF_STMT`/`WHILE_STMT`/
/// `FOR_STMT` node — everything between its leading keyword token and its
/// body `STMT_BLOCK` (a condition for `if`/`while`, or `name in expr` for
/// `for`) — collapsed to single-spaced flat text via `join_token_text`.
fn header_expr_text(node: &SyntaxNode) -> String {
    let Some(first) = node.children_with_tokens().next() else {
        return String::new();
    };
    let first_end = first.text_range().end();
    let Some(body) = node.children().find(|c| c.kind() == SyntaxKind::STMT_BLOCK) else {
        return String::new();
    };
    let body_start = body.text_range().start();
    join_token_text(
        node.descendants_with_tokens()
            .filter(|e| e.text_range().start() >= first_end && e.text_range().start() < body_start),
    )
}

/// Render a `~ { … }` block's `LOGIC_LINE` node — the header `~ {`, the
/// reindented body, and the closing `}` — as a complete, newline-terminated
/// string. `base_indent` is the outer depth of the `~` line itself.
fn render_logic_block(node: &SyntaxNode, base_indent: &str) -> String {
    let mut out = String::new();
    out.push_str(base_indent);
    out.push_str("~ {\n");
    if let Some(body) = node.children().find(|c| c.kind() == SyntaxKind::STMT_BLOCK) {
        render_stmt_block(&body, base_indent, 1, &mut out);
    }
    out.push_str(base_indent);
    out.push_str("}\n");
    out
}

/// Render every statement, comment, and blank line directly inside a
/// `STMT_BLOCK` at `level` (nesting steps in from `base_indent`). Consecutive
/// blank lines collapse to one, matching the rest of the formatter; a
/// same-line trailing comment (no intervening `NEWLINE` token) stays attached
/// to the line it follows instead of becoming its own line.
fn render_stmt_block(block: &SyntaxNode, base_indent: &str, level: u32, out: &mut String) {
    let indent = block_indent(base_indent, level);
    let mut newline_run: u32 = 0;
    let mut has_emitted = false;

    for child in block.children_with_tokens() {
        match child {
            NodeOrToken::Token(tok) => match tok.kind() {
                SyntaxKind::NEWLINE => newline_run += 1,
                SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT => {
                    let text = tok.text().trim_end();
                    if has_emitted && newline_run == 0 {
                        // Trailing comment on the same physical line as the
                        // previous statement/comment — stays attached to it.
                        if out.ends_with('\n') {
                            out.pop();
                        }
                        out.push(' ');
                        out.push_str(text);
                        out.push('\n');
                    } else {
                        if newline_run >= 2 {
                            out.push('\n');
                        }
                        out.push_str(&indent);
                        out.push_str(text);
                        out.push('\n');
                    }
                    has_emitted = true;
                    newline_run = 0;
                }
                // Braces, whitespace, and anything unrecognized: dropped —
                // indentation is re-rendered from structure.
                _ => {}
            },
            NodeOrToken::Node(n) => {
                if newline_run >= 2 {
                    out.push('\n');
                }
                newline_run = 0;
                render_block_stmt(&n, base_indent, level, out);
                has_emitted = true;
            }
        }
    }

    // A blank line immediately before the closing `}` is preserved too.
    if newline_run >= 2 {
        out.push('\n');
    }
}

/// Dispatch one statement node inside a `STMT_BLOCK`. `if`/`while`/`for`
/// recurse (their bodies are themselves `STMT_BLOCK`s at `level + 1`);
/// everything else is a single-line statement.
fn render_block_stmt(node: &SyntaxNode, base_indent: &str, level: u32, out: &mut String) {
    match node.kind() {
        SyntaxKind::IF_STMT => render_if_stmt(node, base_indent, level, false, out),
        SyntaxKind::WHILE_STMT => render_header_stmt(node, base_indent, level, "while", out),
        SyntaxKind::FOR_STMT => render_header_stmt(node, base_indent, level, "for", out),
        _ => {
            out.push_str(&block_indent(base_indent, level));
            out.push_str(&render_flat_stmt_text(node));
            out.push('\n');
        }
    }
}

/// Render `if cond { … } (else if cond { … })* (else { … })?`. `chained` is
/// `true` for an else-if's nested `IF_STMT` — it continues on the previous
/// line (`} else `) instead of starting a fresh indented line.
fn render_if_stmt(
    node: &SyntaxNode,
    base_indent: &str,
    level: u32,
    chained: bool,
    out: &mut String,
) {
    if !chained {
        out.push_str(&block_indent(base_indent, level));
    }
    out.push_str("if ");
    out.push_str(&header_expr_text(node));
    out.push_str(" {\n");
    if let Some(body) = node.children().find(|c| c.kind() == SyntaxKind::STMT_BLOCK) {
        render_stmt_block(&body, base_indent, level + 1, out);
    }
    out.push_str(&block_indent(base_indent, level));
    out.push('}');

    if let Some(else_clause) = node
        .children()
        .find(|c| c.kind() == SyntaxKind::ELSE_CLAUSE)
    {
        if let Some(nested_if) = else_clause
            .children()
            .find(|c| c.kind() == SyntaxKind::IF_STMT)
        {
            out.push_str(" else ");
            render_if_stmt(&nested_if, base_indent, level, true, out);
            return;
        }
        if let Some(else_body) = else_clause
            .children()
            .find(|c| c.kind() == SyntaxKind::STMT_BLOCK)
        {
            out.push_str(" else {\n");
            render_stmt_block(&else_body, base_indent, level + 1, out);
            out.push_str(&block_indent(base_indent, level));
            out.push('}');
        }
    }
    out.push('\n');
}

/// Render `while cond { … }` or `for name in expr { … }` — the shared shape
/// of a keyword, a header expression, and a `STMT_BLOCK` body.
fn render_header_stmt(
    node: &SyntaxNode,
    base_indent: &str,
    level: u32,
    keyword: &str,
    out: &mut String,
) {
    out.push_str(&block_indent(base_indent, level));
    out.push_str(keyword);
    out.push(' ');
    let header = header_expr_text(node);
    if !header.is_empty() {
        out.push_str(&header);
        out.push(' ');
    }
    out.push_str("{\n");
    if let Some(body) = node.children().find(|c| c.kind() == SyntaxKind::STMT_BLOCK) {
        render_stmt_block(&body, base_indent, level + 1, out);
    }
    out.push_str(&block_indent(base_indent, level));
    out.push_str("}\n");
}

/// Render a STRUCT declaration (TM-4b, docs/typed-mode-spec.md §6). For
/// single-line structs, output as-is; for multiline structs, format like
/// blocks with proper field indentation and trailing commas on each field.
fn render_struct_decl(node: &SyntaxNode, base_indent: &str, config: &FormatConfig) -> String {
    // Collect field declarations to determine multiline status and content.
    let fields: Vec<SyntaxNode> = node
        .children()
        .filter(|c| c.kind() == SyntaxKind::STRUCT_FIELD_DECL)
        .collect();

    // Determine if the struct body spans multiple lines by checking if the
    // first and last field (or the opening and closing braces) are on different lines.
    // Check the source text between `#{` and `}` for newlines.
    let opening_brace = node
        .children_with_tokens()
        .find(|e| e.kind() == SyntaxKind::L_BRACE)
        .map(|e| e.text_range().start());
    let closing_brace = node.children_with_tokens().find_map(|e| {
        if e.kind() == SyntaxKind::R_BRACE {
            Some(e.text_range().start())
        } else {
            None
        }
    });

    let is_multiline =
        if let (Some(open_offset), Some(close_offset)) = (opening_brace, closing_brace) {
            // Check if the range between `{` and `}` contains any field on a different line.
            // We do this by checking if any field's text range starts on a line after the
            // opening brace line. Since we don't have direct access to line numbers in the
            // CST, we use a simpler heuristic: if any field has NEWLINE tokens in the parent's
            // descendants before it, or between it and the next field, it's multiline.
            !fields.is_empty()
                && node
                    .children_with_tokens()
                    .skip_while(|e| e.text_range().end() <= open_offset)
                    .take_while(|e| e.text_range().start() < close_offset)
                    .any(|e| e.kind() == SyntaxKind::NEWLINE)
        } else {
            false
        };

    if !is_multiline {
        // Single-line struct: output as a single line with single space after colon.
        // Collapse all whitespace and format as `STRUCT Name = #{field: type, …}`.
        return format_struct_decl_single_line(node, base_indent);
    }

    // Multiline struct: format like a block.
    let mut out = String::new();
    out.push_str(base_indent);
    out.push_str("STRUCT ");
    out.push_str(&struct_decl_name(node));
    out.push_str(" = #{\n");

    // Field indent is base_indent + one level of the file's configured
    // indent (unlike logic blocks which use hardcoded 4 spaces).
    let field_indent = format!("{base_indent}{}", indent_str(config, 1));
    let (Some(open_offset), Some(close_offset)) = (opening_brace, closing_brace) else {
        out.push_str(base_indent);
        out.push_str("}\n");
        return out;
    };
    render_struct_body_multiline(node, open_offset, close_offset, &field_indent, &mut out);

    // Close the struct.
    out.push_str(base_indent);
    out.push_str("}\n");
    out
}

/// Extract the struct name from a `STRUCT_DECL`'s `IDENTIFIER` child.
fn struct_decl_name(node: &SyntaxNode) -> String {
    node.children()
        .find(|c| c.kind() == SyntaxKind::IDENTIFIER)
        .and_then(|id| {
            id.children_with_tokens().find_map(|e| {
                if let NodeOrToken::Token(tok) = e
                    && tok.kind() == SyntaxKind::IDENT
                {
                    Some(tok.text().to_string())
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Render every field declaration and comment directly inside a multiline
/// `STRUCT_DECL` body, in source order, with trailing commas on each field.
/// Comments are direct children of `STRUCT_DECL` (see
/// `skip_struct_body_trivia` in the parser) rather than living inside any
/// field node, so — mirroring `render_stmt_block` — this walks the node's
/// direct children/tokens between the braces instead of iterating field
/// nodes alone; that preserves leading, interleaved, and same-line trailing
/// comments.
fn render_struct_body_multiline(
    node: &SyntaxNode,
    open_offset: rowan::TextSize,
    close_offset: rowan::TextSize,
    field_indent: &str,
    out: &mut String,
) {
    let mut newline_run: u32 = 0;
    let mut has_emitted = false;
    for elem in node
        .children_with_tokens()
        .skip_while(|e| e.text_range().end() <= open_offset)
        .take_while(|e| e.text_range().start() < close_offset)
    {
        match elem {
            NodeOrToken::Token(tok) => match tok.kind() {
                SyntaxKind::NEWLINE => newline_run += 1,
                SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT => {
                    let text = tok.text().trim_end();
                    if has_emitted && newline_run == 0 {
                        // Trailing comment on the same physical line as the
                        // previous field/comment — stays attached to it.
                        if out.ends_with('\n') {
                            out.pop();
                        }
                        out.push(' ');
                        out.push_str(text);
                        out.push('\n');
                    } else {
                        if newline_run >= 2 {
                            out.push('\n');
                        }
                        out.push_str(field_indent);
                        out.push_str(text);
                        out.push('\n');
                    }
                    has_emitted = true;
                    newline_run = 0;
                }
                _ => {}
            },
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::STRUCT_FIELD_DECL => {
                if newline_run >= 2 {
                    out.push('\n');
                }
                newline_run = 0;
                out.push_str(field_indent);
                let field_text = join_token_text(n.descendants_with_tokens());
                out.push_str(&field_text);
                out.push(',');
                out.push('\n');
                has_emitted = true;
            }
            NodeOrToken::Node(_) => {}
        }
    }
}

/// Format a single-line STRUCT declaration: `STRUCT Name = #{field: type, …}`.
/// Collapses all whitespace and ensures canonical spacing: single space after colon.
fn format_struct_decl_single_line(node: &SyntaxNode, base_indent: &str) -> String {
    let mut out = String::new();
    out.push_str(base_indent);
    out.push_str("STRUCT ");
    out.push_str(&struct_decl_name(node));
    out.push_str(" = #{");

    // Comments are direct children of `STRUCT_DECL` (see
    // `skip_struct_body_trivia` in the parser), not children of any field
    // node — a single-line struct can only contain `BLOCK_COMMENT`s (a
    // `LINE_COMMENT` forces a `NEWLINE`, which would make the struct
    // multiline), but they still need to be preserved rather than dropped.
    let opening_brace = node
        .children_with_tokens()
        .find(|e| e.kind() == SyntaxKind::L_BRACE)
        .map(|e| e.text_range().start());
    let closing_brace = node.children_with_tokens().find_map(|e| {
        if e.kind() == SyntaxKind::R_BRACE {
            Some(e.text_range().start())
        } else {
            None
        }
    });

    if let (Some(open_offset), Some(close_offset)) = (opening_brace, closing_brace) {
        let mut has_field = false;
        let mut has_content = false;
        for elem in node
            .children_with_tokens()
            .skip_while(|e| e.text_range().end() <= open_offset)
            .take_while(|e| e.text_range().start() < close_offset)
        {
            match elem {
                NodeOrToken::Node(n) if n.kind() == SyntaxKind::STRUCT_FIELD_DECL => {
                    if has_field {
                        out.push_str(", ");
                    } else if has_content {
                        out.push(' ');
                    }
                    format_struct_field_for_single_line(&n, &mut out);
                    has_field = true;
                    has_content = true;
                }
                NodeOrToken::Token(tok)
                    if matches!(
                        tok.kind(),
                        SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
                    ) =>
                {
                    if has_content {
                        out.push(' ');
                    }
                    out.push_str(tok.text().trim());
                    has_content = true;
                }
                _ => {}
            }
        }
    }

    out.push_str("}\n");
    out
}

/// Extract and format a `STRUCT_FIELD_DECL` for single-line output: `name: type`.
fn format_struct_field_for_single_line(field: &SyntaxNode, out: &mut String) {
    // Use join_token_text to get the entire field as a single-spaced string,
    // then split on the colon to extract name and type, and reconstruct with
    // canonical spacing: `name: type` with single space after colon.
    let field_text = join_token_text(field.descendants_with_tokens());
    if let Some(colon_pos) = field_text.find(':') {
        let name = field_text[..colon_pos].trim();
        let type_str = field_text[colon_pos + 1..].trim();
        out.push_str(name);
        out.push_str(": ");
        out.push_str(type_str);
    } else {
        // Fallback if no colon found (shouldn't happen for valid input).
        out.push_str(&field_text);
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::panic)]
mod tests {
    use super::*;

    fn fmt(source: &str) -> String {
        format(source, &FormatConfig::default())
    }

    fn fmt_tabs(source: &str) -> String {
        format(
            source,
            &FormatConfig {
                indent: IndentStyle::Tabs,
            },
        )
    }

    #[test]
    fn trailing_whitespace_stripped() {
        let input = "Hello world   \nSecond line\t\n";
        let result = fmt(input);
        for line in result.lines() {
            assert_eq!(line, line.trim_end());
        }
    }

    #[test]
    fn knot_header_normalized() {
        assert_eq!(fmt("===myknot===\n"), "=== myknot ===\n");
        assert_eq!(fmt("===  myknot  ===\n"), "=== myknot ===\n");
        assert_eq!(fmt("=== myknot ===\n"), "=== myknot ===\n");
    }

    #[test]
    fn function_knot_header() {
        let input = "=== function  add(a, b) ===\n~ return a + b\n";
        let result = fmt(input);
        assert!(result.starts_with("=== function add(a, b) ===\n"));
    }

    #[test]
    fn stitch_header_normalized() {
        // Standalone stitch at root level — parser promotes to knot, but the
        // CST node is still STITCH_HEADER, so the formatter uses stitch format.
        assert_eq!(fmt("=  mystitch\n"), "= mystitch\n");
        // Inside a knot, stitch headers are indented.
        let input = "=== myknot ===\n= mystitch\nContent\n";
        let result = fmt(input);
        assert!(result.contains("  = mystitch\n"));
    }

    #[test]
    fn choice_formatting() {
        let input = "*  Hello\n";
        let result = fmt(input);
        assert_eq!(result, "* Hello\n");
    }

    #[test]
    fn gather_formatting() {
        let input = "-  gathered\n";
        let result = fmt(input);
        assert_eq!(result, "- gathered\n");
    }

    #[test]
    fn logic_line_formatting() {
        let input = "~   x = 5\n";
        let result = fmt(input);
        assert_eq!(result, "~ x = 5\n");
    }

    #[test]
    fn blank_lines_collapsed() {
        let input = "Hello\n\n\n\nWorld\n";
        let result = fmt(input);
        assert_eq!(result, "Hello\n\nWorld\n");
    }

    #[test]
    fn blank_before_knot() {
        let input = "Hello\n=== knot ===\n";
        let result = fmt(input);
        assert_eq!(result, "Hello\n\n=== knot ===\n");
    }

    #[test]
    fn single_trailing_newline() {
        let input = "Hello\n\n\n";
        let result = fmt(input);
        assert!(result.ends_with('\n'));
        assert!(!result.ends_with("\n\n"));
    }

    #[test]
    fn declaration_no_indent() {
        let input = "VAR x = 5\nCONST y = 10\n";
        let result = fmt(input);
        assert!(result.contains("VAR x = 5\n"));
        assert!(result.contains("CONST y = 10\n"));
    }

    #[test]
    fn empty_input() {
        assert_eq!(fmt(""), "");
    }

    #[test]
    fn comment_preserved() {
        let input = "// This is a comment\nHello\n";
        let result = fmt(input);
        assert!(result.contains("// This is a comment\n"));
    }

    #[test]
    fn content_trimmed() {
        let input = "  Hello world  \n";
        let result = fmt(input);
        assert_eq!(result, "Hello world\n");
    }

    #[test]
    fn choice_with_brackets() {
        let input = "*  \"What's that?\"[he asked.]\n";
        let result = fmt(input);
        assert_eq!(result, "* \"What's that?\"[he asked.]\n");
    }

    #[test]
    fn sticky_choice() {
        let input = "+  Sticky option\n";
        let result = fmt(input);
        assert_eq!(result, "+ Sticky option\n");
    }

    #[test]
    fn include_declaration() {
        let input = "INCLUDE other.ink\n";
        let result = fmt(input);
        assert_eq!(result, "INCLUDE other.ink\n");
    }

    #[test]
    fn knot_body_indented() {
        let input = "=== myknot ===\nHello from knot\n* A choice\n";
        let result = fmt(input);
        assert_eq!(result, "=== myknot ===\n  Hello from knot\n  * A choice\n");
    }

    #[test]
    fn stitch_in_knot_indented() {
        let input = "=== myknot ===\n= mystitch\nContent here\n";
        let result = fmt(input);
        // Stitch header at depth 1, content at depth 2.
        assert!(result.contains("  = mystitch\n"));
        assert!(result.contains("    Content here\n"));
    }

    #[test]
    fn choice_content_indented_in_knot() {
        let input = "=== myknot ===\n* Choice\n  After choice\n";
        let result = fmt(input);
        // Choice at depth 1 (knot body), content after choice at depth 2.
        assert_eq!(result, "=== myknot ===\n  * Choice\n    After choice\n");
    }

    #[test]
    fn idempotent() {
        let input =
            "=== knot ===\n\n  Hello world\n\n  * Choice one\n  * Choice two\n\n  - Gathered\n";
        let first = fmt(input);
        let second = fmt(&first);
        assert_eq!(first, second, "formatting should be idempotent");
    }

    // ── TM-2 inline type annotations (docs/typed-mode-spec.md §3) ───────
    //
    // The knot-header/declaration/logic-line renderers are single-line
    // token-collapsing passes over the raw physical-line text (see
    // `format_knot_header`/`format_logic`/`LineKind::Declaration`'s own
    // docs) — a `:` is just another non-whitespace token to them, so
    // annotations format "for free" through the exact same code path
    // exercised for every other knot header/declaration/logic line. These
    // tests pin that down explicitly rather than relying on it implicitly.

    #[test]
    fn param_and_return_type_annotations_normalize_whitespace() {
        assert_eq!(
            fmt("===function heal(hp:int,amount:  int)  :  int===\n~ return hp\n"),
            "=== function heal(hp:int,amount: int) : int ===\n  ~ return hp\n"
        );
    }

    #[test]
    fn var_type_annotation_formats_verbatim_modulo_trailing_whitespace() {
        assert_eq!(fmt("VAR gold: int = 100   \n"), "VAR gold: int = 100\n");
    }

    #[test]
    fn const_type_annotation_formats_verbatim_modulo_trailing_whitespace() {
        // #641: CONST mirrors VAR — same single-line declaration renderer,
        // no dedicated formatting code.
        assert_eq!(
            fmt("CONST speed: float = 0.5   \n"),
            "CONST speed: float = 0.5\n"
        );
    }

    #[test]
    fn temp_ascription_normalizes_whitespace_like_any_other_logic_line() {
        assert_eq!(
            fmt("=== knot ===\n~   temp   name:string=who\n"),
            "=== knot ===\n  ~ temp   name:string=who\n"
        );
    }

    #[test]
    fn type_annotations_are_idempotent() {
        for input in [
            "=== function heal(hp: int, amount: int): int ===\n~ return hp\n",
            "VAR gold: int = 100\n",
            "=== knot ===\n~ temp name: string = who\n",
            "VAR w: list<Weathers> = 0\nVAR m: map<string, int> = 0\n",
            "VAR cb: fn(int, int): bool = 0\n",
            "CONST speed: float = 0.5\n",
        ] {
            let first = fmt(input);
            let second = fmt(&first);
            assert_eq!(
                first, second,
                "type-annotated formatting should be idempotent for {input:?}"
            );
        }
    }

    // ── T1b `~ { … }` blocks: indentation-aware reformatting ────────────
    // (docs/t1b-surface-spec.md §2, ruled acceptance criteria on #573)

    #[test]
    fn block_at_root_reindents_nesting() {
        // Flat, unindented input — the formatter must reindent it, not
        // pass it through (that was the superseded T1b-1 placeholder, #569).
        let input = "~ {\ntemp x = 0\nif x > 0 {\nx = x - 1\n}\n}\n";
        let expected = "~ {\n    temp x = 0\n    if x > 0 {\n        x = x - 1\n    }\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_with_while_and_for_reindents_nesting() {
        let input =
            "~ {\nwhile x > 0 {\nx = x - 1\n}\nfor item in list {\ntotal = total + item\n}\n}\n";
        let expected = "~ {\n    while x > 0 {\n        x = x - 1\n    }\n    for item in list {\n        total = total + item\n    }\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_collapses_messy_spacing_and_reindents() {
        // Ragged original indentation/spacing is normalized: 4 spaces per
        // nesting level, single space between tokens — but token content
        // (identifiers, literals) is never altered.
        let input = "~ {\n    temp x   =   0  \nif x > 0 {\n\tx = x - 1\n}\n}\n";
        let expected = "~ {\n    temp x = 0\n    if x > 0 {\n        x = x - 1\n    }\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_preserves_string_literal_internal_spacing() {
        // `join_token_text` must only collapse whitespace *between* tokens —
        // never characters inside a STRING_TEXT token's own content.
        let input = "~ {\ntemp msg = \"hello   world\"\n}\n";
        let expected = "~ {\n    temp msg = \"hello   world\"\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_inside_knot_indents_relative_to_knot() {
        // The `~ {` line's own depth still comes from the surrounding
        // structure (knot body = depth 1, 2-space `FormatConfig::default()`
        // step) — the block's *internal* nesting is a separate, fixed
        // 4-space step layered on top of that outer indent.
        let input =
            "=== start ===\nContent\n~ {\ntemp x = 0\nif x > 0 {\nx = x - 1\n}\n}\nMore content\n";
        let expected = "=== start ===\n  Content\n  ~ {\n      temp x = 0\n      if x > 0 {\n          x = x - 1\n      }\n  }\n  More content\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn single_line_logic_line_still_reformatted() {
        // Only the T1b multi-line block form goes through the block
        // renderer — ordinary `~` logic lines keep normal behavior.
        assert_eq!(fmt("~   x = 5\n"), "~ x = 5\n");
    }

    #[test]
    fn block_does_not_disturb_surrounding_lines() {
        let input = "Before\n~ {\ntemp x = 0\n}\nAfter\n";
        let expected = "Before\n~ {\n    temp x = 0\n}\nAfter\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_one_statement_per_line_even_when_source_is_compact() {
        // "One statement per line" is a hard rule — even a single-physical-
        // line block body gets expanded to one statement per rendered line.
        let input = "~ {\ntemp x = 0\nx = x + 1\nx = x + 1\n}\n";
        let expected = "~ {\n    temp x = 0\n    x = x + 1\n    x = x + 1\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_else_if_chain_braces_stay_on_statement_line() {
        let input = "~ {\nif a {\nx = 1\n} else if b {\nx = 2\n} else {\nx = 3\n}\n}\n";
        let expected = "~ {\n    if a {\n        x = 1\n    } else if b {\n        x = 2\n    } else {\n        x = 3\n    }\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_leading_comment_attaches_to_following_statement_depth() {
        let input = "~ {\n// explain x\ntemp x = 0\nif x > 0 {\n// explain the decrement\nx = x - 1\n}\n}\n";
        let expected = "~ {\n    // explain x\n    temp x = 0\n    if x > 0 {\n        // explain the decrement\n        x = x - 1\n    }\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_trailing_comment_stays_on_its_statement_line() {
        let input = "~ {\ntemp x = 0 // starts at zero\nx = x + 1\n}\n";
        let expected = "~ {\n    temp x = 0 // starts at zero\n    x = x + 1\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_trailing_comment_before_closing_brace() {
        // A comment with nothing following it in the block stays at the
        // block's own depth (there is no "following statement" to attach
        // to), not the parent's depth of the closing `}`.
        let input = "~ {\ntemp x = 0\n// done\n}\n";
        let expected = "~ {\n    temp x = 0\n    // done\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_blank_lines_preserved_between_statements() {
        let input = "~ {\ntemp x = 0\n\ntemp y = 1\n}\n";
        let expected = "~ {\n    temp x = 0\n\n    temp y = 1\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_multiple_consecutive_blank_lines_collapse_to_one() {
        let input = "~ {\ntemp x = 0\n\n\n\ntemp y = 1\n}\n";
        let expected = "~ {\n    temp x = 0\n\n    temp y = 1\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_blank_line_before_closing_brace_preserved() {
        let input = "~ {\ntemp x = 0\n\n}\n";
        let expected = "~ {\n    temp x = 0\n\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_idempotent_after_reindenting_messy_input() {
        let input = "~ {\n  temp x=0\n  if x>0{\nx = x - 1\n     }\n\n\n  temp y = 1\n}\n";
        let first = fmt(input);
        let second = fmt(&first);
        assert_eq!(first, second, "block formatting should be idempotent");
    }

    #[test]
    fn block_break_continue_lossless() {
        let input = "~ {\ntemp i = 0\nwhile true {\ni = i + 1\nif i > 10 {\nbreak\n}\nif i mod 2 == 0 {\ncontinue\n}\n}\n}\n";
        let expected = "~ {\n    temp i = 0\n    while true {\n        i = i + 1\n        if i > 10 {\n            break\n        }\n        if i mod 2 == 0 {\n            continue\n        }\n    }\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_parse_error_indented_first_line_keeps_indent() {
        // A knot-nested (indented) malformed block must stay verbatim
        // INCLUDING the first line's leading indentation — the span anchors
        // to the physical line start, not the `~` token offset.
        let input =
            "=== knot ===\n  ~ {\n      temp y = 1\n      if y > 0 // note\n      { y = 2 }\n  }\n";
        let out = fmt(input);
        assert!(
            out.contains("\n  ~ {\n"),
            "first line of the verbatim block must keep its leading indent, got:\n{out}"
        );
        assert_eq!(fmt(&out), out, "verbatim bail-out must stay idempotent");
    }

    fn tier1_brink_fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/tier1-brink")
            .join(name)
            .join("story.ink");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
    }

    #[test]
    fn tier1_brink_fixtures_already_formatted_round_trip_unchanged() {
        // These fixtures are already hand-written in the target style (4
        // spaces per block nesting level) — formatting them must be a no-op,
        // and re-formatting the result must be idempotent.
        for name in [
            "if-else-chain",
            "while-loop",
            "for-in-array",
            "break-continue",
            "stdlib-mutator-nested-lvalue",
            "nested-index-assignment",
        ] {
            let source = tier1_brink_fixture(name);
            let first = fmt(&source);
            assert_eq!(first, source, "fixture {name} should round-trip unchanged");
            let second = fmt(&first);
            assert_eq!(first, second, "fixture {name} should format idempotently");
        }
    }

    #[test]
    fn tier1_brink_fixtures_idempotent_from_deindented_input() {
        // Strip the fixtures' own indentation and confirm the formatter
        // still converges to a fixed point (and does so in one pass).
        for name in [
            "if-else-chain",
            "while-loop",
            "for-in-array",
            "break-continue",
        ] {
            let source = tier1_brink_fixture(name);
            let stripped: String = source
                .lines()
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");
            let stripped = format!("{stripped}\n");
            let first = fmt(&stripped);
            let second = fmt(&first);
            assert_eq!(
                first, second,
                "fixture {name} should converge to a fixed point"
            );
        }
    }

    // ── #603: parse errors inside `~ { … }` blocks bail to verbatim ─────
    // `render_logic_block` assumes a well-formed CST subtree; mid-edit or
    // otherwise malformed blocks must instead pass through byte-for-byte
    // (the pre-#602 `~ { … }` behavior) rather than being corrupted.

    #[test]
    fn block_parse_error_comment_before_brace_stays_verbatim() {
        // Repro (a): a trailing `//` comment between the `if` condition and
        // its opening `{` produces a parse error (the grammar treats the
        // real `{` on the next line as an unexpected token, wrapping it in
        // an `ERROR` node) — `header_expr_text` used to inline the comment
        // right before the ` {`, commenting the brace itself out. Verbatim
        // pass-through must leave the source untouched.
        let input = "~ {\nif x>0 // note\n{\nx = 1\n}\n}\n";
        assert_eq!(
            fmt(input),
            input,
            "malformed block must pass through verbatim"
        );
    }

    #[test]
    fn block_parse_error_multiline_call_stays_verbatim_and_idempotent() {
        // Repro (b): a multi-line call missing a comma is a parse error
        // (ERROR node wraps the unexpected token) — the old code injected
        // spurious blank lines and wasn't idempotent.
        let input = "~ {\nfoo(\n  1,\n  2\n\nbar()\n}\n";
        let first = fmt(input);
        assert_eq!(first, input, "malformed block must pass through verbatim");
        let second = fmt(&first);
        assert_eq!(first, second, "verbatim pass-through must be idempotent");
    }

    #[test]
    fn block_parse_error_lone_else_stays_verbatim() {
        // Repro (c): a lone `else` with no preceding `if {` on the same
        // construct is a parse error (ERROR node wraps the stray `else`
        // keyword) — the old code mangled it into a bare statement line
        // with mismatched braces.
        let input = "~ {\nif x {\nelse\n}\n}\n";
        assert_eq!(
            fmt(input),
            input,
            "malformed block must pass through verbatim"
        );
    }

    #[test]
    fn block_parse_error_missing_closing_brace_stays_verbatim() {
        // A missing expected token (here: the block's own closing `}`) is
        // recorded as a `ParseError` with a zero-length range at EOF but
        // does *not* insert an `ERROR` CST node — `subtree_has_parse_error`
        // must catch this via `Parse::errors()`, not by scanning for `ERROR`
        // nodes alone.
        let input = "~ {\ntemp x = 0\n";
        assert_eq!(
            fmt(input),
            input,
            "malformed block must pass through verbatim"
        );
    }

    #[test]
    fn block_well_formed_still_reindents_alongside_malformed_sibling() {
        // A parse error in one `~ { … }` block must not disable reindenting
        // for a well-formed block elsewhere in the same file.
        let input = "~ {\nif x {\nelse\n}\n}\nContent\n~ {\ntemp y   =   1\n}\n";
        let expected = "~ {\nif x {\nelse\n}\n}\nContent\n~ {\n    temp y = 1\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn tabs_indent_knot() {
        let input = "=== myknot ===\nContent\n";
        let result = fmt_tabs(input);
        assert_eq!(result, "=== myknot ===\n\tContent\n");
    }

    #[test]
    fn intercept_start_knot() {
        // Lines 74-103 of TheIntercept.ink — exercises knot body indentation,
        // gathers, choices at multiple depths, content in choice bodies, logic
        // lines, diverts, comments, and blank line handling.
        let input = "\
=== start === \n\
\n\
//  Intro\n\
\t- \tThey are keeping me waiting. \n\
\t\t*\tHut 14[]. The door was locked after I sat down. \n\
\t\tI don't even have a pen to do any work. There's a copy of the morning's intercept in my pocket, but staring at the jumbled letters will only drive me mad. \n\
\t\tI am not a machine, whatever they say about me.\n\
\n\
\t- (opts)\n\
\t\t{|I rattle my fingers on the field table.|}\n\
 \t\t* \t(think) [Think] \n\
 \t\t\tThey suspect me to be a traitor. They think I stole the component from the calculating machine. They will be searching my bunk and cases. \n\
\t\t\tWhen they don't find it, {plan:then} they'll come back and demand I talk. \n\
\t\t\t-> opts\n\
 \t\t*\t(plan) [Plan]\n\
 \t\t\t{not think:What I am is|I am} a problem\u{2014}solver. Good with figures, quick with crosswords, excellent at chess. \n\
 \t\t\tBut in this scenario \u{2014} in this trap \u{2014} what is the winning play?\n\
 \t\t\t* * \t(cooperate) [Co\u{2014}operate] \n\
\t \t\t\t\tI must co\u{2014}operate. My credibility is my main asset. To contradict myself, or another source, would be fatal. \n\
\t \t\t\t\tI must simply hope they do not ask the questions I do not want to answer.\n\
\t\t \t\t\t~ lower(forceful)\n\
\t \t\t* * \t[Dissemble] \n\
\t\t \t\t\tMisinformation, then. Just as the war in Europe is one of plans and interceptions, not planes and bombs. \n\
\t\t \t\t\tMy best hope is a story they prefer to the truth. \n\
\t\t \t\t\t~ raise(forceful)\n\
\t \t\t* * \t(delay) [Divert] \n\
\t\t \t\t\tAvoidance and delay. The military machine never fights on a single front. If I move slowly enough, things will resolve themselves some other way, my reputation intact.\n\
\t\t \t\t\t~ raise(evasive)\n\
\t\t*\t[Wait]\t\t\n\
\t- \t-> waited\n";

        // NOTE: The first gather `- They are keeping me waiting.` and its
        // following `* Hut 14[]` choice are siblings in the HIR (not parent-
        // child), so the choice is at knot-body depth (1) rather than inside
        // the gather body (depth 2). The `- (opts)` continuation gather
        // correctly indents its body content because the HIR models it as a
        // ChoiceSet continuation block.
        let i1 = "  ";
        let i2 = "    ";
        let i3 = "      ";
        let i4 = "        ";
        let expected = [
            "=== start ===",
            "",
            &format!("{i1}//  Intro"),
            &format!("{i1}- They are keeping me waiting."),
            &format!("{i1}* Hut 14[]. The door was locked after I sat down."),
            &format!("{i2}I don't even have a pen to do any work. There's a copy of the morning's intercept in my pocket, but staring at the jumbled letters will only drive me mad."),
            &format!("{i2}I am not a machine, whatever they say about me."),
            "",
            &format!("{i1}- (opts)"),
            &format!("{i2}{{|I rattle my fingers on the field table.|}}"),
            &format!("{i2}* (think) [Think]"),
            &format!("{i3}They suspect me to be a traitor. They think I stole the component from the calculating machine. They will be searching my bunk and cases."),
            &format!("{i3}When they don't find it, {{plan:then}} they'll come back and demand I talk."),
            &format!("{i3}-> opts"),
            &format!("{i2}* (plan) [Plan]"),
            &format!("{i3}{{not think:What I am is|I am}} a problem\u{2014}solver. Good with figures, quick with crosswords, excellent at chess."),
            &format!("{i3}But in this scenario \u{2014} in this trap \u{2014} what is the winning play?"),
            &format!("{i3}** (cooperate) [Co\u{2014}operate]"),
            &format!("{i4}I must co\u{2014}operate. My credibility is my main asset. To contradict myself, or another source, would be fatal."),
            &format!("{i4}I must simply hope they do not ask the questions I do not want to answer."),
            &format!("{i4}~ lower(forceful)"),
            &format!("{i3}** [Dissemble]"),
            &format!("{i4}Misinformation, then. Just as the war in Europe is one of plans and interceptions, not planes and bombs."),
            &format!("{i4}My best hope is a story they prefer to the truth."),
            &format!("{i4}~ raise(forceful)"),
            &format!("{i3}** (delay) [Divert]"),
            &format!("{i4}Avoidance and delay. The military machine never fights on a single front. If I move slowly enough, things will resolve themselves some other way, my reputation intact."),
            &format!("{i4}~ raise(evasive)"),
            &format!("{i2}* [Wait]"),
            &format!("{i1}- -> waited"),
            "",  // trailing newline
        ].join("\n");

        let result = fmt(input);
        assert_eq!(result, expected);
    }

    // ── TM-4b structs: block-style formatting (docs/typed-mode-spec.md §6) ──
    //
    // Single-line structs format to a single line with canonical spacing
    // (`field: type` with single space after colon). Multiline structs
    // format like blocks: proper field indentation + trailing comma on
    // each field. Both formats are idempotent.

    #[test]
    fn struct_decl_single_line_normalizes_spacing() {
        // Input with irregular spacing; should normalize to `field: type` with
        // single space after colon, no trailing comma on single-line form.
        let input = "STRUCT Point = #{x:float,y:  float}\n";
        let expected = "STRUCT Point = #{x: float, y: float}\n";
        let once = fmt(input);
        assert_eq!(
            once, expected,
            "single-line struct should normalize spacing"
        );
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn struct_decl_multiline_formats_with_indentation_and_trailing_commas() {
        // Multiline struct should be formatted with field indentation and
        // trailing commas. Input indentation is normalized.
        let input = "STRUCT Point = #{\nx: float,\ny: float,\n}\n";
        let expected = "STRUCT Point = #{\n  x: float,\n  y: float,\n}\n";
        let once = fmt(input);
        assert_eq!(
            once, expected,
            "multiline struct should format with proper indentation and trailing commas"
        );
        assert_eq!(
            fmt(&once),
            once,
            "formatting twice must be a no-op (idempotent)"
        );
    }

    #[test]
    fn struct_decl_multiline_is_idempotent() {
        let input = "STRUCT Point = #{\n  x: float,\n  y: float,\n}\n";
        let once = fmt(input);
        assert_eq!(
            once, input,
            "properly formatted multiline struct should round-trip"
        );
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn struct_decl_multiline_with_complex_types() {
        // Fields with complex types (arrays, maps, nested generics) should
        // format correctly with proper type text reconstruction.
        let input = "STRUCT Data = #{\nvalues: array<int>,\nmapping: map<string, float>,\n}\n";
        let expected =
            "STRUCT Data = #{\n  values: array<int>,\n  mapping: map<string, float>,\n}\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting is idempotent");
    }

    #[test]
    fn struct_decl_followed_by_ordinary_content_formats_normally() {
        // The `STRUCT_DECL` formats as a unit; everything after it still
        // goes through the ordinary formatter rules (blank line before knot
        // header, body content indented one level).
        let input = "STRUCT Point = #{\nx: float,\ny: float,\n}\n=== main ===\nHello.\n-> DONE\n";
        let expected =
            "STRUCT Point = #{\n  x: float,\n  y: float,\n}\n\n=== main ===\n  Hello.\n  -> DONE\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting is idempotent");
    }

    // `skip_struct_body_trivia` (brink-syntax parser/declaration.rs) bumps
    // `LINE_COMMENT`/`BLOCK_COMMENT` tokens as direct children of
    // `STRUCT_DECL`, not as children of any `STRUCT_FIELD_DECL`. The
    // multiline and single-line renderers both need to walk the node's own
    // children (like `render_stmt_block` does for logic blocks) rather than
    // iterating fields alone, or these comments are silently dropped.

    #[test]
    fn struct_decl_multiline_preserves_trailing_same_line_comment() {
        let input = "STRUCT Point = #{\n    x: float, // the x coord\n    y: float,\n}\n";
        let expected = "STRUCT Point = #{\n  x: float, // the x coord\n  y: float,\n}\n";
        let once = fmt(input);
        assert_eq!(
            once, expected,
            "a same-line trailing comment must stay attached to its field"
        );
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn struct_decl_multiline_preserves_leading_comment() {
        let input = "STRUCT Point = #{\n    // header comment\n    x: float,\n    y: float,\n}\n";
        let expected = "STRUCT Point = #{\n  // header comment\n  x: float,\n  y: float,\n}\n";
        let once = fmt(input);
        assert_eq!(
            once, expected,
            "a standalone comment before the first field must be preserved"
        );
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn struct_decl_multiline_preserves_interleaved_comment() {
        let input = "STRUCT Point = #{\n    x: float,\n    // between fields\n    y: float,\n}\n";
        let expected = "STRUCT Point = #{\n  x: float,\n  // between fields\n  y: float,\n}\n";
        let once = fmt(input);
        assert_eq!(
            once, expected,
            "a standalone comment between fields must be preserved"
        );
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    // NOTE: this test drives `render_struct_decl` directly against the
    // parsed `STRUCT_DECL` node rather than going through `fmt()`/`format()`.
    // `mark_block_comments` (in this file, pre-existing and out of scope for
    // this fix) marks *any* physical line containing a `BLOCK_COMMENT` token
    // anywhere in its subtree as a pure `LineKind::BlockComment` line — that
    // pre-empts `classify_node`'s `STRUCT_DECL` arm before `render_struct_decl`
    // ever runs, for any single-line construct with an inline block comment
    // (not struct-specific). A single-line `STRUCT` can only carry
    // `BLOCK_COMMENT`s (a `LINE_COMMENT` forces a `NEWLINE`, making the
    // struct multiline), so every single-line-with-comment case trips this
    // separate bug via the full `fmt()` pipeline. Flagged separately; this
    // test exercises the renderer fix in isolation instead.
    fn render_struct_decl_only(source: &str) -> String {
        let parsed = brink_syntax::parse(source);
        let node = parsed
            .syntax()
            .children()
            .find(|c| c.kind() == SyntaxKind::STRUCT_DECL)
            .expect("source must parse to a single STRUCT_DECL");
        render_struct_decl(&node, "", &FormatConfig::default())
    }

    #[test]
    fn struct_decl_single_line_preserves_leading_interleaved_and_trailing_comments() {
        let leading = render_struct_decl_only("STRUCT Point = #{/* lead */ x: float, y: float}\n");
        assert_eq!(leading, "STRUCT Point = #{/* lead */ x: float, y: float}\n");

        let interleaved =
            render_struct_decl_only("STRUCT Point = #{x: float, /* mid */ y: float}\n");
        assert_eq!(
            interleaved,
            "STRUCT Point = #{x: float /* mid */, y: float}\n"
        );

        let trailing =
            render_struct_decl_only("STRUCT Point = #{x: float, y: float /* trail */}\n");
        assert_eq!(
            trailing,
            "STRUCT Point = #{x: float, y: float /* trail */}\n"
        );
    }

    #[test]
    fn struct_decl_empty_multiline_collapses_to_single_line() {
        // An empty struct body has no fields, so `is_multiline` (which
        // requires at least one field) is always false — an empty struct
        // written across multiple lines collapses to the canonical
        // single-line empty form. This is intentional: there is no field
        // content to justify a multiline layout.
        let input = "STRUCT Empty = #{\n}\n";
        let expected = "STRUCT Empty = #{}\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }
}
