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

use std::collections::HashMap;

use brink_analyzer::AnalysisResult;
use brink_format::DefinitionId;
use brink_ir::FileId;
use brink_ir::hir::{
    Block, Choice, Content, DivertPath, Expr, HirFile, HirVisitor, Knot, Stitch, Stmt,
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

    let mut v = ProjectionVisitor {
        decl_ids: &decl_ids,
        ref_targets: &ref_targets,
        spans: Vec::new(),
        depth: 0,
        next_handle: 0,
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

    brink_ir::hir::visit::visit(hir, &mut v);

    let spans = v.spans;
    let lines = build_line_stacks(&spans, source);
    Projection { spans, lines }
}

struct ProjectionVisitor<'a> {
    decl_ids: &'a HashMap<TextRange, DefinitionId>,
    ref_targets: &'a HashMap<TextRange, DefinitionId>,
    spans: Vec<ProjectedSpan>,
    depth: u32,
    next_handle: u32,
}

impl ProjectionVisitor<'_> {
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
        });
    }

    /// Emit a container span over `range`, joining `def_id` if named. Returns
    /// the assigned handle.
    fn push_container(&mut self, range: TextRange, kind: SpanKind, def_id: Option<DefinitionId>) {
        let handle = self.next_handle;
        self.next_handle += 1;
        self.spans.push(ProjectedSpan {
            range,
            kind,
            depth: self.depth,
            def_id,
            target_id: None,
            handle: Some(handle),
        });
    }

    /// Emit reference spans for a path-typed divert target.
    fn push_divert_target(&mut self, target: &brink_ir::hir::DivertTarget) {
        if let DivertPath::Path(path) = &target.path {
            self.push_ref(path.range, SpanKind::Divert);
        }
    }

    /// Project a content node's tags. References inside interpolations are
    /// covered separately by `enter_expr` (the walker descends them).
    fn project_tags(&mut self, content: &Content) {
        for tag in &content.tags {
            self.push_inline(tag.ptr.text_range(), SpanKind::Tag);
        }
    }
}

impl HirVisitor for ProjectionVisitor<'_> {
    fn visit_exprs(&self) -> bool {
        true
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
        self.push_container(extent, SpanKind::Choice, None);
        if let Some(label) = &choice.label {
            self.push_decl(label.range, SpanKind::Label);
        }
        self.depth += 1;
    }

    fn exit_choice(&mut self, _choice: &Choice) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn enter_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Divert(d) => self.push_divert_target(&d.target),
            Stmt::TunnelCall(t) => {
                for target in &t.targets {
                    self.push_divert_target(target);
                }
            }
            Stmt::ThreadStart(t) => self.push_divert_target(&t.target),
            Stmt::TempDecl(t) => self.push_decl(t.name.range, SpanKind::TempDecl),
            Stmt::ChoiceSet(cs) => {
                if let Some(gather) = block_extent(&cs.continuation) {
                    self.push_container(gather, SpanKind::Gather, None);
                }
            }
            Stmt::Conditional(cond) => {
                for branch in &cond.branches {
                    if let Some(ext) = block_extent(&branch.body) {
                        self.push_container(ext, SpanKind::ConditionalBranch, None);
                    }
                }
            }
            Stmt::Sequence(seq) => {
                for branch in &seq.branches {
                    if let Some(ext) = block_extent(branch) {
                        self.push_container(ext, SpanKind::SequenceBranch, None);
                    }
                }
            }
            _ => {}
        }
    }

    fn enter_content(&mut self, content: &Content, _ctx: brink_ir::hir::ContentContext) {
        if let Some(ptr) = &content.ptr {
            self.push_inline(ptr.text_range(), SpanKind::Content);
        }
        self.project_tags(content);
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

/// The source extent of a block: the union of its statements' ranges, or `None`
/// for an empty block. Recurses so a container's extent covers nested content.
fn block_extent(block: &Block) -> Option<TextRange> {
    let mut acc: Option<TextRange> = None;
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
    use brink_syntax::ast::{AstPtr, SyntaxNodePtr};
    match stmt {
        Stmt::Content(c) => c.ptr.as_ref().map(SyntaxNodePtr::text_range),
        Stmt::Divert(d) => d.ptr.as_ref().map(SyntaxNodePtr::text_range),
        Stmt::TunnelCall(t) => Some(t.ptr.text_range()),
        Stmt::ThreadStart(t) => Some(t.ptr.text_range()),
        Stmt::TempDecl(t) => Some(t.ptr.text_range()),
        Stmt::Assignment(a) => Some(a.ptr.text_range()),
        Stmt::Return(r) => r.ptr.as_ref().map(AstPtr::text_range),
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
}
