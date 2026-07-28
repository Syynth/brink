//! HIR projection — the canonical structural model.
//!
//! Projects the HIR onto source ranges: a flat, nested list of [`ProjectedSpan`]s
//! (kind + identity + depth) plus a **per-line container stack** (the weave/rail
//! view). Built on the shared HIR visitor (`brink_ir::hir::visit`). Phase 1 of
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

use std::collections::HashMap;

use brink_analyzer::AnalysisResult;
use brink_format::DefinitionId;
use brink_ir::FileId;
use brink_ir::hir::{
    Block, Choice, Conditional, Content, ContentPart, DivertPath, Expr, HirFile, HirVisitor, Knot,
    Sequence, Stitch, Stmt,
};
use rowan::TextRange;

use crate::LineIndex;

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
#[derive(Debug, Clone, Default)]
pub struct Projection {
    pub spans: Vec<ProjectedSpan>,
    /// One entry per source line.
    pub lines: Vec<LineStack>,
    /// Option identity (#480): each Choice container's full lineage of
    /// zero-based option indices through the weave, keyed by the container's
    /// `handle`. Derived from real HIR nesting (a new choice set restarts
    /// its group at 0 — gathers close groups by construction). Side table,
    /// not serialized with the spans.
    pub option_paths: HashMap<u32, Vec<u32>>,
}

/// Project a file's HIR onto its source ranges.
#[must_use]
pub fn project_hir(
    hir: &HirFile,
    source: &str,
    analysis: &AnalysisResult,
    file: FileId,
) -> Projection {
    // Prebuild the range-keyed identity maps (§6.2). Ranges are unique per
    // declaration identifier / reference site, so no scope logic is needed.
    let mut decl_ids: HashMap<TextRange, DefinitionId> = HashMap::new();
    for info in analysis.index.symbols.values() {
        if info.file == file {
            decl_ids.insert(info.range, info.id);
        }
    }
    let mut ref_targets: HashMap<TextRange, DefinitionId> = HashMap::new();
    for r in &analysis.resolutions {
        if r.file == file {
            ref_targets.insert(r.range, r.target);
        }
    }

    project_with_maps(hir, source, &decl_ids, &ref_targets)
}

/// Project a file's HIR onto its source ranges without the analyzer identity
/// join — every `def_id`/`target_id` is `None`. For structural consumers
/// (the `line_context` view) that need spans and line stacks but no
/// cross-file identity, and must not require an `AnalysisResult`.
#[must_use]
pub fn project_hir_structural(hir: &HirFile, source: &str) -> Projection {
    project_with_maps(hir, source, &HashMap::new(), &HashMap::new())
}

fn project_with_maps(
    hir: &HirFile,
    source: &str,
    decl_ids: &HashMap<TextRange, DefinitionId>,
    ref_targets: &HashMap<TextRange, DefinitionId>,
) -> Projection {
    let mut v = ProjectionVisitor {
        source,
        decl_ids,
        ref_targets,
        spans: Vec::new(),
        depth: 0,
        next_handle: 0,
        cs_depths: Vec::new(),
        continuation_labels: HashMap::new(),
        slot_construct_ranges: Vec::new(),
        cs_child_counters: Vec::new(),
        open_choices: Vec::new(),
        option_paths: HashMap::new(),
    };

    // File-level declarations hang off HirFile directly, not the block tree —
    // iterate them explicitly (§6.2 coverage note).
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

    brink_ir::hir::visit::visit(hir, &mut v);

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
    decl_ids: &'a HashMap<TextRange, DefinitionId>,
    ref_targets: &'a HashMap<TextRange, DefinitionId>,
    spans: Vec<ProjectedSpan>,
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
    continuation_labels: HashMap<TextRange, u32>,
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
    option_paths: HashMap<u32, Vec<u32>>,
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
        let def_id = self.decl_ids.get(&range).copied();
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
        let target_id = self.ref_targets.get(&range).copied();
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

    /// Emit a container span over `range`, joining `def_id` if named.
    fn push_container(&mut self, range: TextRange, kind: SpanKind, def_id: Option<DefinitionId>) {
        let _ = self.push_container_full(range, kind, def_id, None, None);
    }

    /// Emit a weave container (Choice/Gather) carrying stickiness and the
    /// ink weave depth. Returns the assigned handle.
    fn push_weave_container(
        &mut self,
        range: TextRange,
        kind: SpanKind,
        sticky: Option<bool>,
        weave_depth: u32,
    ) -> u32 {
        self.push_container_full(range, kind, None, sticky, Some(weave_depth))
    }

    fn push_container_full(
        &mut self,
        range: TextRange,
        kind: SpanKind,
        def_id: Option<DefinitionId>,
        sticky: Option<bool>,
        weave_depth: Option<u32>,
    ) -> u32 {
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
    fn push_divert_target(&mut self, target: &brink_ir::hir::DivertTarget) {
        if let DivertPath::Path(path) = &target.path {
            self.push_ref(path.range, SpanKind::Divert);
        }
    }

    /// A conditional's branch bodies as container spans (block-level or inline).
    fn push_cond_branches(&mut self, cond: &Conditional) {
        for branch in &cond.branches {
            if let Some(ext) = block_extent(&branch.body) {
                self.push_container(ext, SpanKind::ConditionalBranch, None);
            }
        }
    }

    /// A sequence's branch bodies as container spans (block-level or inline).
    fn push_seq_branches(&mut self, seq: &Sequence) {
        for branch in &seq.branches {
            if let Some(ext) = block_extent(&branch.body) {
                self.push_container(ext, SpanKind::SequenceBranch, None);
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
    fn project_content_extras(&mut self, content: &Content, ctx: brink_ir::hir::ContentContext) {
        let in_body = ctx == brink_ir::hir::ContentContext::Body;
        for part in &content.parts {
            match part {
                ContentPart::InlineConditional(cond) => {
                    if in_body {
                        self.push_inline(cond.ptr.text_range(), SpanKind::Conditional);
                    } else {
                        self.slot_construct_ranges.push(cond.ptr.text_range());
                    }
                    self.push_container(cond.ptr.text_range(), SpanKind::ConditionalBranch, None);
                }
                ContentPart::InlineSequence(seq) => {
                    if in_body {
                        self.push_inline(seq.ptr.text_range(), SpanKind::Sequence);
                    } else {
                        self.slot_construct_ranges.push(seq.ptr.text_range());
                    }
                    self.push_container(seq.ptr.text_range(), SpanKind::SequenceBranch, None);
                }
                ContentPart::Text(_)
                | ContentPart::Glue
                | ContentPart::Spring
                | ContentPart::Interpolation(_) => {}
            }
        }
        for tag in &content.tags {
            self.push_inline(tag.ptr.text_range(), SpanKind::Tag);
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
            let def_id = self.decl_ids.get(&label.range).copied();
            self.spans.push(ProjectedSpan {
                range: label.range,
                kind: SpanKind::Label,
                depth: self.depth,
                def_id,
                target_id: None,
                handle: None,
                sticky: None,
                weave_depth: self.continuation_labels.get(&label.range).copied(),
            });
        }
    }

    fn enter_knot(&mut self, knot: &Knot) {
        let def_id = self.decl_ids.get(&knot.name.range).copied();
        self.push_container(knot.ptr.text_range(), SpanKind::Knot, def_id);
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
        let def_id = self.decl_ids.get(&stitch.name.range).copied();
        self.push_container(stitch.ptr.text_range(), SpanKind::Stitch, def_id);
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
                let weave_depth = if cs.context == brink_ir::ChoiceSetContext::Inline {
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
                    self.continuation_labels.insert(label.range, weave_depth);
                }
                if let Some(ext) = block_extent(&cs.continuation) {
                    self.push_weave_container(ext, SpanKind::Gather, None, weave_depth);
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

    fn enter_content(&mut self, content: &Content, ctx: brink_ir::hir::ContentContext) {
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
        Stmt::Content(c) => c.ptr.as_ref().map(brink_ir::Provenance::text_range),
        Stmt::Divert(d) => d.ptr.as_ref().map(brink_ir::Provenance::text_range),
        Stmt::TunnelCall(t) => Some(t.ptr.text_range()),
        Stmt::ThreadStart(t) => Some(t.ptr.text_range()),
        Stmt::TempDecl(t) => Some(t.ptr.text_range()),
        Stmt::Assignment(a) => Some(a.ptr.text_range()),
        Stmt::Return(r) => r.ptr.as_ref().map(brink_ir::Provenance::text_range),
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
        Stmt::ExprStmt(_) | Stmt::EndOfLine => None,
        Stmt::LogicBlock(lb) => Some(lb.ptr.text_range()),
        Stmt::Await(a) => Some(a.ptr.text_range()),
    }
}

/// Build the per-line container stack from the emitted container spans: for each
/// line a container covers, record it; then order each line's stack outermost →
/// innermost by depth.
fn build_line_stacks(spans: &[ProjectedSpan], source: &str) -> Vec<LineStack> {
    let idx = LineIndex::new(source);
    let line_count = source.lines().count().max(1);
    let mut lines = vec![LineStack::default(); line_count];

    for span in spans {
        let (Some(handle), true) = (span.handle, span.kind.is_container()) else {
            continue;
        };
        let (start_line, _) = idx.line_col(span.range.start());
        let (end_line, _) = idx.line_col(span.range.end());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::IdeSession;

    fn project(src: &str) -> Projection {
        let mut session = IdeSession::new();
        session.update_and_analyze("main.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");
        let db = session.db();
        let file = db.file_ids().next().expect("one file");
        let hir = db.hir(file).expect("hir");
        project_hir(hir, src, analysis, file)
    }

    #[test]
    fn named_symbols_carry_def_ids_and_refs_carry_targets() {
        let src = "\
=== start ===
Hello {name}.
* [Go] -> hub
=== hub ===
-> DONE
VAR name = \"x\"
";
        let p = project(src);

        // Knot declarations carry a def_id (the range-join against SymbolIndex).
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::Knot && s.def_id.is_some()),
            "a knot span carries a def_id"
        );

        // The `-> hub` divert resolves to a target, and that target is exactly
        // some projected knot's def_id — the range-join, end to end.
        let hub_target = p
            .spans
            .iter()
            .filter(|s| s.kind == SpanKind::Divert)
            .find_map(|s| s.target_id)
            .expect("a resolved divert target");
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::Knot && s.def_id == Some(hub_target)),
            "the divert's target_id matches a projected knot's def_id"
        );

        // The VAR decl is projected from the flat vec, with a def_id.
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::VarDecl && s.def_id.is_some()),
            "VAR decl span with def_id"
        );
    }

    // ── TM-2 inline type annotations (docs/typed-mode-spec.md §3) ──────
    //
    // The IDE pipeline (parse → HIR → analyze → project) must not crash on
    // annotated sources — this is a superset-grammar/HIR extension the
    // projection layer never inspects (it walks knots/params/vars by their
    // pre-existing shape), so the assertion here is really "this whole
    // pipeline runs to completion and still finds the same spans a
    // consumer (hover, go-to-def) would have found without annotations".

    #[test]
    fn ide_pipeline_does_not_crash_on_annotated_sources_and_still_projects_spans() {
        let src = "\
VAR gold: int = 100
CONST max_gold: int = 999
LIST Weathers = sunny, (rainy)
=== function heal(ref hp: int, amount: float): bool ===
~ temp bonus: string = \"none\"
~ return true
= aftermath
~ temp w: List<Weathers> = sunny
-> DONE
";
        let p = project(src);

        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::VarDecl && s.def_id.is_some()),
            "annotated VAR decl still projects with a def_id"
        );
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::ConstDecl && s.def_id.is_some()),
            "annotated CONST decl still projects with a def_id"
        );
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::Knot && s.def_id.is_some()),
            "the function knot with param/return annotations still projects"
        );
        assert!(
            p.spans.iter().any(|s| s.kind == SpanKind::Param),
            "annotated params still project"
        );
        assert!(
            p.spans.iter().any(|s| s.kind == SpanKind::TempDecl),
            "annotated/ascribed temps still project"
        );
    }

    #[test]
    fn ide_pipeline_does_not_crash_on_reserved_and_unknown_type_annotations() {
        // `fn(...)` (reserved until T1c) and an unrecognized name both
        // produce analyzer diagnostics (E062/E061), not a panic anywhere
        // in the pipeline.
        let src = "VAR cb: fn(int): bool = 0\nVAR p: Frobnicator = 0\nCONST bad: Frobnicator = 0\n";
        let p = project(src);
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::VarDecl && s.def_id.is_some()),
            "VAR decls with reserved/unknown annotations still project"
        );
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::ConstDecl && s.def_id.is_some()),
            "CONST decl with an unknown annotation still projects"
        );
    }

    #[test]
    fn inline_conditionals_sequences_and_includes_are_covered() {
        let src = "\
INCLUDE other.ink
=== start ===
Take the {red|blue} pill.
{ready: Go now.}
-> go
=== go ===
-> DONE
";
        let p = project(src);
        assert!(
            p.spans.iter().any(|s| s.kind == SpanKind::Include),
            "INCLUDE span projected"
        );
        assert!(
            p.spans.iter().any(|s| s.kind == SpanKind::SequenceBranch),
            "inline sequence {{red|blue}} → SequenceBranch container"
        );
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::ConditionalBranch),
            "inline conditional {{ready: ...}} → ConditionalBranch container"
        );
    }

    #[test]
    fn containers_and_per_line_stack() {
        let src = "\
=== start ===
Hello.
* [Go]
  Nested body.
- gather
";
        let p = project(src);

        // Knot + choice containers present, with handles.
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::Knot && s.handle.is_some())
        );
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::Choice && s.handle.is_some())
        );

        // Every line's stack is depth-ordered (outermost first).
        for stack in &p.lines {
            assert!(
                stack
                    .containers
                    .windows(2)
                    .all(|w| w[0].depth <= w[1].depth),
                "line stack must be depth-ordered: {stack:?}"
            );
        }

        // The choice body line sits inside both the knot and the choice.
        let deepest = p
            .lines
            .iter()
            .map(|l| l.containers.len())
            .max()
            .unwrap_or(0);
        assert!(deepest >= 2, "a line should be inside knot + choice");
    }

    #[test]
    fn choice_containers_carry_stickiness_and_weave_depth() {
        let src = "\
=== start ===
* Once
* * Nested once
+ Sticky
- gathered
-> END
";
        let p = project(src);
        let choices: Vec<_> = p
            .spans
            .iter()
            .filter(|s| s.kind == SpanKind::Choice)
            .collect();
        assert_eq!(choices.len(), 3);
        assert_eq!(
            (choices[0].sticky, choices[0].weave_depth),
            (Some(false), Some(1))
        );
        assert_eq!(
            (choices[1].sticky, choices[1].weave_depth),
            (Some(false), Some(2)),
            "nested choice carries its sigil depth"
        );
        assert_eq!(
            (choices[2].sticky, choices[2].weave_depth),
            (Some(true), Some(1)),
            "`+` choice is sticky"
        );
        let gather = p
            .spans
            .iter()
            .find(|s| s.kind == SpanKind::Gather)
            .expect("gather container");
        assert_eq!(gather.weave_depth, Some(1));
    }

    #[test]
    fn inline_choice_set_reports_sigil_depth() {
        // Choices inside a conditional arm form an Inline choice set with
        // `cs.depth == 0` — their weave depth comes from the literal sigils
        // (#478), the depth Tab/Enter transitions need to rebuild prefixes.
        let src = "\
=== start ===
{ ready:
    * [Go now]
    * * [Deeper]
- else:
    Not ready.
}
-> END
";
        let p = project(src);
        let choices: Vec<_> = p
            .spans
            .iter()
            .filter(|s| s.kind == SpanKind::Choice)
            .collect();
        assert_eq!(choices.len(), 2);
        assert_eq!(
            choices[0].weave_depth,
            Some(1),
            "single-sigil inline choice: {choices:?}"
        );
        assert_eq!(
            choices[1].weave_depth,
            Some(2),
            "double-sigil inline choice: {choices:?}"
        );
    }

    #[test]
    fn terminal_diverts_project_divert_terminal_spans() {
        let src = "\
=== start ===
-> DONE
=== other ===
-> END
";
        let p = project(src);
        let terminals: Vec<_> = p
            .spans
            .iter()
            .filter(|s| s.kind == SpanKind::DivertTerminal)
            .collect();
        assert_eq!(
            terminals.len(),
            2,
            "-> DONE and -> END each project a DivertTerminal span"
        );
        assert!(
            !p.spans.iter().any(|s| s.kind == SpanKind::Divert),
            "terminal diverts are not Divert reference spans"
        );
    }

    #[test]
    fn divert_statements_project_stmt_spans_but_expressions_do_not() {
        // `-> hub` is a divert statement (DivertStmt + a Divert target ref);
        // `~ temp x = -> hub` holds a divert-target *expression* — a Divert
        // reference only, no statement span. Line views classify from the
        // statement span, so the temp line stays logic, not a divert.
        let src = "\
=== start ===
~ temp x = -> hub
-> hub
=== hub ===
-> DONE
";
        let p = project(src);
        assert_eq!(
            p.spans
                .iter()
                .filter(|s| s.kind == SpanKind::DivertStmt)
                .count(),
            1,
            "only the standalone divert is a statement span"
        );
        assert_eq!(
            p.spans
                .iter()
                .filter(|s| s.kind == SpanKind::Divert)
                .count(),
            2,
            "both the expression and the statement carry target references"
        );
    }

    #[test]
    fn assignments_and_returns_project_logic_spans() {
        let src = "\
VAR gold = 0
=== start ===
~ gold = 1
-> DONE
=== function f ===
~ return 0
";
        let p = project(src);
        let logic = p.spans.iter().filter(|s| s.kind == SpanKind::Logic).count();
        assert_eq!(logic, 2, "one assignment + one return");
    }

    #[test]
    fn choice_tags_project_tag_spans() {
        let src = "\
=== start ===
* Choice # tagged
-> DONE
";
        let p = project(src);
        assert!(
            p.spans.iter().any(|s| s.kind == SpanKind::Tag),
            "choice-level tags project Tag spans"
        );
    }

    #[test]
    fn bare_labeled_gather_projects_its_container() {
        // `- (g)` with an empty continuation: the label range alone is the
        // gather extent (block_extent is None), so the container must still
        // be emitted.
        let src = "\
=== start ===
* Choice
- (g)
";
        let p = project(src);
        let gather = p
            .spans
            .iter()
            .find(|s| s.kind == SpanKind::Gather)
            .expect("bare labeled gather projects a container");
        assert_eq!(gather.weave_depth, Some(1));
    }

    #[test]
    fn construct_extent_spans_are_body_gated() {
        // A statement-level conditional and a body inline sequence project
        // construct-extent spans; an inline sequence in a choice's bracket
        // text projects its container (rails) but NOT a construct span
        // (never scaffold).
        let src = "\
=== start ===
{ ready:
Go.
- else:
Wait.
}
Take the {red|blue} pill.
* [Take the {big|small} dose]
-> DONE
";
        let p = project(src);
        assert_eq!(
            p.spans
                .iter()
                .filter(|s| s.kind == SpanKind::Conditional)
                .count(),
            1,
            "the multiline conditional projects one construct-extent span"
        );
        assert_eq!(
            p.spans
                .iter()
                .filter(|s| s.kind == SpanKind::Sequence)
                .count(),
            1,
            "only the body inline sequence projects a construct span — the \
             choice-bracket one is gated out"
        );
        assert_eq!(
            p.spans
                .iter()
                .filter(|s| s.kind == SpanKind::SequenceBranch)
                .count(),
            2,
            "both inline sequences still project rail containers"
        );
        // Construct spans are not containers: no handle, absent from stacks.
        assert!(
            p.spans
                .iter()
                .filter(|s| matches!(s.kind, SpanKind::Conditional | SpanKind::Sequence))
                .all(|s| s.handle.is_none() && !s.kind.is_container())
        );
    }
}
