//! Per-await-site liveness → name-keyed **frame shapes**
//! (`docs/flow-suspension-spec.md` §4/§5/§11, FS-3c).
//!
//! When a flow parks at an `await`, the runtime must spill the locals that
//! *cross the park* — those declared before the suspension and still needed
//! after it (spec §5: "locals in awaiting scopes live in the flow's frame").
//! This module computes, per `await` site, the ordered set of crossing local
//! names — the **frame shape** the codegen emits into the `FrameShapes`
//! `StoryData` section (`brink_format::FrameShapeDef`) and the runtime keys its
//! spill/restore by (name-keyed, so a frame survives recompiles via the same
//! rehydration machinery as `#@was`/saves — spec §2/§7).
//!
//! ## Continuation-splitting identity (§11.1)
//!
//! Each `await` site names a synthesized **continuation container** (the tail
//! of the def after the `await`). Its identity is **stable across recompiles**
//! — `module + enclosing def + site index` (spec §11.1) — never an
//! instruction offset. This module computes the `(enclosing def, site index)`
//! half ([`ContinuationSite`]); the module/`DefinitionId` half is minted by
//! codegen when the continuation-splitting lowering lands (FS-3r). Site
//! indices are assigned in **source pre-order** within a def, so adding an
//! `await` after existing ones does not renumber the earlier sites.
//!
//! ## Soundness
//!
//! The analysis is a **sound (conservative) over-approximation**: it never
//! omits a local that is genuinely live across a park (which would drop needed
//! state), but it may occasionally include one that a fully precise
//! control-flow liveness would prove dead (harmless — the frame carries an
//! extra value). Reads reachable only through a loop back-edge are included
//! (an `await` inside a loop must preserve whatever the loop body re-reads —
//! spec §5's "awaits inside loops just work"). Pure writes *after* the park
//! (a local assigned but never read post-park) do **not** cross — their
//! pre-park value is dead. The `await` **condition's** own local reads always
//! cross: the condition is re-evaluated in the flow's context at wake
//! (spec §10.2), so its inputs must be preserved.
//!
//! ## Fence
//!
//! This is analysis only. The E052 `await` lowering fence stands (FS-3c), so
//! nothing here is wired into codegen yet and no `StoryData` carries a
//! non-empty frame-shapes table. First emission rides the continuation
//! splitting when the fence drops (FS-3r).

use std::collections::BTreeSet;

use super::types::{
    AssignOp, Assignment, Block, BlockStmt, ChoiceSet, CondKind, Conditional, Content, ContentPart,
    ElseBranch, Expr, ForStmt, HirFile, IfStmt, Sequence, Stmt, StringPart, TempDecl, WhileStmt,
};

/// The `(enclosing def, site index)` half of a continuation container's stable
/// identity (`docs/flow-suspension-spec.md` §11.1). Codegen mints the
/// `DefinitionId` (the module half) from this when the fence drops (FS-3r).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContinuationSite {
    /// The enclosing knot/stitch path (e.g. `patrol`, `patrol.wait`), or the
    /// empty string for a top-level (root-content) `await`.
    pub def_path: String,
    /// Ordinal of this `await` within its enclosing def, in source pre-order
    /// (stable: appending a new `await` never renumbers earlier ones).
    pub site_index: usize,
}

/// The frame shape for one `await` site: its stable identity plus the ordered,
/// name-keyed set of locals that cross the park.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwaitFrameShape {
    /// The continuation container's stable identity.
    pub site: ContinuationSite,
    /// The crossing locals, in declaration order (deterministic), each an
    /// interned-later local name (`~ temp x` / `for x in …`).
    pub crossing_locals: Vec<String>,
}

/// Compute the frame shape for every `await` site in a lowered HIR file.
///
/// The result is deterministic: sites are ordered by `(def_path, site_index)`
/// and each shape's `crossing_locals` are in declaration order. A file with no
/// `await` yields an empty vector (the common — and, behind the E052 fence,
/// only — case today).
#[must_use]
pub fn compute_frame_shapes(hir: &HirFile) -> Vec<AwaitFrameShape> {
    let mut out = Vec::new();
    analyze_def(&hir.root_content, "", &mut out);
    for knot in &hir.knots {
        analyze_def(&knot.body, &knot.name.text, &mut out);
        for stitch in &knot.stitches {
            analyze_def(
                &stitch.body,
                &format!("{}.{}", knot.name.text, stitch.name.text),
                &mut out,
            );
        }
    }
    out
}

/// Compute and append the frame shapes for a single def body (`def_path` names
/// the enclosing knot/stitch, empty for root content).
fn analyze_def(body: &Block, def_path: &str, out: &mut Vec<AwaitFrameShape>) {
    let mut a = Analyzer::default();
    a.walk_block(body);
    if a.awaits.is_empty() {
        return;
    }
    // Declaration order for deterministic slot ordering.
    let decl_order: Vec<String> = a.decls.iter().map(|(name, _)| name.clone()).collect();
    let local_names: BTreeSet<&String> = a.decls.iter().map(|(name, _)| name).collect();

    for site in &a.awaits {
        let mut crossing_set: BTreeSet<&String> = BTreeSet::new();
        for name in &local_names {
            // Must be declared textually before the park to be in scope there.
            let declared_before = a.decls.iter().any(|(n, pos)| n == *name && *pos < site.pos);
            if !declared_before {
                continue;
            }
            let read_after = a.reads.iter().any(|(pos, n)| n == *name && *pos > site.pos);
            let read_in_loop = site.loop_ids.iter().any(|id| {
                let (start, end) = a.loops[*id];
                a.reads
                    .iter()
                    .any(|(pos, n)| n == *name && *pos >= start && *pos < end)
            });
            let in_condition = site.cond_reads.iter().any(|n| n == *name);
            if read_after || read_in_loop || in_condition {
                crossing_set.insert(*name);
            }
        }
        let crossing_locals: Vec<String> = decl_order
            .iter()
            .filter(|name| crossing_set.contains(name))
            .cloned()
            .collect::<Vec<_>>()
            // A local may be declared more than once (shadowing across sibling
            // blocks); dedup while preserving first-declaration order.
            .into_iter()
            .fold(Vec::new(), |mut acc, name| {
                if !acc.contains(&name) {
                    acc.push(name);
                }
                acc
            });
        out.push(AwaitFrameShape {
            site: ContinuationSite {
                def_path: def_path.to_owned(),
                site_index: site.site_index,
            },
            crossing_locals,
        });
    }
}

/// One recorded `await` suspension point.
struct AwaitRec {
    /// The statement position of the park.
    pos: usize,
    /// Ordinal within the def (source pre-order).
    site_index: usize,
    /// Local names read by the `await` condition (re-evaluated at wake).
    cond_reads: Vec<String>,
    /// Ids (into [`Analyzer::loops`]) of the loops enclosing this park.
    loop_ids: Vec<usize>,
}

/// Forward, position-numbered walk collecting reads, declarations, `await`
/// sites, and loop ranges for one def.
#[derive(Default)]
struct Analyzer {
    /// Monotonic pre-order statement counter.
    pos: usize,
    /// `(statement position, single-segment name read)` for every read.
    reads: Vec<(usize, String)>,
    /// `(local name, declaration position)` for every `~ temp`/`for` binding.
    decls: Vec<(String, usize)>,
    awaits: Vec<AwaitRec>,
    /// `(start, end)` statement-position range of each loop body, by loop id.
    loops: Vec<(usize, usize)>,
    /// Active enclosing loop ids.
    loop_stack: Vec<usize>,
    /// Next `await` site ordinal.
    site_counter: usize,
}

impl Analyzer {
    /// Consume and return the next statement position.
    fn next_pos(&mut self) -> usize {
        let p = self.pos;
        self.pos += 1;
        p
    }

    fn record_reads(&mut self, expr: &Expr, pos: usize) {
        collect_reads(expr, &mut |name| self.reads.push((pos, name.clone())));
    }

    fn walk_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.walk_stmt(stmt);
        }
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        let pos = self.next_pos();
        match stmt {
            Stmt::Content(c) => self.walk_content(c, pos),
            Stmt::Divert(d) => {
                for e in &d.target.args {
                    self.record_reads(e, pos);
                }
            }
            Stmt::TunnelCall(t) => {
                for target in &t.targets {
                    for e in &target.args {
                        self.record_reads(e, pos);
                    }
                }
            }
            Stmt::ThreadStart(t) => {
                for e in &t.target.args {
                    self.record_reads(e, pos);
                }
            }
            Stmt::TempDecl(decl) => self.walk_temp_decl(decl, pos),
            Stmt::Assignment(a) => self.walk_assignment(a, pos),
            Stmt::Return(r) => {
                if let Some(e) = &r.value {
                    self.record_reads(e, pos);
                }
                for e in &r.onwards_args {
                    self.record_reads(e, pos);
                }
            }
            Stmt::ChoiceSet(cs) => self.walk_choice_set(cs),
            Stmt::LabeledBlock(b) => self.walk_block(b),
            Stmt::Conditional(c) => self.walk_conditional(c),
            Stmt::Sequence(s) => self.walk_sequence(s),
            Stmt::ExprStmt(e) => self.record_reads(e, pos),
            Stmt::EndOfLine => {}
            Stmt::LogicBlock(lb) => {
                for bs in &lb.stmts {
                    self.walk_block_stmt(bs);
                }
            }
            Stmt::Await(a) => {
                let cond_reads = a
                    .condition
                    .as_ref()
                    .map(collect_read_names)
                    .unwrap_or_default();
                if let Some(e) = &a.condition {
                    self.record_reads(e, pos);
                }
                self.record_await(pos, cond_reads);
            }
        }
    }

    fn walk_block_stmt(&mut self, bs: &BlockStmt) {
        let pos = self.next_pos();
        match bs {
            BlockStmt::TempDecl(decl) => self.walk_temp_decl(decl, pos),
            BlockStmt::Assignment(a) => self.walk_assignment(a, pos),
            BlockStmt::Return(r) => {
                if let Some(e) = &r.value {
                    self.record_reads(e, pos);
                }
                for e in &r.onwards_args {
                    self.record_reads(e, pos);
                }
            }
            BlockStmt::If(i) => self.walk_if_stmt(i, pos),
            BlockStmt::While(w) => self.walk_while_stmt(w, pos),
            BlockStmt::For(f) => self.walk_for_stmt(f, pos),
            BlockStmt::Break(_) | BlockStmt::Continue(_) => {}
            BlockStmt::ExprStmt(e) => self.record_reads(e, pos),
            BlockStmt::Await(a) => {
                let cond_reads = a
                    .condition
                    .as_ref()
                    .map(collect_read_names)
                    .unwrap_or_default();
                if let Some(e) = &a.condition {
                    self.record_reads(e, pos);
                }
                self.record_await(pos, cond_reads);
            }
        }
    }

    fn walk_temp_decl(&mut self, decl: &TempDecl, pos: usize) {
        self.decls.push((decl.name.text.clone(), pos));
        if let Some(e) = &decl.value {
            self.record_reads(e, pos);
        }
    }

    fn walk_assignment(&mut self, a: &Assignment, pos: usize) {
        // A plain `=` target is a pure write (a binding position), not a
        // read — but only when the target *is* the binding itself, i.e. a
        // single-segment `Expr::Path` (`x = value`). Targets can also lower
        // from an arbitrary `Expr` (`logic_line.rs` uses `e.lower_expr`), so
        // `~ arr[i] = x` / `~ obj.field = x` yield `Expr::Index`/
        // `Expr::FieldAccess` targets whose base/index sub-expressions
        // (`arr`, `i`) are genuine reads, not writes — skipping them would
        // drop a local that's live only as an index/field base after a park.
        // A compound `+=`/`-=` always reads the whole target too.
        let is_plain_binding =
            a.op == AssignOp::Set && matches!(&a.target, Expr::Path(p) if p.segments.len() == 1);
        if !is_plain_binding {
            self.record_reads(&a.target, pos);
        }
        self.record_reads(&a.value, pos);
    }

    fn walk_if_stmt(&mut self, i: &IfStmt, pos: usize) {
        self.record_reads(&i.condition, pos);
        for s in &i.body {
            self.walk_block_stmt(s);
        }
        match &i.else_branch {
            Some(ElseBranch::ElseIf(inner)) => {
                let else_pos = self.next_pos();
                self.walk_if_stmt(inner, else_pos);
            }
            Some(ElseBranch::Else(stmts)) => {
                for s in stmts {
                    self.walk_block_stmt(s);
                }
            }
            None => {}
        }
    }

    fn walk_while_stmt(&mut self, w: &WhileStmt, pos: usize) {
        let loop_id = self.loops.len();
        self.loops.push((pos, pos)); // end patched after the body
        self.loop_stack.push(loop_id);
        // `while await cond { … }`: the loop head IS the park (spec §3).
        if w.is_await {
            let cond_reads = collect_read_names(&w.condition);
            self.record_await(pos, cond_reads);
        }
        self.record_reads(&w.condition, pos);
        for s in &w.body {
            self.walk_block_stmt(s);
        }
        self.loop_stack.pop();
        self.loops[loop_id].1 = self.pos;
    }

    fn walk_for_stmt(&mut self, f: &ForStmt, pos: usize) {
        // The iterator binds at the loop head, in scope for the whole body.
        self.decls.push((f.var_name.text.clone(), pos));
        self.record_reads(&f.iterable, pos);
        let loop_id = self.loops.len();
        self.loops.push((pos, pos));
        self.loop_stack.push(loop_id);
        for s in &f.body {
            self.walk_block_stmt(s);
        }
        self.loop_stack.pop();
        self.loops[loop_id].1 = self.pos;
    }

    fn record_await(&mut self, pos: usize, cond_reads: Vec<String>) {
        let site_index = self.site_counter;
        self.site_counter += 1;
        self.awaits.push(AwaitRec {
            pos,
            site_index,
            cond_reads,
            loop_ids: self.loop_stack.clone(),
        });
    }

    fn walk_content(&mut self, content: &Content, pos: usize) {
        for part in &content.parts {
            match part {
                ContentPart::Interpolation(e) => self.record_reads(e, pos),
                ContentPart::InlineConditional(c) => self.walk_conditional_at(c, pos),
                ContentPart::InlineSequence(s) => self.walk_sequence(s),
                ContentPart::Text(_) | ContentPart::Glue | ContentPart::Spring => {}
            }
        }
    }

    fn walk_choice_set(&mut self, cs: &ChoiceSet) {
        for choice in &cs.choices {
            let pos = self.next_pos();
            if let Some(e) = &choice.condition {
                self.record_reads(e, pos);
            }
            for c in [
                &choice.start_content,
                &choice.bracket_content,
                &choice.inner_content,
            ]
            .into_iter()
            .flatten()
            {
                self.walk_content(c, pos);
            }
            self.walk_block(&choice.body);
        }
        self.walk_block(&cs.continuation);
    }

    fn walk_conditional(&mut self, cond: &Conditional) {
        let pos = self.next_pos();
        self.walk_conditional_at(cond, pos);
    }

    fn walk_conditional_at(&mut self, cond: &Conditional, pos: usize) {
        if let CondKind::Switch(e) = &cond.kind {
            self.record_reads(e, pos);
        }
        for branch in &cond.branches {
            if let Some(e) = &branch.condition {
                self.record_reads(e, pos);
            }
            self.walk_block(&branch.body);
        }
    }

    fn walk_sequence(&mut self, seq: &Sequence) {
        for branch in &seq.branches {
            self.walk_block(branch);
        }
    }
}

/// Collect the single-segment (candidate-local) names read by an expression,
/// invoking `sink` for each. Mirrors `super::visit::walk_expr`'s descent; a
/// multi-segment path (`knot.stitch`) is a static reference, never a local.
fn collect_reads(expr: &Expr, sink: &mut impl FnMut(&String)) {
    match expr {
        Expr::Path(p) => {
            if p.segments.len() == 1 {
                sink(&p.segments[0].text);
            }
        }
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => collect_reads(inner, sink),
        Expr::Infix(lhs, _, rhs) => {
            collect_reads(lhs, sink);
            collect_reads(rhs, sink);
        }
        Expr::Call(_path, args) => {
            for arg in args {
                collect_reads(arg, sink);
            }
        }
        Expr::String(s) => {
            for part in &s.parts {
                if let StringPart::Interpolation(e) = part {
                    collect_reads(e, sink);
                }
            }
        }
        Expr::ArrayLiteral(a) => {
            for e in &a.elements {
                collect_reads(e, sink);
            }
        }
        Expr::MapLiteral(m) => {
            for (k, v) in &m.entries {
                collect_reads(k, sink);
                collect_reads(v, sink);
            }
        }
        Expr::Index(idx) => {
            collect_reads(&idx.base, sink);
            collect_reads(&idx.index, sink);
        }
        Expr::StructLiteral(sl) => {
            for (_name, v) in &sl.fields {
                collect_reads(v, sink);
            }
        }
        Expr::FieldAccess(fa) => collect_reads(&fa.base, sink),
        Expr::FnLiteral(fl) => {
            for arg in &fl.args {
                collect_reads(arg, sink);
            }
        }
        Expr::RefArg(ra) => collect_reads(&ra.operand, sink),
        // Leaves with no local reads: literals, and static references (a
        // divert-target value / list literal names targets/items, not locals).
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::DivertTarget(_)
        | Expr::ListLiteral(_) => {}
    }
}

/// Collect the single-segment names read by an expression into a `Vec`.
fn collect_read_names(expr: &Expr) -> Vec<String> {
    let mut names = Vec::new();
    collect_reads(expr, &mut |name| names.push(name.clone()));
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FileId;
    use brink_syntax::parse;

    fn shapes(src: &str) -> Vec<AwaitFrameShape> {
        let parsed = parse(src);
        let tree = parsed.tree();
        let (hir, _, _) = crate::hir::lower::lower(FileId(0), &tree);
        compute_frame_shapes(&hir)
    }

    /// A story with no `await` synthesizes no frame shapes (the common — and,
    /// behind the E052 fence, only — case).
    #[test]
    fn no_await_no_shapes() {
        assert!(shapes("Hello.\n=== knot ===\nHi {name}\n-> END\n").is_empty());
    }

    /// A local read *after* the park (here in trailing narrative) crosses it.
    #[test]
    fn local_read_after_park_crosses() {
        let s =
            shapes("=== patrol ===\n~ temp x = 5\n~ await x > 3\nGuard has {x} left.\n-> END\n");
        assert_eq!(s.len(), 1, "one await site");
        assert_eq!(s[0].site.def_path, "patrol");
        assert_eq!(s[0].site.site_index, 0);
        assert_eq!(s[0].crossing_locals, vec!["x".to_owned()]);
    }

    /// A local that is only assigned *before* the park and never read again
    /// does not cross — its pre-park value is dead.
    #[test]
    fn local_dead_after_park_does_not_cross() {
        // `y` is read only before the await; `x` only feeds the condition.
        let s = shapes(
            "=== patrol ===\n~ temp y = 1\n~ temp x = y + 1\n~ await x > 0\nDone.\n-> END\n",
        );
        assert_eq!(s.len(), 1);
        // `x` crosses (it feeds the re-evaluated condition); `y` does not
        // (read only before the park, never after).
        assert_eq!(s[0].crossing_locals, vec!["x".to_owned()]);
    }

    /// The `await` **condition**'s own local reads always cross — the
    /// condition is re-evaluated in the flow's context at wake (spec §10.2).
    #[test]
    fn condition_reads_cross_even_without_later_use() {
        let s = shapes("=== gate ===\n~ temp g = 10\n~ await g > 100\n-> END\n");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].crossing_locals, vec!["g".to_owned()]);
    }

    /// A `for` iterator read after an `await` *inside* the loop crosses the
    /// park — the back-edge re-reads it (spec §5: iterators included).
    #[test]
    fn loop_iterator_crosses_park_inside_loop() {
        let s = shapes(
            "=== sweep ===\n~ {\n  for room in rooms {\n    await ready\n    visit(room)\n  }\n}\n-> END\n",
        );
        assert_eq!(s.len(), 1, "one await inside the loop");
        assert!(
            s[0].crossing_locals.contains(&"room".to_owned()),
            "the for-iterator crosses: {:?}",
            s[0].crossing_locals
        );
    }

    /// `while await` records a park at the loop head.
    #[test]
    fn while_await_records_a_site() {
        let s = shapes(
            "=== ambient ===\n~ {\n  temp n = 0\n  while await alarm {\n    n = n + 1\n  }\n}\n-> END\n",
        );
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].site.def_path, "ambient");
        // `n` is read+written in the loop body, reachable after the park via
        // the back-edge → it crosses.
        assert!(
            s[0].crossing_locals.contains(&"n".to_owned()),
            "loop-carried local crosses: {:?}",
            s[0].crossing_locals
        );
    }

    /// Site indices are assigned in source pre-order and are stable per def.
    #[test]
    fn multiple_sites_numbered_in_order() {
        let s = shapes(
            "=== twostep ===\n~ temp a = 1\n~ await a > 0\n~ temp b = 2\n~ await b > 0\nEnd {a} {b}\n-> END\n",
        );
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].site.site_index, 0);
        assert_eq!(s[1].site.site_index, 1);
        // At the first park `b` is not yet declared, so it cannot cross there.
        assert!(!s[0].crossing_locals.contains(&"b".to_owned()));
        // At the second park both are read in the trailing content.
        assert!(s[1].crossing_locals.contains(&"a".to_owned()));
        assert!(s[1].crossing_locals.contains(&"b".to_owned()));
    }

    /// Frame shapes are stitch-qualified in their `def_path`.
    #[test]
    fn stitch_def_path_is_qualified() {
        let s = shapes("=== knot ===\n= inner\n~ temp x = 1\n~ await x > 0\nGot {x}\n-> END\n");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].site.def_path, "knot.inner");
    }

    /// A local used *only* as the index of an indexed `Set` after a park
    /// still crosses — the assignment target `arr[i]` is not itself a
    /// binding position; `arr` and `i` are genuine reads (the base and the
    /// index), only the indexed cell is written. Dropping them would lose
    /// state on wake (spec's soundness invariant: never omit a genuinely
    /// live local).
    #[test]
    fn index_target_base_and_index_cross_after_park() {
        let s = shapes(
            "=== task ===\n~ temp arr = #[1, 2, 3]\n~ temp i = 0\n~ await ready\n~ arr[i] = 99\n-> END\n",
        );
        assert_eq!(s.len(), 1);
        assert!(
            s[0].crossing_locals.contains(&"arr".to_owned()),
            "index base must cross: {:?}",
            s[0].crossing_locals
        );
        assert!(
            s[0].crossing_locals.contains(&"i".to_owned()),
            "index expression must cross: {:?}",
            s[0].crossing_locals
        );
    }

    /// Same shape but for a `FieldAccess` target: a bare `ident.ident`
    /// assignment target lowers as a (multi-segment, out of scope for local
    /// liveness) `Expr::Path`, so a genuine `Expr::FieldAccess` target only
    /// arises with a non-`Path` base — `arr[i].field = x` (grammar note in
    /// `brink-syntax`'s `indexable_lvalue`). Both `arr` and `i` are reads
    /// (the indexed base), not writes, so they must cross a preceding park.
    #[test]
    fn field_access_target_index_base_crosses_after_park() {
        let s = shapes(
            "=== task ===\n~ temp arr = #[1, 2, 3]\n~ temp i = 0\n~ await ready\n~ arr[i].field = 99\n-> END\n",
        );
        assert_eq!(s.len(), 1);
        assert!(
            s[0].crossing_locals.contains(&"arr".to_owned()),
            "field-access's index base must cross: {:?}",
            s[0].crossing_locals
        );
        assert!(
            s[0].crossing_locals.contains(&"i".to_owned()),
            "field-access's index expression must cross: {:?}",
            s[0].crossing_locals
        );
    }
}
