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
//!   directly by callers that need them. A `VAR`/`CONST` initializer is an
//!   *expression*, though, and one that
//!   [`crate::symbols::project_manifest`] does record references from — a
//!   consumer that needs those expressions too drives
//!   [`visit_with_decl_initializers`] instead of [`visit`] (issue #1571).
//!   A stateful visitor that needs to reset per-decl state (a diagnostic
//!   anchor, "no enclosing knot/stitch locals here") implements
//!   [`HirVisitor::enter_var_decl`]/[`HirVisitor::enter_const_decl`], which
//!   fire immediately before each initializer's own expression tree (issue
//!   #2098: this is what lets a pass built on `HirVisitor` get decl-
//!   initializer coverage *by construction*, rather than hand-rolling a
//!   second, parallel walk of the same shape it would otherwise have to
//!   remember to keep in sync forever);
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
    Block, BlockStmt, Choice, ChoiceSet, CondKind, Conditional, ConstDecl, Content, ContentPart,
    DivertTarget, ElseBranch, Expr, ForStmt, HirFile, IfStmt, Knot, LambdaBody, LambdaExpr,
    LogicBlock, Sequence, Stitch, Stmt, StringPart, VarDecl, WhileStmt,
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

    /// A [`Sequence`] node is about to be walked (its branch bodies follow).
    /// Fires for **both** shapes a sequence can appear in — a block-level
    /// `Stmt::Sequence` (a promoted multiline `{&\n- a\n- b\n}` block) and a
    /// `ContentPart::InlineSequence` nested inside a content line's text
    /// (`{&a|b}`) — one hook so a consumer that only cares about the
    /// `Sequence` node itself (its `kind`/branch count/`container_id`)
    /// doesn't have to match both `Stmt::Sequence` in `enter_stmt` and
    /// `ContentPart::InlineSequence` inside its own `enter_content` descent
    /// (issue #1674).
    fn enter_sequence(&mut self, _seq: &Sequence) {}

    /// An expression node, in descent order — only called when
    /// [`HirVisitor::visit_exprs`] returns `true`.
    fn enter_expr(&mut self, _expr: &Expr) {}

    /// A lambda literal's own body is about to be walked — fires
    /// immediately after `enter_expr` sees the [`Expr::Lambda`] node itself
    /// and before any of its params/body descend, for **both**
    /// [`LambdaBody::Expr`] and [`LambdaBody::Block`] (a bare-expression
    /// body's own params can shadow just as a block body's can). Paired
    /// with [`HirVisitor::exit_lambda`], fired right after the body finishes
    /// — nested lambdas nest correctly by construction, since both hooks
    /// fire from the same recursive call frame around the body descent.
    ///
    /// Issue #2773: this is the walker's half of the shared lambda-frame
    /// contract — a lambda's own param row (and, for a block body, whatever
    /// `TempDecl`/`for`/`if`/`while` `as` binding its statements introduce)
    /// is a fresh scope that can shadow a same-named outer local of a
    /// *different* type. Before this pair of hooks existed, every
    /// `HirVisitor` consumer that reads a bare-name-keyed locals map (e.g.
    /// `BodyTypes::locals`) from `enter_expr` inherited that shadowing
    /// hazard automatically the moment [`walk_expr`]'s existing `Expr::Lambda`
    /// descent (issue #1685) reached an expression inside the lambda body —
    /// with no signal from the walker that a new scope had opened. A
    /// stateful visitor that needs to prune those names out of its own
    /// locals map for the lambda body's duration implements both hooks and
    /// composes with `brink-analyzer`'s shared
    /// `structs::pruned_locals_for_lambda` helper; a visitor with no such
    /// state (most of them) never needs to implement either.
    fn enter_lambda(&mut self, _lambda: &LambdaExpr) {}

    /// [`HirVisitor::enter_lambda`]'s pair — the lambda's body (and any
    /// scope a stateful visitor pushed for it) has been fully walked.
    fn exit_lambda(&mut self, _lambda: &LambdaExpr) {}

    /// A file-level `VAR` declaration's initializer is about to be walked —
    /// fires immediately before [`visit_with_decl_initializers`] hands its
    /// value expression to [`HirVisitor::enter_expr`]. Only
    /// [`visit_with_decl_initializers`] calls this; [`visit`] never reaches a
    /// declaration at all (see its module doc).
    ///
    /// A stateful visitor that tracks "am I inside a knot/stitch" (a
    /// diagnostic-anchor fallback, an enclosing def's finalized locals, …)
    /// needs this hook to reset that state to its file-scope value before the
    /// initializer's expressions arrive — otherwise the walk's stateful
    /// hooks would see whatever the *last* knot/stitch visited left behind,
    /// not "there is no enclosing def here." A visitor with no such state
    /// (most of them) never needs to implement this at all.
    fn enter_var_decl(&mut self, _var: &VarDecl) {}

    /// [`HirVisitor::enter_var_decl`]'s `CONST` twin.
    fn enter_const_decl(&mut self, _const: &ConstDecl) {}

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

/// Walk everything [`visit`] walks, **plus** the initializer expression of
/// every file-level `VAR` / `CONST` declaration (issue #1571).
///
/// [`visit`] covers the block tree only (see the module doc's Scope section),
/// so a visitor driven by it never sees `VAR n = p.x.y`'s initializer. But
/// [`crate::symbols::project_manifest`] *does* walk those initializers and
/// records an `UnresolvedRef` for each path in them, which the analyzer then
/// resolves into a `ResolvedRef` at the path's whole-path range. Any consumer
/// that maps a `ResolvedRef` back onto the HIR path it came from — `brink-ide`'s
/// rename / find-references segment narrowing — must therefore walk with this
/// entry point, or every reference written inside a declaration initializer
/// silently misses the mapping and is handled as if it had no HIR path at all.
///
/// Deliberately a second entry point rather than folded into [`visit`]: the
/// existing structural consumers (folding, story graph, `validate`,
/// `external_check`) are block-tree walkers by contract, and an initializer
/// expression is not part of the block tree — widening [`visit`] itself would
/// silently change what every one of them sees.
pub fn visit_with_decl_initializers(hir: &HirFile, v: &mut impl HirVisitor) {
    visit(hir, v);
    for var in &hir.variables {
        v.enter_var_decl(var);
        walk_expr(&var.value, v);
    }
    for konst in &hir.constants {
        v.enter_const_decl(konst);
        walk_expr(&konst.value, v);
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
        Stmt::ExprStmt(e) | Stmt::AttachElement(e) => walk_expr(e, v),
        Stmt::EndOfLine | Stmt::EndElementRun => {}
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
        walk_content_part(part, ctx, v);
    }
}

/// One [`ContentPart`], dispatched the same way regardless of whether it
/// sits directly in a [`Content`]'s own `parts` or nested inside a
/// [`ContentPart::Span`]'s `children` — a span is presentational (§4.3);
/// it must never become a blind spot for reference-tracking visitors
/// (rename, unused-variable, the symbol index) just because a word inside
/// it happens to be bolded.
fn walk_content_part(part: &ContentPart, ctx: ContentContext, v: &mut impl HirVisitor) {
    match part {
        ContentPart::Interpolation(e) => walk_expr(e, v),
        // Content nested inside an inline conditional/sequence keeps the
        // enclosing `ctx` — so a choice slot's position stays transitive.
        ContentPart::InlineConditional(c) => walk_conditional(c, ctx, v),
        ContentPart::InlineSequence(s) => walk_sequence(s, ctx, v),
        ContentPart::Span(span) => {
            for child in &span.children {
                walk_content_part(child, ctx, v);
            }
        }
        ContentPart::Text(_) | ContentPart::Glue | ContentPart::Spring => {}
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
    v.enter_sequence(seq);
    for branch in &seq.branches {
        walk_block_ctx(&branch.body, ctx, v);
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
        Expr::Infix(ie) => {
            walk_expr(&ie.lhs, v);
            walk_expr(&ie.rhs, v);
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
        // A lambda's body (issue #1685): its expressions are part of this
        // tree — a consumer counting references or collecting spans must
        // see `|g| g.awake`'s `g.awake`, and a braced body's statements are
        // walked with the same `walk_block_stmt` any code-ground block
        // uses. Params/return annotation are declaration data, not `Expr`
        // children, so they do not descend (same shape as `FnLiteral`'s
        // static target).
        //
        // For a lambda in a VAR/CONST initializer specifically, this
        // position **used to be** a hard `E083`
        // (`lir::lower::decls::is_const_foldable_decl_default` rejected
        // every `Expr::Lambda` default) — issue #1774 (RULED 2026-08-01)
        // lifted exactly that gate, so a decl-default lambda's chain now
        // really does reach `CoalesceLookup` in LIR lowering (via
        // `GlobalLambdaCtx::tables`, threaded from the real
        // `coalesce_types_query` in production — the #1774 review's own
        // finding was that this table used to be hard-coded empty for that
        // one caller regardless).
        Expr::Lambda(l) => {
            // Issue #2773: bracket the body descent with the shared
            // lambda-frame hooks — see [`HirVisitor::enter_lambda`]'s own
            // doc for why this pair exists and what it fixes.
            v.enter_lambda(l);
            match &l.body {
                LambdaBody::Expr(e) => walk_expr(e, v),
                LambdaBody::Block { stmts, tail } => {
                    for bs in stmts {
                        walk_block_stmt(bs, v);
                    }
                    if let Some(t) = tail {
                        walk_expr(t, v);
                    }
                }
            }
            v.exit_lambda(l);
        }
        // NS-A5 `start..end` / `start..=end`: both bounds descend.
        Expr::Range(r) => {
            walk_expr(&r.start, v);
            walk_expr(&r.end, v);
        }
        // Block capture (issue #1839): the captured run is a real part of
        // this tree — a consumer walking references/hover/etc. must see
        // the interior lines a `content`-typed argument holds, exactly as
        // it would if they were still at top-level body position. Reuses
        // `walk_stmt` directly (not `walk_block_ctx`, which also fires
        // `enter_block`/`exit_block` for a real `Block` node this isn't) —
        // `ContentContext::Body` matches every interior line's own
        // top-level position before it was captured.
        Expr::Fragment(stmts) => {
            for s in stmts {
                walk_stmt(s, ContentContext::Body, v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FileId;
    use brink_syntax::parse;
    use rowan::TextRange;

    #[derive(Default)]
    struct Counts {
        knots: usize,
        stitches: usize,
        enter_block: usize,
        exit_block: usize,
        stmts: usize,
        content: usize,
        exprs: usize,
        sequences: usize,
        enter_lambda: usize,
        exit_lambda: usize,
        visit_exprs: bool,
    }

    impl HirVisitor for Counts {
        fn enter_knot(&mut self, _: &Knot) {
            self.knots += 1;
        }
        fn enter_stitch(&mut self, _: &Stitch) {
            self.stitches += 1;
        }
        fn enter_lambda(&mut self, _: &LambdaExpr) {
            self.enter_lambda += 1;
        }
        fn exit_lambda(&mut self, _: &LambdaExpr) {
            self.exit_lambda += 1;
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
        fn enter_sequence(&mut self, _: &Sequence) {
            self.sequences += 1;
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

    /// Issue #1571: `visit` never reaches a `VAR`/`CONST` initializer, even
    /// though `symbols::project_manifest` walks exactly those expressions and
    /// records references from them — so a `ResolvedRef` produced inside one
    /// has no HIR path a `visit`-driven consumer can find.
    #[test]
    fn decl_initializers_are_reached_only_by_the_dedicated_entry_point() {
        let hir = lower_src("VAR c = Colors.Red\nCONST k = Other.Thing\n");

        let mut plain = Counts {
            visit_exprs: true,
            ..Default::default()
        };
        visit(&hir, &mut plain);
        assert_eq!(
            plain.exprs, 0,
            "`visit` walks the block tree only — no declaration initializers"
        );

        let mut with_decls = Counts {
            visit_exprs: true,
            ..Default::default()
        };
        visit_with_decl_initializers(&hir, &mut with_decls);
        assert_eq!(
            with_decls.exprs, 2,
            "both the VAR and the CONST initializer expressions are visited"
        );
    }

    #[test]
    fn decl_initializer_walk_still_covers_everything_visit_covers() {
        let src = "Hello {name}\n=== greet ===\n= again\n+ [pick] -> greet\n";
        let hir = lower_src(src);

        let mut plain = Counts {
            visit_exprs: true,
            ..Default::default()
        };
        visit(&hir, &mut plain);
        let mut with_decls = Counts {
            visit_exprs: true,
            ..Default::default()
        };
        visit_with_decl_initializers(&hir, &mut with_decls);

        // No declarations in this fixture, so the two walks must agree exactly.
        assert_eq!(plain.knots, with_decls.knots);
        assert_eq!(plain.stitches, with_decls.stitches);
        assert_eq!(plain.enter_block, with_decls.enter_block);
        assert_eq!(plain.stmts, with_decls.stmts);
        assert_eq!(plain.content, with_decls.content);
        assert_eq!(plain.exprs, with_decls.exprs);
    }

    /// Issue #1674: [`HirVisitor::enter_sequence`] must fire for a sequence
    /// reached either shape it can appear in — a single-line pipe-separated
    /// inline sequence (`ContentPart::InlineSequence`, nested inside a
    /// content line) and a promoted multiline block sequence
    /// (`Stmt::Sequence`) — so a consumer that only implements this one hook
    /// sees both without also matching `Stmt::Sequence` in `enter_stmt`.
    #[test]
    fn enter_sequence_fires_for_inline_and_block_forms() {
        let inline_hir = lower_src("{&a|b}\n");
        let mut inline_counts = Counts::default();
        visit(&inline_hir, &mut inline_counts);
        assert_eq!(
            inline_counts.sequences, 1,
            "inline pipe-separated sequence: {inline_hir:?}"
        );

        let block_hir = lower_src("{&\n- a\n- b\n}\n");
        let mut block_counts = Counts::default();
        visit(&block_hir, &mut block_counts);
        assert_eq!(
            block_counts.sequences, 1,
            "promoted multiline block sequence: {block_hir:?}"
        );
    }

    /// Test-only probe for
    /// [`decl_hooks_reset_a_stateful_visitors_anchor_before_each_initializer`]:
    /// records whichever "current anchor" was live at each expression it
    /// visits, mimicking the shape a real stateful pass (`coalesce`'s
    /// `CoalesceVisitor::fallback`, e.g.) keeps.
    struct Probe {
        anchor: TextRange,
        anchors_at_expr: Vec<TextRange>,
    }
    impl HirVisitor for Probe {
        fn visit_exprs(&self) -> bool {
            true
        }
        fn enter_knot(&mut self, knot: &Knot) {
            self.anchor = knot.ptr.text_range();
        }
        fn enter_var_decl(&mut self, var: &VarDecl) {
            self.anchor = var.ptr.text_range();
        }
        fn enter_const_decl(&mut self, konst: &ConstDecl) {
            self.anchor = konst.ptr.text_range();
        }
        fn enter_expr(&mut self, _expr: &Expr) {
            self.anchors_at_expr.push(self.anchor);
        }
    }

    /// Issue #2098: [`HirVisitor::enter_var_decl`]/[`HirVisitor::enter_const_decl`]
    /// are what let a stateful visitor (one that tracks "what's the nearest
    /// diagnostic anchor" or "what are the enclosing def's locals") reset
    /// that state before a declaration's own initializer arrives, instead of
    /// inheriting whatever the *last* knot/stitch the walk visited left
    /// behind. This is the structural property the whole hand-rolled
    /// decl-initializer-walk family exists to get "for free": a visitor
    /// that implements these two hooks needs no second, hand-written walk
    /// of `hir.variables`/`hir.constants` at all — [`visit_with_decl_initializers`]
    /// drives it through the exact same `enter_expr` path the block tree
    /// uses, correctly reset every time.
    #[test]
    fn decl_hooks_reset_a_stateful_visitors_anchor_before_each_initializer() {
        // The knot comes *before* the declarations in source, so
        // `visit_with_decl_initializers`'s walk (knots/stitches, then
        // decls) visits it first — if `enter_var_decl`/`enter_const_decl`
        // didn't fire, the anchor recorded for `VAR c`/`CONST d`'s own
        // initializer would wrongly still be the knot's.
        let hir = lower_src("=== greet ===\nHello\n\nVAR c = 1\nCONST d = 2\n");
        assert_eq!(hir.variables.len(), 1);
        assert_eq!(hir.constants.len(), 1);

        let mut p = Probe {
            anchor: TextRange::new(0.into(), 0.into()),
            anchors_at_expr: Vec::new(),
        };
        visit_with_decl_initializers(&hir, &mut p);

        assert_eq!(
            p.anchors_at_expr.len(),
            2,
            "the VAR value `1` and the CONST value `2`, no more: {:?}",
            p.anchors_at_expr
        );
        let knot_range = hir.knots[0].ptr.text_range();
        assert_ne!(
            p.anchors_at_expr[0], knot_range,
            "the VAR initializer must not see the knot's leftover anchor"
        );
        assert_eq!(p.anchors_at_expr[0], hir.variables[0].ptr.text_range());
        assert_ne!(
            p.anchors_at_expr[1], knot_range,
            "the CONST initializer must not see the knot's leftover anchor"
        );
        assert_eq!(p.anchors_at_expr[1], hir.constants[0].ptr.text_range());
    }

    /// Issue #2773: [`HirVisitor::enter_lambda`]/[`HirVisitor::exit_lambda`]
    /// fire around **both** lambda body shapes, and nest correctly for a
    /// lambda literal inside another lambda's own body. Lambdas are a
    /// native-surface-only construct (`brink-syntax`, the ink-compat
    /// surface, has no lambda literal), hence `brink_syntax_native::parse` +
    /// `lower_native::lower` rather than this module's own `lower_src`.
    #[test]
    fn enter_exit_lambda_fire_around_both_body_shapes_and_nest_correctly() {
        let parsed = brink_syntax_native::parse(
            "fn f() {\n  let a = |x: int| x + 1;\n  let b = |y: int| {\n    let g = |z: int| z;\n    g(y)\n  };\n}\n",
        );
        assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
        let (hir, _manifest, _diag) = crate::hir::lower_native::lower(FileId(0), &parsed.tree());

        let mut c = Counts {
            visit_exprs: true,
            ..Default::default()
        };
        visit(&hir, &mut c);

        // Three lambda literals total: the expr-body `|x: int| x + 1`, the
        // block-body `|y: int| { ... }`, and the block-body's own nested
        // `|z: int| z`.
        assert_eq!(c.enter_lambda, 3, "expr-body + block-body + nested");
        assert_eq!(c.enter_lambda, c.exit_lambda, "enter/exit balanced");
    }
}
