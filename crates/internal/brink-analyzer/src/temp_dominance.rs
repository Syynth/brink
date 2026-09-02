//! E193 — a `~ temp` read on a path its declaration does not dominate
//! (issue #3354, RULED 2026-09-01 option C; `docs/compiler-spec.md`
//! "Temp scope and definite assignment").
//!
//! # The mistake
//!
//! A classic `~ temp` lives in its knot's call frame, so a read anywhere in
//! that frame resolves to the temp's own slot — lexically correct. What
//! resolution cannot say is whether the *declaring statement* has run by
//! the time the read does. The four shapes the ruling names:
//!
//! 1. a sibling choice branch declares it, another one reads it;
//! 2. a gather is reached from a branch that did not declare it;
//! 3. the read is written textually ahead of the declaration;
//! 4. a stitch reads a temp declared at its knot's root.
//!
//! In every one of these the C# reference prints
//! `RUNTIME WARNING: Variable not found: 'n'. Using default value of 0
//! (false).` and keeps playing, which is why the pattern reaches authors
//! as "it works fine in Inky". Brink now plays it the same way
//! (`brink_runtime::vm`'s `GetTemp` uninitialized-slot arm) — this pass is
//! the half that tells the author *before* they play.
//!
//! # How dominance is decided
//!
//! Structurally, over the HIR block tree, with no CFG construction:
//!
//! > a `~ temp` declaration `D` sitting directly in block `B` dominates
//! > exactly those reads that lie inside `B`'s own subtree and start at or
//! > after `D`'s end.
//!
//! That is sound because reaching any point in `B`'s subtree past `D`
//! means executing `B`'s statements in order through `D` first — nesting
//! (a choice set, a conditional, a labeled gather) inside `B` after `D` is
//! still behind `D`. And it is what makes all four shapes fall out of one
//! rule: a sibling choice body, the gather continuation, and every stitch
//! body are each a *different* block's subtree, so a declaration in one
//! never dominates a read in another.
//!
//! It deliberately does **not** model diverts. A `-> knot.stitch` that
//! jumps over a declaration is shape 4, already covered; a divert that
//! re-enters a gather inside the same frame after the declaration ran is
//! not a defect and is not reported.
//!
//! # What is deliberately not flagged
//!
//! - **Params.** A knot/stitch parameter is bound at call time, so a name
//!   that is also a parameter of its enclosing definition is never
//!   reported — matching `brink_ir::lir::lower::temps::alloc_temps`, which
//!   gives the parameter the slot and lets a same-named `~ temp` write
//!   through it.
//! - **Assignment targets.** `~ n = 1` writes; it does not read. Compound
//!   assignment (`~ n += 1`) and `~ n++` do read, and are left to their own
//!   ruling — this pass reports reads in expression position only.
//! - **Block-scoped (T1b) temps.** A `temp` declared inside `~ { … }` is
//!   [`brink_ir::DiagnosticCode::E082`]'s subject, a lexical-scope defect
//!   rather than a definite-assignment one; only `Stmt::TempDecl` (the
//!   classic weave-level form) is collected here.
//! - **Lambda bodies.** A lambda's own parameters shadow the enclosing
//!   frame, and this pass does not track that binding, so reads inside a
//!   lambda body are skipped rather than risked as false positives.

use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{
    AssignOp, Block, Diagnostic, DiagnosticCode, Expr, FileId, HirFile, Knot, LambdaExpr, Stmt,
};
use rowan::TextRange;

use crate::determinism::{LookupMap, LookupSet};

/// Where a frame temp was declared, for the message's declaration half.
struct DeclSite {
    /// Source range of the declaring `~ temp` statement.
    range: TextRange,
    /// The definition the declaration sits in, already spelled the way the
    /// message wants it — "knot `k`", "stitch `k.s`", or "the file's root
    /// content".
    owner: String,
}

/// One bare-name read collected from a frame's block tree.
struct Read {
    name: String,
    range: TextRange,
}

/// Run the E193 definite-assignment check over one file.
///
/// Frames are the unit: a knot (its own body plus every stitch body — they
/// share one call frame and one `TempMap`) and the file's root content.
pub fn check(file: FileId, hir: &HirFile) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    check_frame(
        file,
        &[(&hir.root_content, "the file's root content")],
        &[],
        &mut out,
    );
    for knot in &hir.knots {
        check_knot(file, knot, &mut out);
    }
    out
}

fn check_knot(file: FileId, knot: &Knot, out: &mut Vec<Diagnostic>) {
    let knot_owner = format!("knot `{}`", knot.name.text);
    let mut regions: Vec<(&Block, String)> = vec![(&knot.body, knot_owner)];
    for stitch in &knot.stitches {
        regions.push((
            &stitch.body,
            format!("stitch `{}.{}`", knot.name.text, stitch.name.text),
        ));
    }
    let borrowed: Vec<(&Block, &str)> = regions
        .iter()
        .map(|(b, owner)| (*b, owner.as_str()))
        .collect();

    let mut params: Vec<&str> = knot.params.iter().map(|p| p.name.text.as_str()).collect();
    for stitch in &knot.stitches {
        params.extend(stitch.params.iter().map(|p| p.name.text.as_str()));
    }

    check_frame(file, &borrowed, &params, out);
}

/// The shared frame check: `regions` are the blocks that share one call
/// frame, each paired with the prose naming the definition it belongs to.
fn check_frame(
    file: FileId,
    regions: &[(&Block, &str)],
    params: &[&str],
    out: &mut Vec<Diagnostic>,
) {
    // Every classic `~ temp` this frame declares, keyed by name. The first
    // declaration wins the message's declaration half — a second one of the
    // same name reuses the same slot (`alloc_temps` inserts once).
    let mut decls: LookupMap<String, DeclSite> = LookupMap::new();
    for (block, owner) in regions {
        collect_decls(block, owner, &mut decls);
    }
    if decls.is_empty() {
        return;
    }

    // Reads the declarations do dominate, by source range.
    let mut dominated: LookupSet<TextRange> = LookupSet::new();
    for (block, _) in regions {
        mark_dominated(block, &mut dominated);
    }

    let mut reads = Vec::new();
    let mut skipped = LookupSet::new();
    for (block, _) in regions {
        let mut v = ReadCollector {
            reads: &mut reads,
            skipped: &mut skipped,
            lambda_depth: 0,
        };
        visit::walk_block(block, &mut v);
    }

    for read in &reads {
        if dominated.contains(&read.range) || skipped.contains(&read.range) {
            continue;
        }
        if params.contains(&read.name.as_str()) {
            continue;
        }
        let Some(decl) = decls.get(&read.name) else {
            continue;
        };
        let name = &read.name;
        let owner = &decl.owner;
        let when = if decl.range.start() >= read.range.end() {
            "is written further down"
        } else {
            "runs on a path this read does not pass through"
        };
        out.push(Diagnostic {
            file,
            range: read.range,
            message: format!(
                "{}: `{name}` is read here, but the `~ temp {name}` that declares it \
                 (in {owner}) {when} — so the slot may still be unset, and an unset \
                 temp reads as `0`",
                DiagnosticCode::E193.title(),
            ),
            code: DiagnosticCode::E193,
        });
    }
}

/// Collect this frame's classic `~ temp` declarations, mirroring
/// `brink_ir::lir::lower::temps`'s own collection (choice bodies and
/// continuation, conditional and sequence branches, labeled blocks) so the
/// set of names matches the set of slots exactly.
fn collect_decls(block: &Block, owner: &str, out: &mut LookupMap<String, DeclSite>) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::TempDecl(decl) => {
                if !out.contains_key(&decl.name.text) {
                    out.insert(
                        decl.name.text.clone(),
                        DeclSite {
                            range: decl.ptr.text_range(),
                            owner: owner.to_owned(),
                        },
                    );
                }
            }
            Stmt::ChoiceSet(cs) => {
                for choice in &cs.choices {
                    collect_decls(&choice.body, owner, out);
                }
                collect_decls(&cs.continuation, owner, out);
            }
            Stmt::Conditional(cond) => {
                for branch in &cond.branches {
                    collect_decls(&branch.body, owner, out);
                }
            }
            Stmt::Sequence(seq) => {
                for branch in &seq.branches {
                    collect_decls(&branch.body, owner, out);
                }
            }
            Stmt::LabeledBlock(inner) => collect_decls(inner, owner, out),
            _ => {}
        }
    }
}

/// Mark every read a declaration in `block` dominates: reads inside
/// `block`'s own subtree that start at or after the declaration's end.
fn mark_dominated(block: &Block, out: &mut LookupSet<TextRange>) {
    let mut subtree: Option<Vec<Read>> = None;
    for stmt in &block.stmts {
        if let Stmt::TempDecl(decl) = stmt {
            let reads = subtree.get_or_insert_with(|| collect_subtree_reads(block));
            let decl_end = decl.ptr.text_range().end();
            for read in reads.iter() {
                if read.name == decl.name.text && read.range.start() >= decl_end {
                    out.insert(read.range);
                }
            }
        }
    }
    for child in child_blocks(block) {
        mark_dominated(child, out);
    }
}

/// Every bare-name read anywhere under `block`, including nested branches
/// and choice bodies — the subtree a declaration in `block` dominates the
/// tail of.
fn collect_subtree_reads(block: &Block) -> Vec<Read> {
    let mut reads = Vec::new();
    let mut skipped = LookupSet::new();
    let mut v = ReadCollector {
        reads: &mut reads,
        skipped: &mut skipped,
        lambda_depth: 0,
    };
    visit::walk_block(block, &mut v);
    reads
}

/// The sub-blocks of `block` that are their own dominance regions.
fn child_blocks(block: &Block) -> Vec<&Block> {
    let mut out = Vec::new();
    for stmt in &block.stmts {
        match stmt {
            Stmt::ChoiceSet(cs) => {
                for choice in &cs.choices {
                    out.push(&choice.body);
                }
                out.push(&cs.continuation);
            }
            Stmt::Conditional(cond) => out.extend(cond.branches.iter().map(|b| &b.body)),
            Stmt::Sequence(seq) => out.extend(seq.branches.iter().map(|b| &b.body)),
            Stmt::LabeledBlock(inner) => out.push(inner),
            _ => {}
        }
    }
    out
}

/// Collects bare single-segment path reads, plus the ranges that must be
/// discounted from them: plain assignment targets (writes, not reads) and
/// anything inside a lambda body.
struct ReadCollector<'a> {
    reads: &'a mut Vec<Read>,
    skipped: &'a mut LookupSet<TextRange>,
    lambda_depth: u32,
}

impl HirVisitor for ReadCollector<'_> {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_stmt(&mut self, stmt: &Stmt) {
        // `walk_stmt` walks an assignment's target as an ordinary
        // expression, so the write has to be discounted by range.
        if let Stmt::Assignment(a) = stmt
            && a.op == AssignOp::Set
            && let Expr::Path(p) = &a.target
        {
            self.skipped.insert(p.range);
        }
    }

    fn enter_lambda(&mut self, _lambda: &LambdaExpr) {
        self.lambda_depth += 1;
    }

    fn exit_lambda(&mut self, _lambda: &LambdaExpr) {
        self.lambda_depth = self.lambda_depth.saturating_sub(1);
    }

    fn enter_expr(&mut self, expr: &Expr) {
        if self.lambda_depth > 0 {
            return;
        }
        let Expr::Path(p) = expr else { return };
        let [seg] = p.segments.as_slice() else {
            return;
        };
        self.reads.push(Read {
            name: seg.text.clone(),
            range: p.range,
        });
    }
}
