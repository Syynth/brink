//! Shared read-only traversal of the HIR block tree.
//!
//! Historically the block-tree descent (blocks → statements → choice sets /
//! conditionals / sequences → content → expressions) was re-implemented in each
//! IDE query (`story_graph`, `line_context`, `folding`) and analyzer pass
//! (`validate`, `external_check`). This module is the single canonical walk
//! those consumers share, so the recursion — and, crucially, *which* nesting
//! sites get descended — lives in exactly one place. See issue #457.
//!
//! ## Model: dumb walker + stateful visitor
//!
//! The walk itself is fixed and stateless; each [`HirVisitor`] keeps whatever
//! state it needs (depth, weave position, an accumulator) in its own fields and
//! updates it inside its `enter_*` / `exit_*` handlers. All trait methods
//! default to no-ops, so a visitor implements only the hooks it cares about.
//!
//! ## Scope
//!
//! The traversal covers the block tree reachable from [`HirFile::root_content`]
//! and each knot / stitch body, plus [`HirVisitor::enter_knot`] /
//! [`HirVisitor::enter_stitch`] structural hooks. It deliberately does **not**
//! visit:
//! - the flat file-level declaration vecs (`variables`, `constants`, `lists`,
//!   `externals`, `includes`) — these are flat, non-recursive, and iterated
//!   directly by callers that need them;
//! - knot / stitch names and params beyond exposing the `Knot` / `Stitch` node
//!   to the hooks;
//! - **tag contents** — neither `Content.tags` nor `Choice.tags` are descended,
//!   so inline expressions/conditionals inside a tag (`# {score(x)}`) are not
//!   visited. `enter_content` exposes a `Content` node's own `tags` for a
//!   visitor to inspect, but choice tags are reachable only via `enter_choice`.
//!   This mirrors every pre-existing HIR walk (all skipped tags); it is a known
//!   coverage gap, not a behavior change.
//!
//! Expression descent is opt-in ([`HirVisitor::visit_exprs`]): structural
//! walkers that never look at expressions pay nothing for the expression tree.

use super::types::{
    Block, BlockStmt, Choice, ChoiceSet, CondKind, Conditional, Content, ContentPart, DivertTarget,
    ElseBranch, Expr, ForStmt, HirFile, IfStmt, Knot, LogicBlock, Sequence, Stitch, Stmt,
    StringPart, WhileStmt,
};

/// Where a visited [`Content`] sits in the tree.
///
/// Some consumers are position-sensitive — e.g. folding processes the content
/// of a statement body but deliberately not a choice's inline text (`* [a
/// {x|y} b]`), so it can discriminate on this rather than re-deriving structure.
/// This describes the content's *immediate* position; a consumer that needs a
/// transitive notion ("anywhere inside choice inline text") must track it via
/// its own state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentContext {
    /// A `Stmt::Content` line within a block body.
    Body,
    /// A choice's text before `[` (appears in the choice list and the output).
    ChoiceStart,
    /// A choice's text inside `[...]` (appears only in the choice list).
    ChoiceBracket,
    /// A choice's text after `]` (appears only after selection).
    ChoiceInner,
}

/// Read-only visitor over the HIR block tree.
///
/// Every method defaults to a no-op; implement only the hooks you need and keep
/// per-traversal state on the implementor. `enter_*` fires before a node's
/// children are visited, `exit_*` after — so a container's full source extent
/// (the union of its children's ranges) can be folded up in `exit_block`.
pub trait HirVisitor {
    /// A knot is about to be walked (its body follows). Fires before any of the
    /// knot's stitches.
    fn enter_knot(&mut self, _knot: &Knot) {}
    /// A knot and all its stitches have been walked.
    fn exit_knot(&mut self, _knot: &Knot) {}
    /// A stitch is about to be walked (its body follows).
    fn enter_stitch(&mut self, _stitch: &Stitch) {}
    /// A stitch has been walked.
    fn exit_stitch(&mut self, _stitch: &Stitch) {}

    /// A choice is about to be walked — its condition, inline content, and body
    /// follow, before the enclosing choice set's gather continuation. Paired
    /// with [`HirVisitor::exit_choice`]; consumers needing "am I inside a
    /// choice" track a depth counter across the two.
    fn enter_choice(&mut self, _choice: &Choice) {}
    /// A choice and its body have been walked (before the continuation).
    fn exit_choice(&mut self, _choice: &Choice) {}

    /// A block is about to be walked (its statements follow).
    fn enter_block(&mut self, _block: &Block) {}
    /// A block and all its statements have been walked.
    fn exit_block(&mut self, _block: &Block) {}

    /// A statement is about to be walked (its children follow).
    fn enter_stmt(&mut self, _stmt: &Stmt) {}
    /// A statement and all its children have been walked.
    fn exit_stmt(&mut self, _stmt: &Stmt) {}

    /// A content line is about to be walked. `ctx` gives its immediate position
    /// (body vs a choice's inline slots). The full [`Content`] (including its
    /// `tags`) is exposed; the walker does not descend into tags itself.
    fn enter_content(&mut self, _content: &Content, _ctx: ContentContext) {}

    /// An expression node, in descent order — only called when
    /// [`HirVisitor::visit_exprs`] returns `true`.
    fn enter_expr(&mut self, _expr: &Expr) {}

    /// Opt in to expression-tree descent. Defaults to `false` so structural
    /// walkers skip expressions entirely.
    fn visit_exprs(&self) -> bool {
        false
    }
}

/// Walk a whole file: top-level content, then each knot body and, within it,
/// each stitch body.
pub fn visit(hir: &HirFile, v: &mut impl HirVisitor) {
    walk_block(&hir.root_content, v);
    for knot in &hir.knots {
        v.enter_knot(knot);
        walk_block(&knot.body, v);
        for stitch in &knot.stitches {
            v.enter_stitch(stitch);
            walk_block(&stitch.body, v);
            v.exit_stitch(stitch);
        }
        v.exit_knot(knot);
    }
}

/// Walk a single block and its statements. Exposed so a visitor can be driven
/// over a sub-tree directly; content is reported with [`ContentContext::Body`].
pub fn walk_block(block: &Block, v: &mut impl HirVisitor) {
    walk_block_ctx(block, ContentContext::Body, v);
}

/// Walk a block, reporting its content (and any content nested inside inline
/// conditionals/sequences) with `ctx`. `ctx` is a choice slot only when the
/// block is (transitively) inside a choice's start/bracket/inner text; it is
/// reset to [`ContentContext::Body`] at a choice's own body and the gather
/// continuation, so the position is *transitive*, not just immediate.
fn walk_block_ctx(block: &Block, ctx: ContentContext, v: &mut impl HirVisitor) {
    v.enter_block(block);
    for stmt in &block.stmts {
        walk_stmt(stmt, ctx, v);
    }
    v.exit_block(block);
}

fn walk_stmt(stmt: &Stmt, ctx: ContentContext, v: &mut impl HirVisitor) {
    v.enter_stmt(stmt);
    match stmt {
        Stmt::Content(c) => walk_content(c, ctx, v),
        Stmt::Divert(d) => walk_target(&d.target, v),
        Stmt::TunnelCall(t) => {
            for target in &t.targets {
                walk_target(target, v);
            }
        }
        Stmt::ThreadStart(t) => walk_target(&t.target, v),
        Stmt::TempDecl(t) => {
            if let Some(e) = &t.value {
                walk_expr(e, v);
            }
        }
        Stmt::Assignment(a) => {
            walk_expr(&a.target, v);
            walk_expr(&a.value, v);
        }
        Stmt::Return(r) => {
            if let Some(e) = &r.value {
                walk_expr(e, v);
            }
            for e in &r.onwards_args {
                walk_expr(e, v);
            }
        }
        Stmt::ChoiceSet(cs) => walk_choice_set(cs, v),
        Stmt::LabeledBlock(b) => walk_block_ctx(b, ctx, v),
        Stmt::Conditional(c) => walk_conditional(c, ctx, v),
        Stmt::Sequence(s) => walk_sequence(s, ctx, v),
        Stmt::ExprStmt(e) => walk_expr(e, v),
        Stmt::EndOfLine => {}
        Stmt::LogicBlock(lb) => walk_logic_block(lb, v),
        Stmt::Await(a) => {
            if let Some(e) = &a.condition {
                walk_expr(e, v);
            }
        }
    }
    v.exit_stmt(stmt);
}

// ─── T1b `~ { … }` blocks ────────────────────────────────────────────
//
// `BlockStmt` is a closed set with no weave variant, so this sub-walk never
// calls back into `walk_stmt`/`walk_content`/`enter_choice` etc. — only
// expressions and nested block statements.

fn walk_logic_block(lb: &LogicBlock, v: &mut impl HirVisitor) {
    for bs in &lb.stmts {
        walk_block_stmt(bs, v);
    }
}

fn walk_block_stmt(bs: &BlockStmt, v: &mut impl HirVisitor) {
    match bs {
        BlockStmt::TempDecl(t) => {
            if let Some(e) = &t.value {
                walk_expr(e, v);
            }
        }
        BlockStmt::Assignment(a) => {
            walk_expr(&a.target, v);
            walk_expr(&a.value, v);
        }
        BlockStmt::Return(r) => {
            if let Some(e) = &r.value {
                walk_expr(e, v);
            }
            for e in &r.onwards_args {
                walk_expr(e, v);
            }
        }
        BlockStmt::If(i) => walk_if_stmt(i, v),
        BlockStmt::While(w) => walk_while_stmt(w, v),
        BlockStmt::For(f) => walk_for_stmt(f, v),
        BlockStmt::Break(_) | BlockStmt::Continue(_) => {}
        BlockStmt::ExprStmt(e) => walk_expr(e, v),
        BlockStmt::Await(a) => {
            if let Some(e) = &a.condition {
                walk_expr(e, v);
            }
        }
    }
}

fn walk_if_stmt(i: &IfStmt, v: &mut impl HirVisitor) {
    walk_expr(&i.condition, v);
    for s in &i.body {
        walk_block_stmt(s, v);
    }
    match &i.else_branch {
        Some(ElseBranch::ElseIf(inner)) => walk_if_stmt(inner, v),
        Some(ElseBranch::Else(stmts)) => {
            for s in stmts {
                walk_block_stmt(s, v);
            }
        }
        None => {}
    }
}

fn walk_while_stmt(w: &WhileStmt, v: &mut impl HirVisitor) {
    walk_expr(&w.condition, v);
    for s in &w.body {
        walk_block_stmt(s, v);
    }
}

fn walk_for_stmt(f: &ForStmt, v: &mut impl HirVisitor) {
    walk_expr(&f.iterable, v);
    for s in &f.body {
        walk_block_stmt(s, v);
    }
}

fn walk_target(target: &DivertTarget, v: &mut impl HirVisitor) {
    for e in &target.args {
        walk_expr(e, v);
    }
}

fn walk_content(content: &Content, ctx: ContentContext, v: &mut impl HirVisitor) {
    v.enter_content(content, ctx);
    for part in &content.parts {
        match part {
            ContentPart::Interpolation(e) => walk_expr(e, v),
            // Content nested inside an inline conditional/sequence keeps the
            // enclosing `ctx` — so a choice slot's position stays transitive.
            ContentPart::InlineConditional(c) => walk_conditional(c, ctx, v),
            ContentPart::InlineSequence(s) => walk_sequence(s, ctx, v),
            ContentPart::Text(_) | ContentPart::Glue | ContentPart::Spring => {}
        }
    }
}

fn walk_choice_set(cs: &ChoiceSet, v: &mut impl HirVisitor) {
    for choice in &cs.choices {
        v.enter_choice(choice);
        if let Some(e) = &choice.condition {
            walk_expr(e, v);
        }
        if let Some(c) = &choice.start_content {
            walk_content(c, ContentContext::ChoiceStart, v);
        }
        if let Some(c) = &choice.bracket_content {
            walk_content(c, ContentContext::ChoiceBracket, v);
        }
        if let Some(c) = &choice.inner_content {
            walk_content(c, ContentContext::ChoiceInner, v);
        }
        // A choice's body is the selected content — not inline choice text —
        // so it (and its continuation below) resets to Body.
        walk_block_ctx(&choice.body, ContentContext::Body, v);
        v.exit_choice(choice);
    }
    walk_block_ctx(&cs.continuation, ContentContext::Body, v);
}

fn walk_conditional(cond: &Conditional, ctx: ContentContext, v: &mut impl HirVisitor) {
    if let CondKind::Switch(e) = &cond.kind {
        walk_expr(e, v);
    }
    for branch in &cond.branches {
        if let Some(e) = &branch.condition {
            walk_expr(e, v);
        }
        walk_block_ctx(&branch.body, ctx, v);
    }
}

fn walk_sequence(seq: &Sequence, ctx: ContentContext, v: &mut impl HirVisitor) {
    for branch in &seq.branches {
        walk_block_ctx(branch, ctx, v);
    }
}

fn walk_expr(expr: &Expr, v: &mut impl HirVisitor) {
    if !v.visit_exprs() {
        return;
    }
    v.enter_expr(expr);
    match expr {
        Expr::Call(_path, args) => {
            for arg in args {
                walk_expr(arg, v);
            }
        }
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => walk_expr(inner, v),
        Expr::Infix(lhs, _, rhs) => {
            walk_expr(lhs, v);
            walk_expr(rhs, v);
        }
        Expr::String(s) => {
            for part in &s.parts {
                if let StringPart::Interpolation(e) = part {
                    walk_expr(e, v);
                }
            }
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Path(_)
        | Expr::DivertTarget(_)
        | Expr::ListLiteral(_) => {}
        Expr::ArrayLiteral(a) => {
            for e in &a.elements {
                walk_expr(e, v);
            }
        }
        Expr::MapLiteral(m) => {
            for (k, val) in &m.entries {
                walk_expr(k, v);
                walk_expr(val, v);
            }
        }
        Expr::Index(idx) => {
            walk_expr(&idx.base, v);
            walk_expr(&idx.index, v);
        }
        Expr::StructLiteral(sl) => {
            for (_name, val) in &sl.fields {
                walk_expr(val, v);
            }
        }
        Expr::FieldAccess(fa) => {
            walk_expr(&fa.base, v);
        }
        // T1c `#fn(target, args…)`: the target is a static `Path` field
        // (not an `Expr` child, same as `Call`'s path); only the bound
        // arguments descend.
        Expr::FnLiteral(fl) => {
            for arg in &fl.args {
                walk_expr(arg, v);
            }
        }
        // T1e `ref lvalue-path`: only the operand descends.
        Expr::RefArg(ra) => walk_expr(&ra.operand, v),
        // NS-A5 `start..end` / `start..=end`: both bounds descend.
        Expr::Range(r) => {
            walk_expr(&r.start, v);
            walk_expr(&r.end, v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FileId;
    use brink_syntax::parse;

    #[derive(Default)]
    struct Counts {
        knots: usize,
        stitches: usize,
        enter_block: usize,
        exit_block: usize,
        stmts: usize,
        content: usize,
        exprs: usize,
        visit_exprs: bool,
    }

    impl HirVisitor for Counts {
        fn enter_knot(&mut self, _: &Knot) {
            self.knots += 1;
        }
        fn enter_stitch(&mut self, _: &Stitch) {
            self.stitches += 1;
        }
        fn enter_block(&mut self, _: &Block) {
            self.enter_block += 1;
        }
        fn exit_block(&mut self, _: &Block) {
            self.exit_block += 1;
        }
        fn enter_stmt(&mut self, _: &Stmt) {
            self.stmts += 1;
        }
        fn enter_content(&mut self, _: &Content, _: ContentContext) {
            self.content += 1;
        }
        fn enter_expr(&mut self, _: &Expr) {
            self.exprs += 1;
        }
        fn visit_exprs(&self) -> bool {
            self.visit_exprs
        }
    }

    fn lower_src(src: &str) -> HirFile {
        let parsed = parse(src);
        let tree = parsed.tree();
        let (hir, _, _) = crate::hir::lower::lower(FileId(0), &tree);
        hir
    }

    #[test]
    fn visits_structure_and_balances_enter_exit() {
        let hir = lower_src("Hello {name}\n=== greet ===\n= again\n+ [pick] -> greet\n");
        let mut c = Counts {
            visit_exprs: true,
            ..Default::default()
        };
        visit(&hir, &mut c);

        assert_eq!(c.knots, 1, "one knot");
        assert_eq!(c.stitches, 1, "one stitch");
        assert_eq!(c.enter_block, c.exit_block, "enter/exit block balanced");
        // root content + greet body + again body + choice body + continuation.
        assert!(c.enter_block >= 3, "several blocks: {}", c.enter_block);
        assert!(c.content >= 1, "at least the greeting content");
        assert!(c.exprs >= 1, "the {{name}} interpolation is an expr");
    }

    #[test]
    fn expr_descent_is_gated_off_by_default() {
        let hir = lower_src("Hello {name}\n");
        let mut c = Counts::default(); // visit_exprs = false
        visit(&hir, &mut c);

        assert_eq!(c.exprs, 0, "no expression hooks when visit_exprs is false");
        assert!(c.content >= 1, "content is still visited");
    }
}
