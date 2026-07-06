//! Per-line structural context derived from the HIR.
//!
//! `line_contexts()` returns one `LineContext` per source line, giving the
//! editor authoritative information about element type, weave position,
//! and inline structure — replacing the regex-based `classifyLine` in TS.
//!
//! `line_contexts_with_dialect()` (#368) layers a registered
//! [`brink_ir::ResolvedDialect`] on top: it runs the same base pass, then a
//! dialect classify+chain post-pass exactly mirroring today's TS screenplay
//! post-pass (`element-type.ts`) — classification runs on narrative AND
//! choice-body base lines (preserving depth), but chaining (narrative →
//! dialect-chained kind) runs on top-level AND conditional/sequence-branch
//! narrative only (#413), so cues inside choice bodies classify but never
//! chain, while cues inside conditional/sequence arms both classify AND
//! chain. Blank lines always break a chain. A `~`-sigil logic line always
//! wins over any chain/blank-fill promotion (#413) — see
//! `detect_sigil_logic_lines`.

use brink_ir::{Block, ChoiceSetContext, Content, ContentPart, HirFile, ResolvedDialect, Stmt};
use brink_syntax::SyntaxNode;
use rowan::TextRange;
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
    /// Dialect classification for this line, if a dialect is registered and
    /// this line matched one of its declared kinds (directly, or via a
    /// chain rule). `None` when no dialect is active or the line is plain
    /// structural content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dialect: Option<DialectLineInfo>,
}

/// Dialect-classification result for one line, computed once at
/// classification time (hidden geometry + content region are never
/// re-derived by downstream hot paths).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DialectLineInfo {
    /// The dialect kind (e.g. `"character"`, `"parenthetical"`, `"dialogue"`).
    pub kind: String,
    /// Captured named-group attributes (e.g. `speaker` → `"Alice"`), sorted
    /// by name. For chained lines, this carries the `chain.carry` groups
    /// forward from the run's originating cue (whole-run `data-speaker`).
    pub attrs: Vec<(String, String)>,
    /// Hidden geometry byte spans (full-line-relative), e.g. the `@` and
    /// `:<>` sigils on a character cue. Empty for chain-only (pattern-less)
    /// kinds like `dialogue`.
    pub hidden_spans: Vec<(u32, u32)>,
    /// The editable content byte span (full-line-relative). `None` for
    /// chain-only kinds, where the pattern-less-kind contract applies:
    /// content is the whole trimmed line.
    pub content_span: Option<(u32, u32)>,
    /// The dialect's declared [`brink_ir::ElementNature`] for this kind,
    /// looked up once here (never re-derived downstream) — consumed by
    /// `brink-ide::folding`'s machinery/narrative fold-run computation
    /// (#365).
    pub nature: brink_ir::ElementNature,
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
            dialect: None,
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

    // Conditional/sequence block ranges collected during the walk below —
    // consumed by pass 4b (scaffold + arm-descent) after all statement
    // pointers have had their chance to claim a line.
    let mut cond_ranges: Vec<(TextRange, WeaveElement)> = Vec::new();

    // Root content block
    walk_block(
        &hir.root_content,
        &idx,
        &mut ctx,
        top_level,
        &mut cond_ranges,
    );

    // Knots and stitches
    for knot in &hir.knots {
        let knot_line = idx.line_col(knot.ptr.text_range().start()).0 as usize;
        if knot_line < ctx.len() {
            ctx[knot_line].element = LineElement::KnotHeader;
        }

        walk_block(&knot.body, &idx, &mut ctx, top_level, &mut cond_ranges);

        for stitch in &knot.stitches {
            let stitch_line = idx.line_col(stitch.ptr.text_range().start()).0 as usize;
            if stitch_line < ctx.len() {
                ctx[stitch_line].element = LineElement::StitchHeader;
            }

            walk_block(&stitch.body, &idx, &mut ctx, top_level, &mut cond_ranges);
        }
    }

    // ── Pass 4: detect gather lines from source text ──
    // The HIR only marks gathers via labeled blocks, but a bare `- text`
    // (no parenthesized label) still needs to show as Gather in the editor.
    detect_gathers(source, &mut ctx);

    // ── Pass 4b: conditional/sequence scaffold + arm-descent (#413) ──
    // Branch bodies accumulate their content via raw token buffering (no
    // per-line `ptr`), so lines inside a conditional/sequence arm that the
    // HIR left `Blank` need a text-based pass over the block's own range —
    // the same "HIR under-covers this, patch from source text" idiom as
    // `detect_gathers`/`detect_comments` above.
    apply_conditional_scaffold(source, &idx, &cond_ranges, &mut ctx);

    // ── Pass 5: sigil logic lines win over any earlier promotion (#413) ──
    // A `~`-prefixed logic line (`Stmt::ExprStmt`, `TempDecl`, `Assignment`)
    // is walked by `walk_stmt`, but `ExprStmt` carries no `ptr` at all — it
    // never claims its own line, so it can be swept into a neighboring
    // `Content` node's blank-fill or a conditional-arm promotion. Sigil
    // classification always wins: force `Logic` from source text last, so
    // no earlier pass's structural guess can outrank the literal `~`.
    detect_sigil_logic_lines(source, &mut ctx);

    ctx
}

/// Compute per-line context, then layer a registered dialect's
/// classification on top (#368).
///
/// Mirrors the base [`line_contexts`] pass exactly, then runs a dialect
/// post-pass over the result:
///
/// 1. **Classify pass** — every line whose base `element` is `Narrative`
///    (top-level OR inside a choice body — weave depth and role are
///    preserved) is matched against the dialect's declared elements, in
///    declaration order. A match records `dialect` on that line without
///    touching `element`/`weave` (those stay the interpreter's structural
///    truth; `dialect` is an additional facet).
/// 2. **Chain pass** — runs over lines whose base `element` is `Narrative`
///    at `WeaveElement::TopLevel`, `ConditionalBranch`, or `SequenceBranch`
///    (see [`chain_eligible_weave`]) — i.e. plain top-level narrative or
///    narrative inside a conditional/sequence arm, but NOT choice-body
///    narrative: if the immediately preceding line's dialect kind matches a
///    chain rule's `after` list, this line's dialect is set to the chained
///    kind, carrying forward the rule's `carry` attrs from the run's
///    originating match. Blank lines always break the chain (a `Blank`
///    line has no dialect, so the next line's "previous dialect kind"
///    lookup sees `None` and the chain resets).
///
/// This split reproduces today's TS behavior exactly: a cue written inside
/// a choice body classifies (and keeps its depth) but never chains to
/// dialogue, because the TS chain gate checks `type === NarrativeText`,
/// which choice-body narrative never is (it was already retyped to
/// `ChoiceBody` before the screenplay post-pass runs). A cue inside a
/// conditional/sequence arm, by contrast, both classifies and chains (#413)
/// — conditional-arm narrative is still plain `Narrative` at the interpreter
/// level (unlike `ChoiceBody`, it never gets a separate retyped kind).
pub fn line_contexts_with_dialect(
    hir: &HirFile,
    source: &str,
    root: &SyntaxNode,
    dialect: &ResolvedDialect,
) -> Vec<LineContext> {
    let mut ctx = line_contexts(hir, source, root);
    apply_dialect(source, dialect, &mut ctx);
    ctx
}

/// The dialect post-pass, split out so it can also be exercised directly
/// against a hand-built `Vec<LineContext>` in tests.
fn apply_dialect(source: &str, dialect: &ResolvedDialect, ctx: &mut [LineContext]) {
    let lines: Vec<&str> = source.split('\n').collect();

    // A line with no non-whitespace text is "truly blank" regardless of what
    // the HIR classified it as — a multi-line `Content` node's range can
    // span an interior blank line as `Narrative` (it's part of the same
    // paragraph), but TS's `classifyLine`/screenplay post-pass treats an
    // empty-trimmed line as `Blank` unconditionally. Blank always breaks a
    // chain (spec decision 9), so both passes below must see this, not the
    // HIR's `element`.
    let is_blank = |i: usize| lines.get(i).is_some_and(|l| l.trim().is_empty());

    // ── Classify pass: narrative AND choice-body base lines, depth preserved ──
    for (i, line) in lines.iter().enumerate() {
        if i >= ctx.len() {
            break;
        }
        if ctx[i].element != LineElement::Narrative || is_blank(i) {
            continue;
        }
        let leading_ws = leading_ws_len(line);
        let trimmed = &line[leading_ws as usize..];
        if let Some(m) = dialect.classify(trimmed, leading_ws) {
            let nature = dialect
                .nature_of(&m.kind)
                .unwrap_or(brink_ir::ElementNature::Narrative);
            ctx[i].dialect = Some(DialectLineInfo {
                kind: m.kind,
                attrs: m.attrs,
                hidden_spans: m.hidden_spans,
                content_span: m.content_span,
                nature,
            });
        }
    }

    // ── Chain pass: narrative-only (top-level), preserving carried attrs ──
    // A choice-body narrative line's base `element` is still `Narrative` in
    // `LineContext` (unlike the TS `LineInfo`, which retypes it to
    // `ChoiceBody` before the screenplay pass runs) — the interpreter-owned
    // distinction here is `weave.element`, so the chain gate checks that
    // directly instead of relying on a separate derived type.
    //
    // `carry` groups (e.g. `speaker`) must survive the *whole run*, not just
    // one hop back: a cue → parenthetical → dialogue chain has the speaker
    // attr only on the cue line, since the parenthetical's own capture
    // groups don't include it. `run_carry` tracks the most recent carried
    // values seen anywhere in the current unbroken run and is reset the
    // moment a blank line (or any non-dialect, non-chained line) appears.
    let mut run_carry: Vec<(String, String)> = Vec::new();
    for i in 0..ctx.len() {
        if is_blank(i) {
            run_carry.clear();
            continue;
        }
        // Any dialect-classified line (whether matched directly by the
        // classify pass, or by a chain rule below) can refresh the run's
        // carried values — a rule's `carry` names are looked up against
        // whatever attrs this line actually has; unmatched names simply
        // don't update (so a parenthetical with no `speaker` attr leaves
        // the previously-carried speaker untouched).
        if i > 0
            && ctx[i].element == LineElement::Narrative
            && chain_eligible_weave(ctx[i].weave.element)
            && ctx[i].dialect.is_none()
            && let Some(prev_kind) = ctx[i - 1].dialect.as_ref().map(|d| d.kind.clone())
            && let Some(rule) = dialect.chain_rule_after(&prev_kind)
        {
            let carried: Vec<(String, String)> = rule
                .carry
                .iter()
                .filter_map(|name| run_carry.iter().find(|(k, _)| k == name).cloned())
                .collect();
            let nature = dialect
                .nature_of(&rule.becomes)
                .unwrap_or(brink_ir::ElementNature::Narrative);
            ctx[i].dialect = Some(DialectLineInfo {
                kind: rule.becomes.clone(),
                attrs: carried,
                hidden_spans: Vec::new(),
                content_span: None,
                nature,
            });
        }

        if let Some(d) = &ctx[i].dialect {
            for (k, v) in &d.attrs {
                if let Some(existing) = run_carry.iter_mut().find(|(ek, _)| ek == k) {
                    existing.1.clone_from(v);
                } else {
                    run_carry.push((k.clone(), v.clone()));
                }
            }
        } else {
            // A non-dialect, non-blank line (plain narrative/structural)
            // still breaks the run — only dialect-classified lines keep it
            // alive.
            run_carry.clear();
        }
    }
}

/// Whether the chain pass may run for a line at this weave position.
/// Top-level narrative and conditional/sequence-branch narrative both
/// chain (#413 — dialogue written inside a conditional arm reads the same
/// as top-level dialogue); choice-body narrative does not (spec-mandated:
/// a cue written inside a choice body classifies, keeping its depth, but
/// the following choice-body narrative never chains to dialogue — the TS
/// chain gate checks `type === NarrativeText`, which choice-body narrative
/// never is, since it's already retyped to `ChoiceBody`).
fn chain_eligible_weave(element: WeaveElement) -> bool {
    matches!(
        element,
        WeaveElement::TopLevel | WeaveElement::ConditionalBranch | WeaveElement::SequenceBranch
    )
}

/// Byte length of a line's leading whitespace (spaces only — ink
/// indentation is space-based).
#[expect(clippy::cast_possible_truncation)]
fn leading_ws_len(line: &str) -> u32 {
    (line.len() - line.trim_start_matches(' ').len()) as u32
}

// ── HIR walking ─────────────────────────────────────────────────────

fn walk_block(
    block: &Block,
    idx: &LineIndex,
    ctx: &mut [LineContext],
    weave: WeavePosition,
    cond_ranges: &mut Vec<(TextRange, WeaveElement)>,
) {
    for stmt in &block.stmts {
        walk_stmt(stmt, idx, ctx, weave, cond_ranges);
    }
}

fn walk_stmt(
    stmt: &Stmt,
    idx: &LineIndex,
    ctx: &mut [LineContext],
    weave: WeavePosition,
    cond_ranges: &mut Vec<(TextRange, WeaveElement)>,
) {
    match stmt {
        Stmt::Content(content) => {
            set_content_lines(
                content,
                idx,
                ctx,
                LineElement::Narrative,
                weave,
                cond_ranges,
            );
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
        Stmt::ChoiceSet(cs) => walk_choice_set(cs, idx, ctx, weave, cond_ranges),
        Stmt::LabeledBlock(block) => walk_labeled_block(block, idx, ctx, weave, cond_ranges),
        Stmt::Conditional(cond) => walk_conditional(cond, idx, ctx, weave, cond_ranges),
        Stmt::Sequence(seq) => walk_sequence(seq, idx, ctx, weave, cond_ranges),
        Stmt::ExprStmt(_) | Stmt::EndOfLine => {}
    }
}

/// Walk a multiline `Conditional`'s branches, recording its own range in
/// `cond_ranges` (#413 — consumed by `apply_conditional_scaffold` after the
/// main walk, since branch-body content has no per-line `ptr` of its own).
fn walk_conditional(
    cond: &brink_ir::Conditional,
    idx: &LineIndex,
    ctx: &mut [LineContext],
    weave: WeavePosition,
    cond_ranges: &mut Vec<(TextRange, WeaveElement)>,
) {
    cond_ranges.push((cond.ptr.text_range(), WeaveElement::ConditionalBranch));
    for branch in &cond.branches {
        walk_block(
            &branch.body,
            idx,
            ctx,
            WeavePosition {
                depth: weave.depth,
                element: WeaveElement::ConditionalBranch,
            },
            cond_ranges,
        );
    }
}

/// Walk a multiline `Sequence`'s branches, recording its own range in
/// `cond_ranges` (#413), mirroring [`walk_conditional`].
fn walk_sequence(
    seq: &brink_ir::Sequence,
    idx: &LineIndex,
    ctx: &mut [LineContext],
    weave: WeavePosition,
    cond_ranges: &mut Vec<(TextRange, WeaveElement)>,
) {
    cond_ranges.push((seq.ptr.text_range(), WeaveElement::SequenceBranch));
    for branch in &seq.branches {
        walk_block(
            branch,
            idx,
            ctx,
            WeavePosition {
                depth: weave.depth,
                element: WeaveElement::SequenceBranch,
            },
            cond_ranges,
        );
    }
}

fn walk_choice_set(
    cs: &brink_ir::ChoiceSet,
    idx: &LineIndex,
    ctx: &mut [LineContext],
    weave: WeavePosition,
    cond_ranges: &mut Vec<(TextRange, WeaveElement)>,
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
            cond_ranges,
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
            cond_ranges,
        );

        if let Some(label) = &cs.continuation.label {
            let line = idx.line_col(label.range.start()).0 as usize;
            if line < ctx.len() {
                ctx[line].element = LineElement::Gather;
                ctx[line].weave = WeavePosition {
                    depth,
                    element: WeaveElement::GatherContinuation,
                };
            }
        }
    }
}

fn walk_labeled_block(
    block: &Block,
    idx: &LineIndex,
    ctx: &mut [LineContext],
    weave: WeavePosition,
    cond_ranges: &mut Vec<(TextRange, WeaveElement)>,
) {
    if let Some(label) = &block.label {
        let line = idx.line_col(label.range.start()).0 as usize;
        if line < ctx.len() {
            ctx[line].element = LineElement::Gather;
            ctx[line].weave = weave;
        }
    }
    walk_block(block, idx, ctx, weave, cond_ranges);
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
    cond_ranges: &mut Vec<(TextRange, WeaveElement)>,
) {
    if let Some(ptr) = &content.ptr {
        let range = ptr.text_range();
        let start_line = idx.line_col(range.start()).0 as usize;
        let (end_line_raw, end_col) = idx.line_col(range.end());
        // A `CONTENT_LINE` node's range commonly includes its trailing
        // newline, so the exclusive end offset lands exactly at column 0 of
        // the FOLLOWING physical line. Without this correction, that next
        // line — which may be a completely different statement (e.g. a `~`
        // logic line with no `ptr` of its own) — gets swept into this
        // content's blank-fill promotion below. Only back off when the
        // range actually spans past the start line (end_col == 0 on the
        // start line itself would mean an empty range, which can't happen
        // for a real content node).
        let end_line = if end_col == 0 && end_line_raw as usize > start_line {
            end_line_raw as usize - 1
        } else {
            end_line_raw as usize
        };
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
                cond_ranges.push((cond.ptr.text_range(), WeaveElement::ConditionalBranch));
                for branch in &cond.branches {
                    walk_block(
                        &branch.body,
                        idx,
                        ctx,
                        WeavePosition {
                            depth: weave.depth,
                            element: WeaveElement::ConditionalBranch,
                        },
                        cond_ranges,
                    );
                }
            }
            ContentPart::InlineSequence(seq) => {
                cond_ranges.push((seq.ptr.text_range(), WeaveElement::SequenceBranch));
                for branch in &seq.branches {
                    walk_block(
                        branch,
                        idx,
                        ctx,
                        WeavePosition {
                            depth: weave.depth,
                            element: WeaveElement::SequenceBranch,
                        },
                        cond_ranges,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Detect gather lines from source text.
///
/// Lines starting with `-` (but not `->`) that the HIR didn't already classify
/// as `Gather` get promoted here. This handles bare gathers without labels,
/// where the HIR has no source range to locate the gather line.
fn detect_gathers(source: &str, ctx: &mut [LineContext]) {
    for (i, line) in source.lines().enumerate() {
        if i >= ctx.len() {
            break;
        }
        // Only promote lines the HIR left as Blank or Narrative
        if !matches!(ctx[i].element, LineElement::Blank | LineElement::Narrative) {
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with('-') && !trimmed.starts_with("->") {
            ctx[i].element = LineElement::Gather;
            // Preserve existing weave info if the HIR set it (GatherContinuation),
            // otherwise count the sigils for depth
            if ctx[i].weave.element == WeaveElement::TopLevel {
                let mut depth = 0u32;
                let mut pos = 0;
                let bytes = trimmed.as_bytes();
                while pos < bytes.len() && bytes[pos] == b'-' {
                    depth += 1;
                    pos += 1;
                    while pos < bytes.len() && bytes[pos] == b' ' {
                        pos += 1;
                    }
                }
                ctx[i].weave = WeavePosition {
                    depth,
                    element: WeaveElement::GatherContinuation,
                };
            }
        }
    }
}

/// Conditional/sequence scaffold + arm-descent post-pass (#413).
///
/// `walk_stmt`'s `Stmt::Conditional`/`Stmt::Sequence`/inline-part handling
/// walks each branch's body, but branch-body `Content` is accumulated via
/// raw token buffering (`ContentAccumulator::flush`), which never stamps a
/// `ptr` — so `set_content_lines` has no range to promote arm content with.
/// The block's OWN range (`Conditional.ptr`/`Sequence.ptr`, collected during
/// the walk as `cond_ranges`) is the only source-located anchor available,
/// so this pass re-scans the block's line range from source text and:
///
/// - classifies scaffold lines (the opening `{`/`{cond:` line, a bare `}`
///   closing line, and `- cond:`/`- else:` branch headers) as `Logic` —
///   they are conditional routing, not gathers (a `detect_gathers` bare-`-`
///   line under a still-open conditional range gets corrected back here);
/// - promotes any still-`Blank` line inside the range to `Narrative` with
///   the block's weave element, so the dialect classify/chain pass (which
///   only looks at `Narrative` lines) can see cues/dialogue written inside
///   conditional arms.
///
/// Lines the HIR (or an earlier pass) already classified — divert arms,
/// nested choices, etc. — are left untouched; this only fills gaps.
fn apply_conditional_scaffold(
    source: &str,
    idx: &LineIndex,
    cond_ranges: &[(TextRange, WeaveElement)],
    ctx: &mut [LineContext],
) {
    let lines: Vec<&str> = source.split('\n').collect();

    for (range, weave_element) in cond_ranges {
        let start_line = idx.line_col(range.start()).0 as usize;
        let (end_line_raw, end_col) = idx.line_col(range.end());
        let end_line = if end_col == 0 && end_line_raw as usize > start_line {
            end_line_raw as usize - 1
        } else {
            end_line_raw as usize
        };

        for i in start_line..=end_line {
            if i >= ctx.len() || i >= lines.len() {
                break;
            }
            let trimmed = lines[i].trim();
            if trimmed.is_empty() {
                continue;
            }

            if is_conditional_scaffold_line(trimmed) {
                ctx[i].element = LineElement::Logic;
                ctx[i].weave = WeavePosition {
                    depth: 0,
                    element: WeaveElement::TopLevel,
                };
                continue;
            }

            // Arm content: only fill the gap — never override a line the
            // HIR (or an earlier statement) already placed with confidence.
            if ctx[i].element == LineElement::Blank {
                ctx[i].element = LineElement::Narrative;
                ctx[i].weave = WeavePosition {
                    depth: 0,
                    element: *weave_element,
                };
            }
        }
    }
}

/// Whether a trimmed, non-empty line is conditional/sequence routing
/// scaffold rather than branch content: an opening brace (`{`, possibly
/// followed by the switch expression and `:`), a bare closing brace (`}`,
/// possibly with trailing scaffold after it on the same physical line is
/// still scaffold), or a multiline branch header (`- cond:` / `- else:`,
/// ink's `-` bullet followed eventually by a bare trailing `:`).
fn is_conditional_scaffold_line(trimmed: &str) -> bool {
    if trimmed.starts_with('{') || trimmed == "}" || trimmed.ends_with('}') {
        return true;
    }
    // A multiline branch header is a `-`-bulleted (not `->`) line whose
    // last non-whitespace character is `:` — e.g. `- get_variable(16) == 2:`
    // or `- else:`. This deliberately does NOT match `- else: -> busy`
    // (inline-divert branch shorthand): that line ends in a divert target,
    // not `:`, and the divert itself already classifies via `Stmt::Divert`'s
    // `ptr` — this scaffold check only needs to catch the header itself when
    // it's the JSON pinned repro shape (bare `- cond:` / `- else:` opening a
    // multiline body), and it must not clobber a divert-terminated line that
    // `walk_stmt` already placed correctly.
    if trimmed.starts_with('-') && !trimmed.starts_with("->") && trimmed.ends_with(':') {
        return true;
    }
    false
}

/// Detect `~`-sigil logic lines from source text and force `Logic`,
/// overriding any earlier pass (#413). `Stmt::ExprStmt` (`~ <call-expr>`)
/// carries no `ptr` at all, so `walk_stmt` can never claim its own line —
/// it can inherit a neighboring `Content` node's blank-fill promotion (now
/// fixed for the exact-boundary case) or a conditional arm-descent
/// promotion above. Sigil classification must always win: this pass runs
/// last, after every structural/dialect-adjacent pass, so a literal `~`
/// prefix can never be re-swallowed into a dialogue chain or a conditional
/// arm's narrative classification.
fn detect_sigil_logic_lines(source: &str, ctx: &mut [LineContext]) {
    for (i, line) in source.lines().enumerate() {
        if i >= ctx.len() {
            break;
        }
        if line.trim_start().starts_with('~') {
            ctx[i].element = LineElement::Logic;
        }
    }
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
    fn choice_body_text_classified() {
        let source = "=== start ===\n* Choice one\n  Body text here\n";
        let ctx = make_contexts(source);
        assert_eq!(ctx[2].element, LineElement::Narrative);
        assert_eq!(ctx[2].weave.element, WeaveElement::ChoiceBody);
        assert_eq!(ctx[2].weave.depth, 1);
    }

    #[test]
    fn gather_after_choice_with_label() {
        let source = "=== start ===\n* [Go back]\n- (gather) g\n";
        let ctx = make_contexts(source);
        assert_eq!(ctx[2].element, LineElement::Gather);
        assert_eq!(ctx[2].weave.depth, 1);
        assert_eq!(ctx[2].weave.element, WeaveElement::GatherContinuation);
    }

    #[test]
    fn gather_after_choice_bare() {
        let source = "=== start ===\n* Choice\n- bare gather\n";
        let ctx = make_contexts(source);
        assert_eq!(ctx[2].element, LineElement::Gather);
        assert_eq!(ctx[2].weave.depth, 1);
    }

    #[test]
    fn gather_empty_sigil() {
        let source = "=== start ===\n* Choice\n- \n";
        let ctx = make_contexts(source);
        assert_eq!(ctx[2].element, LineElement::Gather);
    }

    #[test]
    fn choice_body_empty_indent_is_blank() {
        // Just two spaces — no text content. The HIR correctly reports Blank
        // with TopLevel weave. The *editor* post-pass in TS promotes this to
        // ChoiceBody based on the preceding Choice line.
        let source = "=== start ===\n* Choice one\n  \n";
        let ctx = make_contexts(source);
        assert_eq!(ctx[2].element, LineElement::Blank);
        assert_eq!(ctx[2].weave.element, WeaveElement::TopLevel);
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

// ── Dialect integration tests (#368) ───────────────────────────────
//
// Byte-parity evidence: these pin the Rust `line_contexts_with_dialect`
// classification for the at-cue preset against today's hardcoded TS
// screenplay behavior (`element-type.ts`'s post-pass, `screenplay.ts`'s
// `CHAR_SUFFIX_LEN`/`GLUE_LEN`/`characterName()`).
#[cfg(test)]
mod dialect_tests {
    use super::*;
    use brink_ir::{FileId, ResolvedDialect, hir};

    fn make_dialect_contexts(source: &str) -> Vec<LineContext> {
        let parse = brink_syntax::parse(source);
        let file_id = FileId(0);
        let ast = parse.tree();
        let (hir, _, _) = hir::lower(file_id, &ast);
        let dialect = ResolvedDialect::compile(&brink_ir::DialogueDialect::default())
            .expect("at-cue preset compiles");
        line_contexts_with_dialect(&hir, source, &parse.syntax(), &dialect)
    }

    #[test]
    fn character_cue_classifies_with_hidden_geometry() {
        let source = "=== start ===\n@Alice:<>\nHello there.\n";
        let ctx = make_dialect_contexts(source);
        let d = ctx[1].dialect.as_ref().expect("cue classified");
        assert_eq!(d.kind, "character");
        assert_eq!(d.attrs, vec![("speaker".to_owned(), "Alice".to_owned())]);
        // '@' hidden at (0,1), ':<>' hidden at (6,9) — matches
        // screenplay.ts's CHAR_SUFFIX_LEN = 3 exactly.
        assert_eq!(d.hidden_spans, vec![(0, 1), (6, 9)]);
        assert_eq!(d.content_span, Some((1, 6)));
    }

    #[test]
    fn narrative_after_cue_chains_to_dialogue_and_carries_speaker() {
        let source = "=== start ===\n@Alice:<>\nHello there.\n";
        let ctx = make_dialect_contexts(source);
        let d = ctx[2].dialect.as_ref().expect("chained to dialogue");
        assert_eq!(d.kind, "dialogue");
        assert_eq!(d.attrs, vec![("speaker".to_owned(), "Alice".to_owned())]);
    }

    #[test]
    fn parenthetical_between_cue_and_dialogue_keeps_chain_alive() {
        let source = "=== start ===\n@Alice:<>\n(warmly)<>\nHello there.\n";
        let ctx = make_dialect_contexts(source);
        assert_eq!(
            ctx[2].dialect.as_ref().expect("parenthetical").kind,
            "parenthetical"
        );
        let dialogue = ctx[3].dialect.as_ref().expect("chained");
        assert_eq!(dialogue.kind, "dialogue");
        // carried speaker traces back through the parenthetical link.
        assert_eq!(
            dialogue.attrs,
            vec![("speaker".to_owned(), "Alice".to_owned())]
        );
    }

    #[test]
    fn blank_line_breaks_the_chain() {
        let source = "=== start ===\n@Alice:<>\n\nHello there.\n";
        let ctx = make_dialect_contexts(source);
        // The blank line itself never gets a dialect classification.
        assert!(ctx[2].dialect.is_none());
        // Narrative after the blank does NOT chain to dialogue (spec
        // decision 9: blank always breaks — this holds even though the HIR
        // may still report the blank line's `element` as `Narrative` when
        // it's part of the same multi-line `Content` node's span; the
        // dialect pass treats an empty-trimmed line as blank regardless).
        assert!(ctx[3].dialect.is_none());
        assert_eq!(ctx[3].element, LineElement::Narrative);
    }

    #[test]
    fn cue_inside_choice_body_classifies_but_does_not_chain() {
        // A cue written inside a choice body classifies (depth preserved)
        // but the following narrative — also inside the choice body — must
        // NOT chain to dialogue. This is the P2-critique-mandated
        // classify-vs-chain eligibility split (spec deliverable 3).
        let source = "=== start ===\n* Choice\n  @Alice:<>\n  Hello there.\n";
        let ctx = make_dialect_contexts(source);
        let cue = ctx[2]
            .dialect
            .as_ref()
            .expect("cue classified in choice body");
        assert_eq!(cue.kind, "character");
        assert_eq!(ctx[2].weave.element, WeaveElement::ChoiceBody);
        assert_eq!(ctx[2].weave.depth, 1, "depth preserved inside choice body");

        // The following line is still plain Narrative/ChoiceBody — no chain.
        assert!(
            ctx[3].dialect.is_none(),
            "choice-body narrative must not chain to dialogue"
        );
        assert_eq!(ctx[3].weave.element, WeaveElement::ChoiceBody);
    }

    #[test]
    fn plain_narrative_prose_does_not_classify() {
        let source = "=== start ===\nJust some narrative text.\n";
        let ctx = make_dialect_contexts(source);
        assert!(ctx[1].dialect.is_none());
    }

    #[test]
    fn negative_fixture_channel_prose_is_not_a_cue() {
        // Spec negative fixture: '@channel: hello' prose must NOT classify
        // as a character cue (no ':<>' terminator).
        let source = "=== start ===\n@channel: hello\n";
        let ctx = make_dialect_contexts(source);
        assert!(ctx[1].dialect.is_none());
    }

    #[test]
    fn no_dialect_registered_means_no_classification() {
        // The base `line_contexts` path (no dialect) never populates
        // `dialect` — it stays an opt-in facet.
        let source = "=== start ===\n@Alice:<>\n";
        let parse = brink_syntax::parse(source);
        let file_id = FileId(0);
        let ast = parse.tree();
        let (hir, _, _) = hir::lower(file_id, &ast);
        let ctx = line_contexts(&hir, source, &parse.syntax());
        assert!(ctx[1].dialect.is_none());
    }

    // ── #413 regression tests ──────────────────────────────────────────
    // Two classification gaps that broke screenplay mode (celeris repro,
    // reproduced against published 0.8.0): a `~`-sigil line after dialogue
    // got swallowed into the cue→dialogue chain, and lines in/around
    // conditional blocks got NO classes at all.

    #[test]
    fn sigil_logic_line_after_dialogue_is_not_swallowed_into_chain() {
        // The exact issue-#413 shape: a `~` line immediately follows a
        // chained dialogue line. Sigil classification must win — the line
        // must be `Logic`, never `Narrative`/`dialogue`.
        let source = "=== leave ===\n@Solstice:<>\nAwwww... I have to get going now, Minnie. Sorry!\n~ change_party_member(2, false)\n-> END\n";
        let ctx = make_dialect_contexts(source);
        assert_eq!(ctx[1].dialect.as_ref().expect("cue").kind, "character");
        assert_eq!(
            ctx[2].dialect.as_ref().expect("chained dialogue").kind,
            "dialogue"
        );
        assert_eq!(
            ctx[3].element,
            LineElement::Logic,
            "sigil line must classify as Logic, not be swallowed into the dialogue chain"
        );
        assert!(
            ctx[3].dialect.is_none(),
            "a Logic line must never carry a dialect classification"
        );
    }

    #[test]
    fn if_else_conditional_scaffold_classifies_as_logic() {
        // `{ - cond: -> a  - else: -> b }` — the routing-block braces
        // aren't a divert/content statement themselves; before #413 they
        // were left `Blank`.
        let source =
            "=== start ===\n{\n    - get_variable(16) == 2: -> leave\n    - else: -> busy\n}\n";
        let ctx = make_dialect_contexts(source);
        assert_eq!(ctx[1].element, LineElement::Logic, "opening brace");
        assert_eq!(ctx[2].element, LineElement::Divert, "if-arm divert");
        assert_eq!(ctx[3].element, LineElement::Divert, "else-arm divert");
        assert_eq!(ctx[4].element, LineElement::Logic, "closing brace");
    }

    #[test]
    fn conditional_arm_dialogue_classifies_and_chains() {
        // The issue's `busy` stitch shape: a branchless `{ cond: ... -
        // else: ... }` block whose arms contain cue/dialogue lines. Before
        // #413 every line here was `Blank` with no dialect at all.
        let source = "=== start ===\n{ get_variable(17) >= 1:\n    @Solstice:<>\n    Hello, this is Sols.\n    @Minnie:<>\n    Uhhhh... I have no idea.\n- else:\n    @Solstice:<>\n    Hello?\n}\n-> END\n";
        let ctx = make_dialect_contexts(source);

        assert_eq!(ctx[1].element, LineElement::Logic, "opening scaffold line");

        assert_eq!(ctx[2].weave.element, WeaveElement::ConditionalBranch);
        assert_eq!(ctx[2].dialect.as_ref().expect("cue").kind, "character");
        assert_eq!(
            ctx[3].dialect.as_ref().expect("chained dialogue").kind,
            "dialogue"
        );
        assert_eq!(ctx[4].dialect.as_ref().expect("cue").kind, "character");
        assert_eq!(
            ctx[5].dialect.as_ref().expect("chained dialogue").kind,
            "dialogue"
        );

        assert_eq!(
            ctx[6].element,
            LineElement::Logic,
            "`- else:` is conditional scaffold, not a weave gather"
        );

        assert_eq!(ctx[7].weave.element, WeaveElement::ConditionalBranch);
        assert_eq!(ctx[7].dialect.as_ref().expect("cue").kind, "character");
        assert_eq!(
            ctx[8].dialect.as_ref().expect("chained dialogue").kind,
            "dialogue"
        );

        assert_eq!(ctx[9].element, LineElement::Logic, "closing brace");
        assert_eq!(ctx[10].element, LineElement::Divert);
    }

    #[test]
    fn choice_body_cue_still_does_not_chain_inside_conditional_gate_change() {
        // Regression guard: widening the chain gate to conditional/sequence
        // branches (#413) must not accidentally re-enable chaining for
        // choice-body narrative — that stays off per the pre-existing
        // spec-mandated split (see `cue_inside_choice_body_classifies_but_does_not_chain`).
        let source = "=== start ===\n* Choice\n  @Alice:<>\n  Hello there.\n";
        let ctx = make_dialect_contexts(source);
        assert_eq!(ctx[2].dialect.as_ref().expect("cue").kind, "character");
        assert!(ctx[3].dialect.is_none(), "choice-body chain stays off");
    }
}
