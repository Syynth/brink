//! Source code formatter for inkle's ink narrative scripting language.
//!
//! Parses the input with `brink_syntax::parse`, lowers to HIR for structural
//! nesting information, then walks the CST to classify each source line and
//! reformats according to consistent rules. HIR provides the correct
//! indentation depth for every source line.

use brink_ir::hir;
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

    let lines = classify_lines(source, &root, &depth_map);
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
        brink_ir::Stmt::ExprStmt(_) | brink_ir::Stmt::EndOfLine => {}
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
    Choice { depth: u32 },
    Gather { depth: u32 },
    Logic,
    Content,
    Tag,
    Blank,
    Declaration,
    Comment,
    BlockComment,
    Other,
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
}

/// Classify every line in the source by walking the CST, using HIR depth map.
fn classify_lines(source: &str, root: &SyntaxNode, depth_map: &[u32]) -> Vec<ClassifiedLine> {
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
            }
        })
        .collect();

    // Mark lines that are inside block comments.
    mark_block_comments(root, &line_starts, &mut lines);

    // Walk CST to classify line kinds (but depth comes from HIR).
    classify_node(root, &line_starts, &mut lines);

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
fn classify_node(node: &SyntaxNode, line_starts: &[usize], lines: &mut [ClassifiedLine]) {
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
                classify_node(&child, line_starts, lines);
            }
            SyntaxKind::GATHER => {
                let depth = gather_depth(&child);
                if line_idx < lines.len() {
                    lines[line_idx].kind = LineKind::Gather { depth };
                }
                classify_node(&child, line_starts, lines);
            }
            SyntaxKind::LOGIC_LINE => {
                if line_idx < lines.len() {
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
                classify_node(&child, line_starts, lines);
            }
            _ => {
                if is_comment_only(&child) && line_idx < lines.len() {
                    lines[line_idx].kind = LineKind::Comment;
                }
                classify_node(&child, line_starts, lines);
            }
        }
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
            LineKind::Blank | LineKind::BlockComment => unreachable!(),
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

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
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
}
