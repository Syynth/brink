//! Per-line structural context — a composition of layered facets (#463).
//!
//! `line_contexts()` returns one `LineContext` per source line, giving the
//! editor authoritative information about element type, weave position,
//! and inline structure — replacing the regex-based `classifyLine` in TS.
//!
//! Since #463 this module owns no HIR walk: it **composes**
//! `docs/editor-hir-overlay-spec.md` §1a's layers —
//!
//! 1. the **trivia facet** ([`crate::trivia`]: comments, block comments,
//!    tag lines — CST/source facts);
//! 2. the **structural view** over the HIR projection
//!    ([`crate::hir_projection::project_hir_structural`]), replayed span by
//!    span in [`apply_structural_view`];
//! 3. source-text patch passes for what the HIR under-covers
//!    (`detect_gathers`, `apply_conditional_scaffold`,
//!    `detect_sigil_logic_lines`);
//! 4. the **dialect facet** (`apply_dialect`), layered last.
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

use brink_ir::{HirFile, ResolvedDialect};
use brink_syntax::SyntaxNode;
use rowan::TextRange;
use serde::Serialize;

use crate::LineIndex;
use crate::hir_projection::{ProjectedSpan, Projection, SpanKind};

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

    // ── Pass 1: apply the trivia facet (comments, block comments, tags) ──
    // Computed standalone (`crate::trivia`) and composed here: comments win
    // over a tag sigil; the structural passes below never reconsider a
    // comment line (no statement can share it), and only a still-`Blank`
    // line keeps a tag classification.
    for (i, t) in crate::trivia::line_trivia(source, root, ctx.len())
        .iter()
        .enumerate()
    {
        if t.comment {
            ctx[i].element = LineElement::Comment;
        } else if t.tag && ctx[i].element == LineElement::Blank {
            ctx[i].element = LineElement::Tag;
        }
        if t.block_comment {
            ctx[i].block_comment = true;
        }
    }

    // ── Pass 3: structural view over the HIR projection ──
    // The structural facts come from `project_hir_structural` (#463): the
    // spans are replayed in emission (walk) order with the hand-rolled
    // walk's exact overwrite discipline — statement spans overwrite,
    // content spans fill only still-`Blank` lines. `cond_ranges` (consumed
    // by pass 4b) falls out of the projection's construct-extent spans.
    let projection = crate::hir_projection::project_hir_structural(hir, source);
    let mut cond_ranges: Vec<(TextRange, WeaveElement)> = Vec::new();
    apply_structural_view(&projection, source, &idx, &mut ctx, &mut cond_ranges);

    // ── Pass 3c: blank lines in a choice body inherit its weave (#478) ──
    // A whitespace-only line following a ChoiceLine/ChoiceBody-weave line is
    // still inside the body for editing purposes — Tab on it must know the
    // depth. The element stays `Blank` (dialect chain-breaking and fold runs
    // key on blankness); only the weave is inherited, chaining through blank
    // runs. Subsumes the TS-side blank-after-choice patch, and covers deeper
    // blank runs that patch missed.
    let src_lines: Vec<&str> = source.split('\n').collect();
    for i in 1..ctx.len() {
        if ctx[i].element != LineElement::Blank
            || !src_lines.get(i).is_none_or(|l| l.trim().is_empty())
        {
            continue;
        }
        let prev = ctx[i - 1].weave;
        if matches!(
            prev.element,
            WeaveElement::ChoiceLine { .. } | WeaveElement::ChoiceBody
        ) {
            ctx[i].weave = WeavePosition {
                depth: prev.depth,
                element: WeaveElement::ChoiceBody,
            };
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

// ── Structural view over the HIR projection (#463) ─────────────────

/// Replay the projection's spans onto per-line element/weave/tags with the
/// old hand-rolled walk's exact overwrite discipline:
///
/// - statement spans (diverts, logic, temp decls) **overwrite** the line —
///   emission order is walk order, so a body statement sharing its choice's
///   physical line wins, exactly as the walk's nested `set_line` did;
/// - content spans **fill** only still-`Blank` lines within their range
///   (with the trailing-newline end-column backoff);
/// - a **continuation** gather label overwrites its line *after* the replay
///   (the walk applied it after walking the continuation body), while a
///   **labeled block**'s label applies in order, so the block's own
///   statements overwrite it — the walk's historical asymmetry, preserved;
/// - reference spans (`Divert`/`VarRef`/`Call`) never classify lines: they
///   also arise from expressions the walk never looked at.
///
/// Weave comes from the containing weave containers (by range containment,
/// innermost = smallest): containment distinguishes narrative that merely
/// *hosts* an inline `{...}` (not inside it — `TopLevel`) from statements
/// genuinely inside a construct's branches.
fn apply_structural_view(
    projection: &Projection,
    source: &str,
    idx: &LineIndex,
    ctx: &mut [LineContext],
    cond_ranges: &mut Vec<(TextRange, WeaveElement)>,
) {
    let containers: Vec<&ProjectedSpan> = projection
        .spans
        .iter()
        .filter(|s| s.handle.is_some())
        .collect();
    let src_lines: Vec<&str> = source.split('\n').collect();

    // Continuation-label overwrites, applied after the replay.
    let mut deferred_gathers: Vec<(usize, WeavePosition)> = Vec::new();
    // The last content/choice span, for attributing Tag spans: a tag marks
    // `has_tags` on its owner's start line (the walk read `content.tags` /
    // `choice.tags` directly), and a tag with no owning span in range —
    // e.g. inside ptr-less inline-branch content — marks nothing.
    let mut tag_anchor: Option<(usize, TextRange)> = None;

    for span in &projection.spans {
        let start_line = idx.line_col(span.range.start()).0 as usize;
        match span.kind {
            SpanKind::VarDecl | SpanKind::ConstDecl | SpanKind::ListDecl => {
                set_element(ctx, start_line, LineElement::VarDecl);
            }
            SpanKind::External => set_element(ctx, start_line, LineElement::External),
            SpanKind::Include => set_element(ctx, start_line, LineElement::Include),
            SpanKind::Knot if span.handle.is_some() => {
                set_element(ctx, start_line, LineElement::KnotHeader);
            }
            SpanKind::Stitch if span.handle.is_some() => {
                set_element(ctx, start_line, LineElement::StitchHeader);
            }
            SpanKind::Choice => {
                if start_line < ctx.len() {
                    ctx[start_line].element = LineElement::Choice;
                    ctx[start_line].weave = WeavePosition {
                        depth: span.weave_depth.unwrap_or(0),
                        element: WeaveElement::ChoiceLine {
                            sticky: span.sticky.unwrap_or(false),
                        },
                    };
                }
                tag_anchor = Some((start_line, span.range));
            }
            // #478: a body statement sharing its choice's physical line
            // (`* [Go] -> hub`) no longer reclassifies it — the line stays
            // Choice so Tab/Enter transitions keep working. The divert's
            // reference span still projects for the overlay.
            SpanKind::DivertStmt | SpanKind::DivertTerminal
                if !on_choice_first_line(&containers, idx, span.range) =>
            {
                set_element_weave(
                    ctx,
                    start_line,
                    LineElement::Divert,
                    derive_weave(&containers, span.range),
                );
            }
            SpanKind::TempDecl | SpanKind::Logic
                if !on_choice_first_line(&containers, idx, span.range) =>
            {
                set_element_weave(
                    ctx,
                    start_line,
                    LineElement::Logic,
                    derive_weave(&containers, span.range),
                );
            }
            SpanKind::Content => {
                fill_content_lines(span.range, idx, ctx, derive_weave(&containers, span.range));
                tag_anchor = Some((start_line, span.range));
            }
            SpanKind::Tag => {
                if let Some((line, range)) = tag_anchor
                    && range.contains_range(span.range)
                    && line < ctx.len()
                {
                    // Historical quirk, preserved bug-for-bug: a tag on the
                    // choice line itself never set `has_tags`. Lowering
                    // leaves `Choice.tags` empty (choice-line tags are
                    // distributed into the slot contents), and the old walk
                    // never visited slot contents — so its
                    // `!choice.tags.is_empty()` check was always false.
                    // Fixing this is a deliberate behavior change to make
                    // separately, not a refactor side effect.
                    let on_choice_line = containing(&containers, SpanKind::Choice, span.range)
                        .map(|c| idx.line_col(c.range.start()).0 as usize)
                        == Some(line);
                    // A tag inside a construct's extent belongs to ptr-less
                    // branch content (`{mood: hi # tag}`) — the old walk's
                    // `content.ptr` gate meant those never set `has_tags`.
                    // Construct spans replay before their content's tags, so
                    // `cond_ranges` is already populated here.
                    let in_construct = cond_ranges
                        .iter()
                        .any(|(r, _)| r.contains_range(span.range));
                    if !on_choice_line && !in_construct {
                        ctx[line].has_tags = true;
                    }
                }
            }
            SpanKind::Label => {
                apply_label_span(span, &containers, &src_lines, idx, &mut deferred_gathers);
            }
            SpanKind::Conditional => {
                cond_ranges.push((span.range, WeaveElement::ConditionalBranch));
            }
            SpanKind::Sequence => {
                cond_ranges.push((span.range, WeaveElement::SequenceBranch));
            }
            // References and expression-level spans never classify lines;
            // container-only kinds contribute via `derive_weave`.
            _ => {}
        }
    }

    for (line, weave) in deferred_gathers {
        if line < ctx.len() {
            ctx[line].element = LineElement::Gather;
            ctx[line].weave = weave;
        }
    }
}

/// Apply a Label span: a choice label (no element), or a gather label —
/// continuation and `LabeledBlock` labels alike (#478): every gather-label
/// line is `Gather` with `GatherContinuation` weave at its sigil depth,
/// applied as a deferred overwrite so same-line statements (`- (g) -> next`)
/// never reclassify it. This deliberately removes the legacy asymmetry where
/// a `LabeledBlock`'s statements overwrote its label (→ `Divert`) while a
/// continuation's didn't (→ `Gather`) — two visually identical lines with
/// different Tab/Enter behavior. Depth is the sigil count (what transitions
/// need to rebuild the prefix), matching `detect_gathers`' rule for bare
/// gathers; a continuation label's producer-stamped `weave_depth` equals its
/// sigil count by construction.
fn apply_label_span(
    span: &ProjectedSpan,
    containers: &[&ProjectedSpan],
    src_lines: &[&str],
    idx: &LineIndex,
    deferred_gathers: &mut Vec<(usize, WeavePosition)>,
) {
    let start_line = idx.line_col(span.range.start()).0 as usize;
    if span.weave_depth.is_none() {
        // Not a continuation label: a choice label never classifies its line.
        let choice_first_line = containing(containers, SpanKind::Choice, span.range)
            .map(|c| idx.line_col(c.range.start()).0 as usize);
        if choice_first_line == Some(start_line) {
            return;
        }
    }
    let depth = span.weave_depth.unwrap_or_else(|| {
        src_lines
            .get(start_line)
            .map_or(0, |l| gather_sigil_depth(l.trim_start()))
    });
    deferred_gathers.push((
        start_line,
        WeavePosition {
            depth,
            element: WeaveElement::GatherContinuation,
        },
    ));
}

/// The gather-sigil depth of a trimmed line (`-` count, `->` excluded) —
/// the same counting `detect_gathers` uses for bare gathers.
fn gather_sigil_depth(trimmed: &str) -> u32 {
    let mut depth = 0u32;
    let bytes = trimmed.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() && bytes[pos] == b'-' {
        if bytes.get(pos + 1) == Some(&b'>') {
            break;
        }
        depth += 1;
        pos += 1;
        while pos < bytes.len() && bytes[pos] == b' ' {
            pos += 1;
        }
    }
    depth
}

/// Whether `range` sits on the first line of its innermost containing Choice
/// container — i.e. on the choice's own physical line (#478: statements
/// there never reclassify the line away from `Choice`).
fn on_choice_first_line(containers: &[&ProjectedSpan], idx: &LineIndex, range: TextRange) -> bool {
    containing(containers, SpanKind::Choice, range)
        .is_some_and(|c| idx.line_col(c.range.start()).0 == idx.line_col(range.start()).0)
}

/// The innermost of `spans`: smallest range, ties resolved to the **last**
/// emitted. Emission order is walk order — an outer container is emitted
/// before the containers nested inside it, so when a branch/gather extent is
/// byte-identical to the single choice that fills it, the choice (inner)
/// wins, exactly as the old walk's lexical threading did.
fn innermost<'a>(spans: impl Iterator<Item = &'a ProjectedSpan>) -> Option<&'a ProjectedSpan> {
    let mut best: Option<&'a ProjectedSpan> = None;
    for s in spans {
        if best.is_none_or(|b| s.range.len() <= b.range.len()) {
            best = Some(s);
        }
    }
    best
}

/// The innermost container of `kind` containing `range`.
fn containing<'a>(
    containers: &[&'a ProjectedSpan],
    kind: SpanKind,
    range: TextRange,
) -> Option<&'a ProjectedSpan> {
    innermost(
        containers
            .iter()
            .copied()
            .filter(|c| c.kind == kind && c.range.contains_range(range)),
    )
}

/// Derive a span's weave position from the containers that contain it.
///
/// Containment (not line coverage) is what reproduces the walk: narrative
/// hosting an inline `{...}` is not *contained by* the construct's branch
/// container (the construct is inside the content), so it stays at the
/// outer weave; a statement inside a branch is contained and takes the
/// branch's role. A statement on a choice's own line reports `ChoiceBody` —
/// the walk classified body statements with the body weave even when they
/// shared the choice's physical line (`* [Go] -> hub`); `ChoiceLine` is set
/// only by the Choice container itself.
fn derive_weave(containers: &[&ProjectedSpan], range: TextRange) -> WeavePosition {
    let inner = innermost(
        containers
            .iter()
            .copied()
            .filter(|c| weave_container(c.kind) && c.range.contains_range(range)),
    );
    let Some(c) = inner else {
        return WeavePosition {
            depth: 0,
            element: WeaveElement::TopLevel,
        };
    };
    match c.kind {
        SpanKind::Choice => WeavePosition {
            depth: c.weave_depth.unwrap_or(0),
            element: WeaveElement::ChoiceBody,
        },
        SpanKind::Gather => WeavePosition {
            depth: c.weave_depth.unwrap_or(0),
            element: WeaveElement::GatherContinuation,
        },
        SpanKind::ConditionalBranch | SpanKind::SequenceBranch => {
            // Branches inherit the surrounding weave depth (the walk passed
            // `weave.depth` through): the nearest enclosing choice/gather's.
            let depth = innermost(containers.iter().copied().filter(|w| {
                matches!(w.kind, SpanKind::Choice | SpanKind::Gather)
                    && w.range.contains_range(range)
            }))
            .and_then(|w| w.weave_depth)
            .unwrap_or(0);
            WeavePosition {
                depth,
                element: if c.kind == SpanKind::ConditionalBranch {
                    WeaveElement::ConditionalBranch
                } else {
                    WeaveElement::SequenceBranch
                },
            }
        }
        _ => WeavePosition {
            depth: 0,
            element: WeaveElement::TopLevel,
        },
    }
}

/// Container kinds that define a weave role (knots/stitches don't).
fn weave_container(kind: SpanKind) -> bool {
    matches!(
        kind,
        SpanKind::Choice
            | SpanKind::Gather
            | SpanKind::ConditionalBranch
            | SpanKind::SequenceBranch
    )
}

fn set_element(ctx: &mut [LineContext], line: usize, element: LineElement) {
    if line < ctx.len() {
        ctx[line].element = element;
    }
}

fn set_element_weave(
    ctx: &mut [LineContext],
    line: usize,
    element: LineElement,
    weave: WeavePosition,
) {
    if line < ctx.len() {
        ctx[line].element = element;
        ctx[line].weave = weave;
    }
}

/// Fill a content span's still-`Blank` lines with `Narrative` + `weave`.
fn fill_content_lines(
    range: TextRange,
    idx: &LineIndex,
    ctx: &mut [LineContext],
    weave: WeavePosition,
) {
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
            ctx[line].element = LineElement::Narrative;
            ctx[line].weave = weave;
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

            // The `- cond:`/`- else:` branch-header check corrects
            // `detect_gathers`' bare-`-` heuristic (Pass 4, which runs
            // before this pass and has already swept a headerless-looking
            // `- else:` line to `Gather`) — so it must be allowed to
            // override `Gather` in addition to `Blank`.
            if matches!(ctx[i].element, LineElement::Blank | LineElement::Gather)
                && is_conditional_branch_header_line(trimmed)
            {
                ctx[i].element = LineElement::Logic;
                ctx[i].weave = WeavePosition {
                    depth: 0,
                    element: WeaveElement::TopLevel,
                };
                continue;
            }

            // Brace scaffold classification only ever fills a gap: a line
            // the main HIR walk (or an earlier pass) already classified —
            // e.g. a `Content` node whose own `ptr` range covers this exact
            // physical line, including a single-line inline conditional
            // used as ordinary narrative, `{visited: You were here
            // before.}` — is never reconsidered here. Only a genuinely
            // uncovered (`Blank`) line can be brace scaffold: that's
            // precisely the gap this pass exists to fill (branch-body
            // content is accumulated via raw token buffering with no
            // per-line `ptr`, so the block's own opening/closing brace
            // line has nothing else to claim it and is still `Blank` at
            // this point).
            if ctx[i].element != LineElement::Blank {
                continue;
            }

            if is_conditional_brace_scaffold_line(trimmed) {
                ctx[i].element = LineElement::Logic;
                ctx[i].weave = WeavePosition {
                    depth: 0,
                    element: WeaveElement::TopLevel,
                };
                continue;
            }

            // Arm content: promote the remaining gap to Narrative so the
            // dialect classify/chain pass (which only looks at Narrative
            // lines) can see cues/dialogue written inside conditional arms.
            ctx[i].element = LineElement::Narrative;
            ctx[i].weave = WeavePosition {
                depth: 0,
                element: *weave_element,
            };
        }
    }
}

/// Whether a trimmed, non-empty line is a conditional/sequence opening
/// brace (bare `{`, or `{` followed by a switch expression whose own last
/// non-whitespace character is `:` — e.g. `{ get_variable(17) >= 1:`) or a
/// bare closing brace (exactly `}`, nothing else on the line).
///
/// Deliberately narrower than "starts with `{`" / "ends with `}`": a
/// narrative line can itself start or end with a brace due to ink's
/// inline-logic syntax without being the block's own routing scaffold —
/// e.g. a standalone inline conditional used as narrative content,
/// `{visited: You were here before.}` (starts with `{` but does NOT end
/// with `:` — it ends with prose closed by `}` on the same line), or
/// narrative ending in a value interpolation, `You have {gold}` (ends
/// with `}` but does not start with `{`). Neither shape is a genuine
/// scaffold brace, so neither is matched.
///
/// Also gated (belt-and-suspenders, see `apply_conditional_scaffold`) to
/// only ever fire on a line still `Blank` — a line the HIR content walk
/// already classified is never reconsidered here.
fn is_conditional_brace_scaffold_line(trimmed: &str) -> bool {
    if trimmed == "{" || trimmed == "}" {
        return true;
    }
    trimmed.starts_with('{') && trimmed.ends_with(':')
}

/// Whether a trimmed, non-empty line is a multiline branch header
/// (`- cond:` / `- else:`, ink's `-` bullet followed eventually by a bare
/// trailing `:`).
///
/// Unlike the brace check, this may override a line `detect_gathers`
/// already swept to `Gather` (its bare-`-` heuristic can't tell a branch
/// header apart from a weave gather) in addition to a still-`Blank` line —
/// see the `Blank | Gather` match in `apply_conditional_scaffold`.
fn is_conditional_branch_header_line(trimmed: &str) -> bool {
    // A multiline branch header is a `-`-bulleted (not `->`) line whose
    // last non-whitespace character is `:` — e.g. `- get_variable(16) == 2:`
    // or `- else:`. This deliberately does NOT match `- else: -> busy`
    // (inline-divert branch shorthand): that line ends in a divert target,
    // not `:`, and the divert itself already classifies via `Stmt::Divert`'s
    // `ptr` — this scaffold check only needs to catch the header itself when
    // it's the JSON pinned repro shape (bare `- cond:` / `- else:` opening a
    // multiline body), and it must not clobber a divert-terminated line that
    // `walk_stmt` already placed correctly.
    trimmed.starts_with('-') && !trimmed.starts_with("->") && trimmed.ends_with(':')
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
    fn choice_body_blank_line_inherits_body_weave() {
        // Just two spaces — no text content. The element stays Blank, but
        // the line inherits the body weave (#478) so Tab knows the depth —
        // this used to be a TS-side post-pass covering only this exact
        // shape; it now lives here and chains through blank runs.
        let source = "=== start ===\n* Choice one\n  \n\n- done\n";
        let ctx = make_contexts(source);
        assert_eq!(ctx[2].element, LineElement::Blank);
        assert_eq!(ctx[2].weave.element, WeaveElement::ChoiceBody);
        assert_eq!(ctx[2].weave.depth, 1);
        // The blank run chains: the next blank line inherits too.
        assert_eq!(ctx[3].element, LineElement::Blank);
        assert_eq!(ctx[3].weave.element, WeaveElement::ChoiceBody);
        // The gather that closes the weave is unaffected.
        assert_eq!(ctx[4].element, LineElement::Gather);
        assert_eq!(ctx[4].weave.element, WeaveElement::GatherContinuation);
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
    fn narrative_with_standalone_inline_conditional_keeps_narrative_class() {
        // Regression guard for the conditional-scaffold pass (#413 follow-
        // up): a whole physical line composed of a single-line inline
        // conditional (`{cond: text}`) is ordinary narrative content, not
        // the routing brace of a multi-arm `{ - cond: ... }` block. It must
        // never be reclassified to `Logic` merely because it starts with
        // `{` and ends with `}` — only a block's own recorded opening/
        // closing brace line is scaffold.
        let source = "=== start ===\n{visited: You were here before.}\nNext.\n";
        let ctx = make_dialect_contexts(source);
        assert_eq!(
            ctx[1].element,
            LineElement::Narrative,
            "a standalone inline conditional used as narrative must not be swept to Logic"
        );
        assert_eq!(ctx[2].element, LineElement::Narrative);
    }

    #[test]
    fn narrative_with_trailing_interpolation_keeps_narrative_class() {
        // Same guard, different shape: ordinary narrative ending in a
        // value interpolation, `You have {gold} coins.` — this doesn't
        // even end with a bare `}` due to trailing punctuation, but a
        // narrative line whose LAST character is `}` (`You have {gold}`)
        // must also stay Narrative, not become conditional-brace scaffold.
        let source = "=== start ===\nYou have {gold}\nMore text.\n";
        let ctx = make_dialect_contexts(source);
        assert_eq!(
            ctx[1].element,
            LineElement::Narrative,
            "narrative ending in an interpolation must not be swept to Logic"
        );
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
