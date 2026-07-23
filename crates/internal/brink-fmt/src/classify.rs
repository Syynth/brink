//! Line classification: walk the CST and tag every physical line with a
//! [`LineKind`], carrying whatever CST node and HIR depth `render()` needs to
//! reformat it.
//!
//! This is the second stage of the [`crate::format`] pipeline — it consumes
//! the depth map built by [`crate::depth`] and produces the [`ClassifiedLine`]
//! stream that rendering consumes.

use brink_syntax::ParseError;
use brink_syntax::SyntaxNode;
use brink_syntax::syntax_kind::SyntaxKind;
use rowan::NodeOrToken;

use crate::depth::{build_line_starts, line_for_offset};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LineKind {
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
pub(crate) struct ClassifiedLine {
    pub(crate) kind: LineKind,
    /// Byte offset of the start of this line in the source.
    pub(crate) start: usize,
    /// Byte offset one past the end of this line (excluding newline).
    pub(crate) end: usize,
    /// Indentation depth from HIR structure.
    pub(crate) depth: u32,
    /// The `LOGIC_LINE` CST node for a [`LineKind::LogicBlock`] or
    /// single-line [`LineKind::Logic`] line, or the `STRUCT_DECL` CST node
    /// for a [`LineKind::StructDecl`] line — `None` for every other kind.
    pub(crate) cst_node: Option<SyntaxNode>,
}

/// Classify every line in the source by walking the CST, using HIR depth map.
pub(crate) fn classify_lines(
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
///
/// A line marked `BlockComment` renders verbatim (`trim_end` only), and
/// `classify_node` skips any construct starting on it. That's right for
/// free-floating comments (banners, multi-line comments, comments the parser
/// split a content line around), but wrong for a comment nested inside a
/// construct whose renderer handles comments itself — marking the line would
/// pre-empt the construct's classification entirely (a single-line
/// `STRUCT Point = #{x: float, /* mid */ y: float}` never reached
/// `render_struct_decl`; a comment on a `#{` or `~ {` opening line dedented
/// the whole body). [`comment_handled_by_construct`] decides which comments
/// to leave to their construct.
fn mark_block_comments(root: &SyntaxNode, line_starts: &[usize], lines: &mut [ClassifiedLine]) {
    for token in root.descendants_with_tokens() {
        if let NodeOrToken::Token(tok) = token
            && tok.kind() == SyntaxKind::BLOCK_COMMENT
        {
            let start_line = line_for_offset(line_starts, tok.text_range().start().into());
            let end_line = line_for_offset(line_starts, tok.text_range().end().into());
            if start_line == end_line && comment_handled_by_construct(&tok) {
                continue;
            }
            let last = end_line.min(lines.len() - 1);
            for line in &mut lines[start_line..=last] {
                line.kind = LineKind::BlockComment;
            }
        }
    }
}

/// Is this single-line `BLOCK_COMMENT` token inside a region that a
/// construct's own renderer preserves? If so, `mark_block_comments` must not
/// mark its line — the construct's `classify_node` arm takes over and its
/// renderer emits the comment itself. The regions are deliberately exact:
/// anywhere the answer is wrongly `true`, the renderer silently drops the
/// comment (it only walks the region listed here); anywhere wrongly `false`,
/// the line just stays verbatim (safe, the pre-fix behavior).
///
/// Multi-line comments never qualify (the caller checks): the `Logic` arm
/// classifies only the construct's first physical line, so a comment
/// spanning further lines must keep the verbatim treatment.
fn comment_handled_by_construct(tok: &brink_syntax::SyntaxToken) -> bool {
    let Some(parent) = tok.parent() else {
        return false;
    };
    for anc in parent.ancestors() {
        match anc.kind() {
            // Inside a `~ { … }` body: `render_stmt_block` emits comments
            // among statements and `join_token_text` (statement/header text)
            // keeps mid-statement comments — everything is covered.
            //
            // Reaching LOGIC_LINE instead means the comment is a direct child
            // of the logic line, outside any STMT_BLOCK (an in-block comment
            // matched STMT_BLOCK first). Both forms preserve those:
            // `format_logic` re-emits the whole raw `~ expr` line, comment
            // included, and `render_logic_block` emits a leading (`~ /* c */
            // {`) or trailing (`} /* c */`) direct-child comment on the
            // block's header/closing line.
            SyntaxKind::STMT_BLOCK | SyntaxKind::LOGIC_LINE => return true,
            // `render_struct_decl` / `format_struct_decl_single_line` walk
            // only the region strictly between `#{` and `}` (comments there
            // are direct children of `STRUCT_DECL`, see the parser's
            // `skip_struct_body_trivia`). A comment outside the braces
            // (e.g. `STRUCT Point /* c */ = #{…}`) would be dropped.
            SyntaxKind::STRUCT_DECL => {
                let open = anc
                    .children_with_tokens()
                    .find(|e| e.kind() == SyntaxKind::L_BRACE)
                    .map(|e| e.text_range().end());
                let close = anc
                    .children_with_tokens()
                    .find(|e| e.kind() == SyntaxKind::R_BRACE)
                    .map(|e| e.text_range().start());
                return match (open, close) {
                    (Some(open), Some(close)) => {
                        tok.text_range().start() >= open && tok.text_range().end() <= close
                    }
                    _ => false,
                };
            }
            _ => {}
        }
    }
    false
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
                    // Single-line `~ expr` form (issue #858): carry the
                    // `LOGIC_LINE` node itself so `render()` can retokenize
                    // the statement through `join_token_text` instead of
                    // reformatting the raw source text, matching the
                    // normalization multi-line `~ { … }` bodies already get
                    // (canonical single-space operators, zero-space
                    // `.`/`[`/`]`, comments preserved).
                    lines[line_idx].cst_node = Some(child.clone());
                }
            }
            SyntaxKind::CONTENT_LINE => {
                if line_idx < lines.len() {
                    lines[line_idx].kind = LineKind::Content;
                }
            }
            // NS-A2: `@[effects(…)]` annotation lines format like tag lines
            // — kept verbatim at their weave depth, never content-reflowed.
            SyntaxKind::TAG_LINE | SyntaxKind::ANNOTATION_LINE => {
                if line_idx < lines.len() {
                    lines[line_idx].kind = LineKind::Tag;
                }
            }
            SyntaxKind::VAR_DECL | SyntaxKind::CONST_DECL | SyntaxKind::LIST_DECL => {
                if line_idx < lines.len() {
                    lines[line_idx].kind = LineKind::Declaration;
                    lines[line_idx].depth = 0;
                    // Carry the CST node so `render()` can retokenize the
                    // declaration through `join_token_text` (#642 fix): that
                    // joiner emits string-literal token text byte-for-byte,
                    // so a colon canonicalized inside a string value like
                    // `VAR msg = "time 12:30"` never mutates the literal —
                    // unlike the character-based `collapse_whitespace` pass
                    // formerly used here, which was string-unaware and
                    // corrupted such literals (removed as dead code, #984:
                    // this arm always sets `cst_node` alongside `kind`, so
                    // `render()` never needs a raw-text fallback for these
                    // three node kinds).
                    lines[line_idx].cst_node = Some(child.clone());
                }
            }
            SyntaxKind::INCLUDE_STMT | SyntaxKind::IMPORT_STMT | SyntaxKind::EXTERNAL_DECL => {
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
