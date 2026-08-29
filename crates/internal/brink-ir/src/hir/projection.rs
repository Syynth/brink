//! HIR projection — the canonical structural model.
//!
//! Projects the HIR onto source ranges: a flat, nested list of [`ProjectedSpan`]s
//! (kind + identity + depth) plus a **per-line container stack** (the weave/rail
//! view). Built on the shared HIR visitor (`crate::hir::visit`). Phase 1 of
//! the HIR overlay (#454): producer + contract, no frontend. See
//! `docs/editor-hir-overlay-spec.md`.
//!
//! Identity is a **range-keyed join** against the analyzer (§6.2): named
//! declarations get their real `DefinitionId`; references get their resolved
//! target. Both `SymbolInfo.range` and `ResolvedRef.range` equal the HIR node's
//! range verbatim, so the join needs no scope logic.
//!
//! ## Coverage & phase-1 decisions
//!
//! - **Containers:** knot, stitch, choice (full-branch extent), gather
//!   continuation, and conditional/sequence branches — block-level per-branch,
//!   inline as one span over the whole `{...}` (inline branch content now
//!   carries a real per-branch `Provenance`, issue #404, but this producer
//!   still emits one container per construct rather than per branch — that
//!   coarser granularity is exactly what `line_context` marks per line; see
//!   `project_content_extras`'s doc for the not-yet-wired per-branch data).
//! - **`ChoiceLine` vs `ChoiceBody`** (`line_context` `WeaveElement`) is
//!   **derivable by the consumer**, not split here: a Choice container's first
//!   line is the choice line, its remaining lines are the body. No producer
//!   change needed.
//! - **Threads / tunnels** are single statements with no HIR body (the threaded
//!   content lives at the target knot), so they are **reference spans**, not
//!   rail containers.
//! - **Dropped for phase 1:** per-`ContentPart` glue/spring/text spans and the
//!   interpolation `{expr}` region span — these have no own source range (like
//!   bare expression leaves) and would need a CST walk; references *inside*
//!   interpolations are already covered via `enter_expr`.
//! - **Determinism:** the HIR is `Vec`-based and the identity maps are only
//!   looked up (never iterated for emission), so span order is deterministic.
//! - Byte ranges only; line/UTF-16 conversion is the phase-2 WASM bridge.

use std::collections::BTreeMap;

use crate::hir::{
    Block, Choice, Conditional, Content, ContentPart, DivertPath, Expr, HirFile, HirVisitor, Knot,
    Sequence, Stitch, Stmt,
};
use brink_format::DefinitionId;
use rowan::TextRange;

use crate::line_index::LineIndex;

/// The kind of a projected span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    // ── Containers (block-level; drive rails + the per-line stack) ──
    Knot,
    Stitch,
    Choice,
    /// A gather continuation (the block after a choice set converges).
    Gather,
    ConditionalBranch,
    SequenceBranch,

    // ── Named declarations (inline, identity-bearing) ──
    Label,
    Param,
    VarDecl,
    ConstDecl,
    ListDecl,
    ListMember,
    External,
    TempDecl,

    // ── References (inline, resolved) ──
    Divert,
    VarRef,
    Call,

    // ── Content (inline) ──
    Content,
    Interpolation,
    Tag,
    Include,

    // ── Statement / construct spans (inline, no identity) ──
    /// A simple divert *statement* (`-> target`; whole `ptr` extent).
    /// Distinct from the [`SpanKind::Divert`] target reference inside it:
    /// reference spans also arise from divert-target *expressions* (`~ temp
    /// x = -> hub`, choice conditions), which are not divert statements —
    /// line views classify from the statement span only. Tunnels and
    /// threads are their own kinds (#480) so "standalone divert" is a
    /// structural fact, not a text sniff.
    DivertStmt,
    /// A tunnel call statement (`-> knot ->`; whole `ptr` extent).
    TunnelStmt,
    /// A thread start statement (`<- knot`; whole `ptr` extent).
    ThreadStmt,
    /// A `-> END` / `-> DONE` divert (terminal by design — distinct from
    /// [`SpanKind::Divert`] so consumers never treat it as an unresolved
    /// reference). Covers the whole divert statement.
    DivertTerminal,
    /// A logic statement without its own declaration/reference span:
    /// assignments (`~ x = 1`) and returns (`~ return x`, `->->`).
    Logic,
    /// A whole conditional construct's extent (the HIR `ptr` range —
    /// braces excluded, as lowered). Non-container: feeds line-view
    /// scaffold classification, never the per-line stack/rails.
    Conditional,
    /// A whole sequence construct's extent; see [`SpanKind::Conditional`].
    Sequence,
}

impl SpanKind {
    /// Whether this kind is a block-level container (participates in the
    /// per-line stack / rails).
    #[must_use]
    pub fn is_container(self) -> bool {
        matches!(
            self,
            Self::Knot
                | Self::Stitch
                | Self::Choice
                | Self::Gather
                | Self::ConditionalBranch
                | Self::SequenceBranch
        )
    }
}

/// A span projected from an HIR node onto its source range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectedSpan {
    pub range: TextRange,
    pub kind: SpanKind,
    /// Container-nesting depth (0 at file top level).
    pub depth: u32,
    /// `DefinitionId` for a declaration span (named symbols).
    pub def_id: Option<DefinitionId>,
    /// Resolved target `DefinitionId` for a reference span.
    pub target_id: Option<DefinitionId>,
    /// Walk-order id for container spans — identifies "the same block" (stable
    /// within a doc version, no stamping). `None` for non-containers.
    pub handle: Option<u32>,
    /// For Choice containers: `+` (sticky) vs `*` (once-only).
    pub sticky: Option<bool>,
    /// Ink weave depth for Choice/Gather containers — the choice set's sigil
    /// depth, with inline choice sets (inside conditional/sequence branches)
    /// inheriting the surrounding weave depth, exactly as `line_context`'s
    /// `WeavePosition.depth` reports it. Distinct from `depth`, which counts
    /// all container nesting including knots/stitches.
    pub weave_depth: Option<u32>,
}

/// One container covering a line, in the per-line stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineContainer {
    pub kind: SpanKind,
    pub handle: u32,
    pub depth: u32,
}

/// The container stack covering a single source line, outermost (lowest depth)
/// → innermost. The innermost entry is the line's `WeaveElement`-equivalent;
/// the whole stack drives concentric rails.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineStack {
    pub containers: Vec<LineContainer>,
}

/// The projection of one file: nested spans + the per-line container stack.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Projection {
    pub spans: Vec<ProjectedSpan>,
    /// One entry per source line.
    pub lines: Vec<LineStack>,
    /// Option identity (#480): each Choice container's full lineage of
    /// zero-based option indices through the weave, keyed by the container's
    /// `handle`. Derived from real HIR nesting (a new choice set restarts
    /// its group at 0 — gathers close groups by construction). Side table,
    /// not serialized with the spans.
    pub option_paths: BTreeMap<u32, Vec<u32>>,
}

/// `BTreeMap` key for a `TextRange` (`rowan::TextRange` implements no
/// `Ord`; this crate's determinism rule disallows `HashMap`).
#[must_use]
pub fn range_key(range: TextRange) -> (u32, u32) {
    (range.start().into(), range.end().into())
}

/// The analyzer-identity join key a span was (or would be) joined by
/// (#3064 B2): declaration spans and named containers join through
/// `decl_ids` by a name/label range; reference spans join through
/// `ref_targets` by their own range. Recorded alongside every span so a
/// per-segment STRUCTURAL projection can be identity-joined later, at
/// assembly time, once the key is rebased to absolute coordinates —
/// reproducing exactly the lookup the whole-file walk performs inline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKey {
    /// Look up in the declaration-identity map (`analysis.index.symbols`).
    Decl(TextRange),
    /// Look up in the reference-target map (`analysis.resolutions`).
    Ref(TextRange),
    /// Look up in the anonymous-container identity map (#3234): the
    /// stamped `container_id` of an ANONYMOUS container (a choice target,
    /// a gather continuation, a conditional/sequence branch, an inline
    /// sequence's wrapper), keyed by the range recorded here — the node's
    /// own source range, which [`anonymous_container_ids`] derives
    /// identically from a stamped-but-NOT-normalized clone of the same
    /// pristine HIR. Joined at assembly, like the other keys, so
    /// per-segment structural memos stay id-free and backdatable. The ids
    /// are codegen's real ids by construction: the map is built by the
    /// same `stamp_container_ids` call the codegen road runs before
    /// normalization, and the lift inherits ids rather than re-minting
    /// them (#3275).
    Anon(TextRange),
}

/// A projection fragment (#3064 B2): spans + aligned join keys +
/// walk-local option paths, WITHOUT line stacks (built once over the
/// assembled whole) and without the identity join applied (`def_id`/
/// `target_id` are `None`; assembly replays them via `join_keys`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectionParts {
    pub spans: Vec<ProjectedSpan>,
    /// One entry per span, aligned by index.
    pub join_keys: Vec<Option<JoinKey>>,
    /// Keyed by walk-local handle (0-based within this fragment).
    pub option_paths: BTreeMap<u32, Vec<u32>>,
    /// Number of handles the walk assigned — the next fragment's handle
    /// offset during assembly.
    pub handle_count: u32,
}

/// The walk half of a projection for one HIR fragment, structural
/// (#3064 B2): the container/reference visitor pass only — no file-level
/// declaration prologue (see [`project_file_decl_parts`]), no line
/// stacks, no identity join. Per-segment memos are built from this.
#[must_use]
pub fn project_walk_parts(hir: &HirFile, source: &str) -> ProjectionParts {
    let empty_decls = BTreeMap::new();
    let empty_refs = BTreeMap::new();
    let mut v = new_visitor(source, &empty_decls, &empty_refs, &empty_decls);
    crate::hir::visit::visit(hir, &mut v);
    ProjectionParts {
        spans: v.spans,
        join_keys: v.join_keys,
        option_paths: v.option_paths,
        handle_count: v.next_handle,
    }
}

/// The file-level declaration prologue as parts (#3064 B2): the
/// `VAR`/`CONST`/`LIST`(+members)/`EXTERNAL`/`INCLUDE` spans that
/// [`project_hir`] emits before the block-tree walk, in the same order,
/// with join keys and no identity join. Run over the ASSEMBLED file's
/// HIR at assembly time (cheap — O(declarations)).
#[must_use]
pub fn project_file_decl_parts(hir: &HirFile, source: &str) -> ProjectionParts {
    let empty_decls = BTreeMap::new();
    let empty_refs = BTreeMap::new();
    let mut v = new_visitor(source, &empty_decls, &empty_refs, &empty_decls);
    emit_file_decl_spans(&mut v, hir);
    ProjectionParts {
        spans: v.spans,
        join_keys: v.join_keys,
        option_paths: v.option_paths,
        handle_count: v.next_handle,
    }
}

/// Project a file's HIR onto its source ranges without the analyzer identity
/// join — every `def_id`/`target_id` is `None`. For structural consumers
/// (the `line_context` view) that need spans and line stacks but no
/// cross-file identity, and must not require an `AnalysisResult`.
#[must_use]
pub fn project_hir_structural(hir: &HirFile, source: &str) -> Projection {
    project_with_maps(
        hir,
        source,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
}

fn new_visitor<'a>(
    source: &'a str,
    decl_ids: &'a BTreeMap<(u32, u32), DefinitionId>,
    ref_targets: &'a BTreeMap<(u32, u32), DefinitionId>,
    anon_ids: &'a BTreeMap<(u32, u32), DefinitionId>,
) -> ProjectionVisitor<'a> {
    ProjectionVisitor {
        source,
        decl_ids,
        ref_targets,
        anon_ids,
        spans: Vec::new(),
        join_keys: Vec::new(),
        depth: 0,
        next_handle: 0,
        cs_depths: Vec::new(),
        continuation_labels: BTreeMap::new(),
        slot_construct_ranges: Vec::new(),
        cs_child_counters: Vec::new(),
        open_choices: Vec::new(),
        option_paths: BTreeMap::new(),
    }
}

/// File-level declarations hang off `HirFile` directly, not the block
/// tree — emitted explicitly before the walk (§6.2 coverage note).
/// Shared by [`project_with_maps`] (whole-file) and
/// [`project_file_decl_parts`] (assembly), so the two can never drift.
fn emit_file_decl_spans(v: &mut ProjectionVisitor<'_>, hir: &HirFile) {
    for var in &hir.variables {
        v.push_decl(var.name.range, SpanKind::VarDecl);
    }
    for c in &hir.constants {
        v.push_decl(c.name.range, SpanKind::ConstDecl);
    }
    for list in &hir.lists {
        v.push_decl(list.name.range, SpanKind::ListDecl);
        for member in &list.members {
            v.push_decl(member.name.range, SpanKind::ListMember);
        }
    }
    for ext in &hir.externals {
        v.push_decl(ext.name.range, SpanKind::External);
    }
    for inc in &hir.includes {
        v.push_inline(inc.ptr.text_range(), SpanKind::Include);
    }
}

pub fn project_with_maps(
    hir: &HirFile,
    source: &str,
    decl_ids: &BTreeMap<(u32, u32), DefinitionId>,
    ref_targets: &BTreeMap<(u32, u32), DefinitionId>,
    anon_ids: &BTreeMap<(u32, u32), DefinitionId>,
) -> Projection {
    let mut v = new_visitor(source, decl_ids, ref_targets, anon_ids);
    emit_file_decl_spans(&mut v, hir);
    crate::hir::visit::visit(hir, &mut v);

    let spans = v.spans;
    let option_paths = v.option_paths;
    let lines = build_line_stacks(&spans, source);
    Projection {
        spans,
        lines,
        option_paths,
    }
}

struct ProjectionVisitor<'a> {
    source: &'a str,
    decl_ids: &'a BTreeMap<(u32, u32), DefinitionId>,
    ref_targets: &'a BTreeMap<(u32, u32), DefinitionId>,
    /// Anonymous-container identity (#3234) — filled by whole-file
    /// callers holding a stamped clone ([`anonymous_container_ids`]);
    /// empty in structural walks and per-segment memos, whose `Anon`
    /// join replays at assembly instead.
    anon_ids: &'a BTreeMap<(u32, u32), DefinitionId>,
    spans: Vec<ProjectedSpan>,
    /// Aligned with `spans`: the identity-join key each span used (or
    /// would use, in a structural walk) — see [`JoinKey`].
    join_keys: Vec<Option<JoinKey>>,
    depth: u32,
    next_handle: u32,
    /// Effective weave depth of each enclosing choice set, innermost last —
    /// pushed at `enter_stmt(ChoiceSet)`, popped at `exit_stmt`. An inline
    /// choice set (inside a conditional/sequence branch) inherits the
    /// surrounding weave depth instead of its own `cs.depth`, mirroring
    /// `line_context`'s `walk_choice_set`.
    cs_depths: Vec<u32>,
    /// Continuation-label ranges of every choice set seen, recorded at
    /// `enter_stmt(ChoiceSet)` with the set's effective weave depth — so
    /// `enter_block` can tell a gather continuation's own label apart from
    /// a `LabeledBlock`'s (both arrive as `Block.label`), and stamp it.
    continuation_labels: BTreeMap<(u32, u32), u32>,
    /// Extents of inline constructs sitting in choice slot text (transitive,
    /// via `ContentContext`) — statement spans inside them are suppressed:
    /// a divert in `* [Go {ready: -> hub}]` is choice text, not a divert
    /// statement line.
    slot_construct_ranges: Vec<TextRange>,
    /// Per-choice-set next-option counter, innermost set last (#480).
    cs_child_counters: Vec<u32>,
    /// Open-choice lineage of option indices, outermost first (#480).
    open_choices: Vec<u32>,
    /// Choice handle → option path (the `Projection.option_paths` table).
    option_paths: BTreeMap<u32, Vec<u32>>,
}

impl ProjectionVisitor<'_> {
    /// The weave depth of the innermost enclosing choice set (0 outside any).
    fn current_weave_depth(&self) -> u32 {
        self.cs_depths.last().copied().unwrap_or(0)
    }

    /// Whether `range` lies inside an inline construct in choice slot text.
    fn in_slot_construct(&self, range: TextRange) -> bool {
        self.slot_construct_ranges
            .iter()
            .any(|r| r.contains_range(range))
    }

    /// Emit an inline declaration span, joining its `DefinitionId` by range.
    fn push_decl(&mut self, range: TextRange, kind: SpanKind) {
        let def_id = self.decl_ids.get(&range_key(range)).copied();
        self.join_keys.push(Some(JoinKey::Decl(range)));
        self.spans.push(ProjectedSpan {
            range,
            kind,
            depth: self.depth,
            def_id,
            target_id: None,
            handle: None,
            sticky: None,
            weave_depth: None,
        });
    }

    /// Emit an inline reference span, joining its resolved target by range.
    fn push_ref(&mut self, range: TextRange, kind: SpanKind) {
        let target_id = self.ref_targets.get(&range_key(range)).copied();
        self.join_keys.push(Some(JoinKey::Ref(range)));
        self.spans.push(ProjectedSpan {
            range,
            kind,
            depth: self.depth,
            def_id: None,
            target_id,
            handle: None,
            sticky: None,
            weave_depth: None,
        });
    }

    /// Emit a plain inline span (no identity).
    fn push_inline(&mut self, range: TextRange, kind: SpanKind) {
        self.join_keys.push(None);
        self.spans.push(ProjectedSpan {
            range,
            kind,
            depth: self.depth,
            def_id: None,
            target_id: None,
            handle: None,
            sticky: None,
            weave_depth: None,
        });
    }

    /// Emit a container span over `range`, joining `def_id` if the key is
    /// a `Decl` (named containers); `Anon` keys are recorded for the
    /// assembly-time join (#3234) and fill nothing here.
    fn push_container(&mut self, range: TextRange, kind: SpanKind, join: Option<JoinKey>) {
        let _ = self.push_container_full(range, kind, join, None, None);
    }

    /// Emit a weave container (Choice/Gather) carrying stickiness and the
    /// ink weave depth. Returns the assigned handle.
    fn push_weave_container(
        &mut self,
        range: TextRange,
        kind: SpanKind,
        join: Option<JoinKey>,
        sticky: Option<bool>,
        weave_depth: u32,
    ) -> u32 {
        self.push_container_full(range, kind, join, sticky, Some(weave_depth))
    }

    fn push_container_full(
        &mut self,
        range: TextRange,
        kind: SpanKind,
        join: Option<JoinKey>,
        sticky: Option<bool>,
        weave_depth: Option<u32>,
    ) -> u32 {
        let def_id = match join {
            Some(JoinKey::Decl(r)) => self.decl_ids.get(&range_key(r)).copied(),
            Some(JoinKey::Anon(r)) => self.anon_ids.get(&range_key(r)).copied(),
            _ => None,
        };
        self.join_keys.push(join);
        let handle = self.next_handle;
        self.next_handle += 1;
        self.spans.push(ProjectedSpan {
            range,
            kind,
            depth: self.depth,
            def_id,
            target_id: None,
            handle: Some(handle),
            sticky,
            weave_depth,
        });
        handle
    }

    /// Emit reference spans for a path-typed divert target.
    fn push_divert_target(&mut self, target: &crate::hir::DivertTarget) {
        if let DivertPath::Path(path) = &target.path {
            self.push_ref(path.range, SpanKind::Divert);
        }
    }

    /// A conditional's branch bodies as container spans (block-level or inline).
    fn push_cond_branches(&mut self, cond: &Conditional) {
        for branch in &cond.branches {
            if let Some(ext) = block_extent(&branch.body) {
                self.push_container(
                    ext,
                    SpanKind::ConditionalBranch,
                    Some(JoinKey::Anon(branch.ptr.text_range())),
                );
            }
        }
    }

    /// A sequence's branch bodies as container spans (block-level or inline).
    fn push_seq_branches(&mut self, seq: &Sequence) {
        for branch in &seq.branches {
            if let Some(ext) = block_extent(&branch.body) {
                self.push_container(
                    ext,
                    SpanKind::SequenceBranch,
                    Some(JoinKey::Anon(branch.ptr.text_range())),
                );
            }
        }
    }

    /// Project a content node's inline conditionals/sequences and its tags.
    ///
    /// Inline `{...}` constructs are single-line; each is emitted as **one**
    /// container over the whole construct (`ptr`) rather than per-branch,
    /// which is exactly the granularity `line_context` marks per line. This
    /// is no longer forced by a data gap — `CondBranch`/`SequenceBranch::ptr`
    /// now carries a real per-branch source range even for inline branches
    /// (issue #404) — it is simply not wired up here; a future per-branch
    /// inline projection would iterate `cond.branches`/`seq.branches` and
    /// push one container per `branch.ptr` instead of one for the whole
    /// construct. References inside interpolations are covered by
    /// `enter_expr` (the walker descends).
    ///
    /// Construct-extent spans ([`SpanKind::Conditional`]/[`SpanKind::Sequence`])
    /// are emitted only for body content (`ctx == Body`): inline logic in a
    /// choice's start/bracket/inner text is choice text, never scaffold — the
    /// same transitive gate both structural line-view consumers apply.
    fn project_content_extras(&mut self, content: &Content, ctx: crate::hir::ContentContext) {
        let in_body = ctx == crate::hir::ContentContext::Body;
        for part in &content.parts {
            self.project_content_part_extras(part, in_body);
        }
        for tag in &content.tags {
            self.push_inline(tag.ptr.text_range(), SpanKind::Tag);
        }
    }

    /// One [`ContentPart`], recursing into a [`ContentPart::Span`]'s
    /// children — a span nesting a conditional/sequence (§4.3's nesting
    /// doctrine) still needs its container spans projected for the editor,
    /// the same reasoning `hir::visit::walk_content_part` documents for
    /// reference-tracking.
    fn project_content_part_extras(&mut self, part: &ContentPart, in_body: bool) {
        match part {
            ContentPart::InlineConditional(cond) => {
                if in_body {
                    self.push_inline(cond.ptr.text_range(), SpanKind::Conditional);
                } else {
                    self.slot_construct_ranges.push(cond.ptr.text_range());
                }
                // No Anon key: an inline conditional construct has no
                // single container id — its branches carry the ids, and
                // this span deliberately covers the whole construct (see
                // this fn's doc). A future per-branch inline projection
                // would key each `branch.ptr` like `push_cond_branches`.
                self.push_container(cond.ptr.text_range(), SpanKind::ConditionalBranch, None);
            }
            ContentPart::InlineSequence(seq) => {
                if in_body {
                    self.push_inline(seq.ptr.text_range(), SpanKind::Sequence);
                } else {
                    self.slot_construct_ranges.push(seq.ptr.text_range());
                }
                // The wrapper container IS the construct's identity (its
                // visit count drives selection), so the whole-construct
                // span joins it (#3234).
                self.push_container(
                    seq.ptr.text_range(),
                    SpanKind::SequenceBranch,
                    Some(JoinKey::Anon(seq.ptr.text_range())),
                );
            }
            ContentPart::Span(span) => {
                for child in &span.children {
                    self.project_content_part_extras(child, in_body);
                }
            }
            ContentPart::Text(_)
            | ContentPart::Glue
            | ContentPart::Spring
            | ContentPart::Interpolation(_) => {}
        }
    }
}

impl HirVisitor for ProjectionVisitor<'_> {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_block(&mut self, block: &Block) {
        // Gather / labeled-block labels are `Block.label`, covered uniformly
        // here. A continuation's own label is stamped with the choice set's
        // weave depth (recorded at `enter_stmt(ChoiceSet)`) so views can tell
        // it from a `LabeledBlock`'s label — the two carry different
        // overwrite semantics in `line_context`.
        if let Some(label) = &block.label {
            let def_id = self.decl_ids.get(&range_key(label.range)).copied();
            self.join_keys.push(Some(JoinKey::Decl(label.range)));
            self.spans.push(ProjectedSpan {
                range: label.range,
                kind: SpanKind::Label,
                depth: self.depth,
                def_id,
                target_id: None,
                handle: None,
                sticky: None,
                weave_depth: self
                    .continuation_labels
                    .get(&range_key(label.range))
                    .copied(),
            });
        }
    }

    fn enter_knot(&mut self, knot: &Knot) {
        self.push_container(
            knot.ptr.text_range(),
            SpanKind::Knot,
            Some(JoinKey::Decl(knot.name.range)),
        );
        // The knot's own name identifier as a decl span too (for go-to-def).
        self.push_decl(knot.name.range, SpanKind::Knot);
        for param in &knot.params {
            self.push_decl(param.name.range, SpanKind::Param);
        }
        self.depth += 1;
    }

    fn exit_knot(&mut self, _knot: &Knot) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn enter_stitch(&mut self, stitch: &Stitch) {
        self.push_container(
            stitch.ptr.text_range(),
            SpanKind::Stitch,
            Some(JoinKey::Decl(stitch.name.range)),
        );
        self.push_decl(stitch.name.range, SpanKind::Stitch);
        for param in &stitch.params {
            self.push_decl(param.name.range, SpanKind::Param);
        }
        self.depth += 1;
    }

    fn exit_stitch(&mut self, _stitch: &Stitch) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn enter_choice(&mut self, choice: &Choice) {
        // Full-branch extent: the choice's own line unioned with its body (§5.1).
        let mut extent = choice.ptr.text_range();
        if let Some(body) = block_extent(&choice.body) {
            extent = extent.cover(body);
        }
        // The choice's own sigil count is the ground truth for its weave
        // depth — for weave-folded sets it equals `cs.depth`, and for inline
        // sets (which don't weave-fold, so mixed sigil depths share one set)
        // it is the only per-choice source (#478). Falls back to the set's
        // depth when the line has no leading sigil (e.g. a choice mid-line
        // in a single-line inline conditional).
        let weave_depth = choice_sigil_depth(self.source, choice.ptr.text_range().start())
            .unwrap_or_else(|| self.current_weave_depth());
        let handle = self.push_weave_container(
            extent,
            SpanKind::Choice,
            // #3234: anonymous identity, keyed by the choice's own line
            // range — the stamped clone keys `choice.container_id` the
            // same way. A LABELED choice's stamped id IS the label id
            // (`lookup_label_id` resolves it at stamp time), so one join
            // path serves both.
            Some(JoinKey::Anon(choice.ptr.text_range())),
            Some(choice.is_sticky),
            weave_depth,
        );
        // Option identity (#480): this choice's index within its set, under
        // the open lineage. The counter was pushed at `enter_stmt(ChoiceSet)`.
        let index = if let Some(counter) = self.cs_child_counters.last_mut() {
            let i = *counter;
            *counter += 1;
            i
        } else {
            0
        };
        self.open_choices.push(index);
        self.option_paths.insert(handle, self.open_choices.clone());
        if let Some(label) = &choice.label {
            self.push_decl(label.range, SpanKind::Label);
        }
        for tag in &choice.tags {
            self.push_inline(tag.ptr.text_range(), SpanKind::Tag);
        }
        self.depth += 1;
    }

    fn exit_choice(&mut self, _choice: &Choice) {
        self.depth = self.depth.saturating_sub(1);
        self.open_choices.pop();
    }

    fn enter_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Divert(d) => {
                // The statement span first, then the target reference. A
                // terminal divert (`-> END` / `-> DONE`) has no path to
                // reference, so its statement span is the terminal kind.
                // A divert inside an inline construct in choice slot text
                // (`* [Go {ready: -> hub}]`) is choice text, not a divert
                // statement line — no statement span (the target reference
                // still projects).
                if matches!(d.target.path, DivertPath::Done | DivertPath::End) {
                    if let Some(ptr) = &d.ptr
                        && !self.in_slot_construct(ptr.text_range())
                    {
                        self.push_inline(ptr.text_range(), SpanKind::DivertTerminal);
                    }
                } else {
                    if let Some(ptr) = &d.ptr
                        && !self.in_slot_construct(ptr.text_range())
                    {
                        self.push_inline(ptr.text_range(), SpanKind::DivertStmt);
                    }
                    self.push_divert_target(&d.target);
                }
            }
            Stmt::TunnelCall(t) => {
                if !self.in_slot_construct(t.ptr.text_range()) {
                    self.push_inline(t.ptr.text_range(), SpanKind::TunnelStmt);
                }
                for target in &t.targets {
                    self.push_divert_target(target);
                }
            }
            Stmt::ThreadStart(t) => {
                if !self.in_slot_construct(t.ptr.text_range()) {
                    self.push_inline(t.ptr.text_range(), SpanKind::ThreadStmt);
                }
                self.push_divert_target(&t.target);
            }
            Stmt::TempDecl(t) => self.push_decl(t.name.range, SpanKind::TempDecl),
            Stmt::Assignment(a) if !self.in_slot_construct(a.ptr.text_range()) => {
                self.push_inline(a.ptr.text_range(), SpanKind::Logic);
            }
            Stmt::Return(r) => {
                if let Some(ptr) = &r.ptr
                    && !self.in_slot_construct(ptr.text_range())
                {
                    self.push_inline(ptr.text_range(), SpanKind::Logic);
                }
            }
            Stmt::ChoiceSet(cs) => {
                // A weave-folded choice set carries its own sigil depth. An
                // inline one (inside a conditional/sequence branch) has
                // `cs.depth == 0`, so its depth is read from the first
                // choice's sigils in source (#478) — the depth Tab/Enter
                // transitions need to rebuild the prefix — falling back to
                // the surrounding weave depth if the line is unreadable.
                let weave_depth = if cs.context == crate::ChoiceSetContext::Inline {
                    cs.choices
                        .first()
                        .and_then(|c| choice_sigil_depth(self.source, c.ptr.text_range().start()))
                        .unwrap_or_else(|| self.current_weave_depth())
                } else {
                    cs.depth
                };
                // `block_extent` includes the continuation label, so a bare
                // labeled gather (`- (g)` with an empty continuation) still
                // projects its container. Record the label so `enter_block`
                // can stamp it as a continuation label.
                if let Some(label) = &cs.continuation.label {
                    self.continuation_labels
                        .insert(range_key(label.range), weave_depth);
                }
                if let Some(ext) = block_extent(&cs.continuation) {
                    self.push_weave_container(
                        ext,
                        SpanKind::Gather,
                        Some(JoinKey::Anon(ext)),
                        None,
                        weave_depth,
                    );
                }
                self.cs_depths.push(weave_depth);
                self.cs_child_counters.push(0);
            }
            Stmt::Conditional(cond) => {
                self.push_inline(cond.ptr.text_range(), SpanKind::Conditional);
                self.push_cond_branches(cond);
            }
            Stmt::Sequence(seq) => {
                self.push_inline(seq.ptr.text_range(), SpanKind::Sequence);
                self.push_seq_branches(seq);
            }
            _ => {}
        }
    }

    fn exit_stmt(&mut self, stmt: &Stmt) {
        if matches!(stmt, Stmt::ChoiceSet(_)) {
            self.cs_depths.pop();
            self.cs_child_counters.pop();
        }
    }

    fn enter_content(&mut self, content: &Content, ctx: crate::hir::ContentContext) {
        if let Some(ptr) = &content.ptr {
            self.push_inline(ptr.text_range(), SpanKind::Content);
        }
        self.project_content_extras(content, ctx);
    }

    fn enter_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Path(p) => self.push_ref(p.range, SpanKind::VarRef),
            Expr::DivertTarget(p) => self.push_ref(p.range, SpanKind::Divert),
            Expr::Call(p, _) => self.push_ref(p.range, SpanKind::Call),
            _ => {}
        }
    }
}

/// The choice-sigil depth (`*`/`+` count) of the line containing `offset`,
/// or `None` when the line doesn't start with a choice sigil (#478 — inline
/// choice sets carry `cs.depth == 0`, so their weave depth comes from the
/// literal sigils).
fn choice_sigil_depth(source: &str, offset: rowan::TextSize) -> Option<u32> {
    let start = usize::from(offset).min(source.len());
    let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    let line = source[line_start..].split('\n').next().unwrap_or("");
    let mut depth = 0u32;
    let mut chars = line.trim_start().chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            '*' | '+' => {
                depth += 1;
                chars.next();
            }
            ' ' => {
                chars.next();
            }
            _ => break,
        }
    }
    (depth > 0).then_some(depth)
}

/// The source extent of a block: its label (if any) unioned with its
/// statements' ranges, or `None` for an empty unlabeled block. Recurses so a
/// container's extent covers nested content. The label matters twice over: a
/// bare labeled gather (`- (g)`, no statements) still has an extent, and a
/// nested `LabeledBlock`'s gather line — whose prose content is ptr-less —
/// stays covered by the enclosing container, so views derive its weave.
fn block_extent(block: &Block) -> Option<TextRange> {
    let mut acc: Option<TextRange> = block.label.as_ref().map(|l| l.range);
    for stmt in &block.stmts {
        if let Some(r) = stmt_extent(stmt) {
            acc = Some(match acc {
                Some(a) => a.cover(r),
                None => r,
            });
        }
    }
    acc
}

fn stmt_extent(stmt: &Stmt) -> Option<TextRange> {
    match stmt {
        Stmt::Content(c) => c.ptr.as_ref().map(crate::Provenance::text_range),
        Stmt::Divert(d) => d.ptr.as_ref().map(crate::Provenance::text_range),
        Stmt::TunnelCall(t) => Some(t.ptr.text_range()),
        Stmt::ThreadStart(t) => Some(t.ptr.text_range()),
        Stmt::TempDecl(t) => Some(t.ptr.text_range()),
        Stmt::Assignment(a) => Some(a.ptr.text_range()),
        Stmt::Return(r) => r.ptr.as_ref().map(crate::Provenance::text_range),
        Stmt::ChoiceSet(cs) => {
            let mut acc: Option<TextRange> = None;
            for choice in &cs.choices {
                let mut ext = choice.ptr.text_range();
                if let Some(body) = block_extent(&choice.body) {
                    ext = ext.cover(body);
                }
                acc = Some(acc.map_or(ext, |a| a.cover(ext)));
            }
            if let Some(cont) = block_extent(&cs.continuation) {
                acc = Some(acc.map_or(cont, |a| a.cover(cont)));
            }
            acc
        }
        Stmt::LabeledBlock(b) => block_extent(b),
        Stmt::Conditional(c) => Some(c.ptr.text_range()),
        Stmt::Sequence(s) => Some(s.ptr.text_range()),
        // Issue #2108: `AttachElement`/`EndElementRun` carry no
        // `Provenance`/`ptr` of their own.
        Stmt::ExprStmt(_) | Stmt::EndOfLine | Stmt::AttachElement(_) | Stmt::EndElementRun => None,
        Stmt::LogicBlock(lb) => Some(lb.ptr.text_range()),
        Stmt::Await(a) => Some(a.ptr.text_range()),
    }
}

/// Build the per-line container stack from the emitted container spans: for each
/// line a container covers, record it; then order each line's stack outermost →
/// innermost by depth.
/// The last line a container's rail (and reported range) should cover — the
/// TIGHT end of the two-range model (issue #3054 review): the structural
/// range still runs to where the next sibling begins (ownership — folding,
/// containment), but the display range trims trailing whitespace AND a
/// trailing `///` doc block, which documents the NEXT declaration, not this
/// container ("look at what roll gets defined as": a function whose body is
/// two lines was reported four lines long, through the next function's
/// docs).
#[must_use]
pub fn tight_container_end_line(idx: &LineIndex, source: &str, range: rowan::TextRange) -> u32 {
    let start = usize::from(range.start()).min(source.len());
    let end = usize::from(range.end()).min(source.len());
    let doc_start = crate::doc_extended_start(source, end);
    let content_end = if doc_start > start {
        doc_start.min(end)
    } else {
        end
    };
    // `trimmed` is the exclusive end of the trimmed text — a char boundary.
    // After `trim_end` it can never sit at column 0 (the text does not end
    // in a newline), so its own line IS the last content line; deriving
    // `trimmed - 1` instead would split a multi-byte char (the live wasm
    // panic this comment memorializes: em-dashes in the fixture).
    let trimmed = start + source[start..content_end].trim_end().len();
    idx.line_col(rowan::TextSize::from(
        u32::try_from(trimmed).unwrap_or(u32::MAX),
    ))
    .0
}

/// Public since #3064 B2: assembly (`brink-db`) builds the stacks once
/// over the assembled whole file's spans.
#[must_use]
pub fn build_line_stacks(spans: &[ProjectedSpan], source: &str) -> Vec<LineStack> {
    let idx = LineIndex::new(source);
    let line_count = source.lines().count().max(1);
    let mut lines = vec![LineStack::default(); line_count];

    for span in spans {
        let (Some(handle), true) = (span.handle, span.kind.is_container()) else {
            continue;
        };
        let (start_line, _) = idx.line_col(span.range.start());
        // Rails cover the TIGHT range — actual content only, not the
        // trailing blank lines / next declaration's doc block the
        // structural range owns. See `tight_container_end_line`.
        let end_line = tight_container_end_line(&idx, source, span.range).max(start_line);
        for line in start_line..=end_line {
            if let Some(stack) = lines.get_mut(line as usize) {
                stack.containers.push(LineContainer {
                    kind: span.kind,
                    handle,
                    depth: span.depth,
                });
            }
        }
    }

    for stack in &mut lines {
        stack.containers.sort_by_key(|c| c.depth);
    }
    lines
}

// ─── Anonymous-container identity (#3234) ───────────────────────────

/// Collect every anonymous container's stamped `DefinitionId`, keyed by
/// the same range each projection push site records as its
/// [`JoinKey::Anon`]:
///
/// * a choice → its `ptr` range (the choice's own line);
/// * a gather continuation → its [`block_extent`];
/// * a block-level conditional/sequence branch → the branch's `ptr`;
/// * an inline sequence → its construct `ptr` (the wrapper container's
///   visit count is the construct's identity).
///
/// The input must be a **stamped, NOT normalized** `HirFile` — the
/// pristine tree `stamp_container_ids` runs on, where authored nodes are
/// 1:1 with compiled containers (#3275). A normalized tree would carry
/// lift-cloned branches whose duplicated `ptr` ranges collide as keys.
/// Nodes the stamp walk deliberately leaves unstamped (constructs in
/// choice slot text, tags, span children) simply contribute no entry, so
/// their spans stay identity-free rather than guessing.
#[must_use]
pub fn anonymous_container_ids(hir: &HirFile) -> BTreeMap<(u32, u32), DefinitionId> {
    struct Collector {
        ids: BTreeMap<(u32, u32), DefinitionId>,
    }
    impl Collector {
        fn insert(&mut self, range: TextRange, id: Option<DefinitionId>) {
            if let Some(id) = id {
                self.ids.insert(range_key(range), id);
            }
        }
        fn collect_part(&mut self, part: &ContentPart) {
            match part {
                ContentPart::InlineSequence(seq) => {
                    self.insert(seq.ptr.text_range(), seq.container_id);
                }
                ContentPart::Span(span) => {
                    for child in &span.children {
                        self.collect_part(child);
                    }
                }
                _ => {}
            }
        }
    }
    impl HirVisitor for Collector {
        fn enter_choice(&mut self, choice: &Choice) {
            self.insert(choice.ptr.text_range(), choice.container_id);
        }
        fn enter_stmt(&mut self, stmt: &Stmt) {
            match stmt {
                Stmt::ChoiceSet(cs) => {
                    if let Some(ext) = block_extent(&cs.continuation) {
                        self.insert(ext, cs.gather_id);
                    }
                }
                Stmt::Conditional(cond) => {
                    for branch in &cond.branches {
                        self.insert(branch.ptr.text_range(), branch.container_id);
                    }
                }
                Stmt::Sequence(seq) => {
                    for branch in &seq.branches {
                        self.insert(branch.ptr.text_range(), branch.body.container_id);
                    }
                }
                _ => {}
            }
        }
        fn enter_content(&mut self, content: &Content, _ctx: crate::hir::ContentContext) {
            for part in &content.parts {
                self.collect_part(part);
            }
        }
    }
    let mut c = Collector {
        ids: BTreeMap::new(),
    };
    crate::hir::visit::visit(hir, &mut c);
    c.ids
}
