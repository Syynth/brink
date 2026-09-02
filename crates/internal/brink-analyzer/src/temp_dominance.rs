//! E193 — a `~ temp` read on a path its declaration does not dominate
//! (issue #3354, RULED 2026-09-01 option C; `docs/compiler-spec.md`
//! "Temp scope and definite assignment").
//!
//! # The mistake
//!
//! A classic `~ temp` lives in its own definition's block tree, so a read
//! anywhere in that block resolves to the temp's own slot — lexically
//! correct. What resolution cannot say is whether the *declaring
//! statement* has run by the time the read does. The three shapes the
//! ruling names:
//!
//! 1. a sibling choice branch declares it, another one reads it;
//! 2. a gather is reached from a branch that did not declare it;
//! 3. the read is written textually ahead of the declaration.
//!
//! In every one of these the C# reference prints
//! `RUNTIME WARNING: Variable not found: 'n'. Using default value of 0
//! (false).` and keeps playing, which is why the pattern reaches authors
//! as "it works fine in Inky". Brink now plays it the same way
//! (`brink_runtime::vm`'s `GetTemp` uninitialized-slot arm) — this pass is
//! the half that tells the author *before* they play.
//!
//! A fourth shape the ruling originally enumerated here — a stitch reading
//! a temp declared at its knot's root — is not a dominance question at
//! all: PR #3369's review found this pass warned on a knot/stitch divert
//! that runs the declaration and then plays correctly (`-> k`, `~ temp n =
//! 7`, `-> s`, `= s`, `Stitch sees {n}.` plays `Stitch sees 7.` — no
//! defect), and the 2026-09-01 follow-up ruling on #3373 moved that shape
//! out of `E193` entirely into its own compat-deny code, `E194`
//! (`brink_analyzer::compat_deny::knot_temp_from_stitch`,
//! `docs/compiler-spec.md` "Compat-deny diagnostics") — every read in a
//! knot's stitch of a name that knot's own root declares, regardless of
//! whether the divert into the stitch ran the declaration first, since ink
//! itself never extends a knot's `~ temp` visibility into its stitches at
//! all.
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
//! still behind `D`. And it is what makes both remaining shapes fall out
//! of one rule: a sibling choice body and the gather continuation are each
//! a *different* block's subtree, so a declaration in one never dominates
//! a read in another.
//!
//! Each of a knot's root body and every one of its stitch bodies is
//! checked as its own independent region: a `~ temp` declared in one
//! region is never looked up for a read in another (that cross-region
//! case is `E194`'s subject, not this pass's). It deliberately does
//! **not** model diverts within a region either — a divert that re-enters
//! a gather inside the same block after the declaration ran is not a
//! defect and is not reported.
//!
//! # What is deliberately not flagged
//!
//! - **Params.** A knot/stitch parameter is bound at call time, so a name
//!   that is also a parameter of its enclosing definition is never
//!   reported — matching `brink_ir::lir::lower::temps::alloc_temps`, which
//!   gives the parameter the slot and lets a same-named `~ temp` write
//!   through it.
//! - **Plain assignment targets.** `~ n = 1` writes; it does not read, so
//!   `ReadCollector::enter_stmt` discounts an `AssignOp::Set` target by
//!   range. Compound assignment (`~ n += 1`) and `~ n++` DO read their
//!   target before writing it back, and this pass already reaches them —
//!   `enter_stmt` only discounts `Set`, so a non-`Set` target is walked and
//!   reported like any other read when undominated.
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

/// Where a temp was declared, for the message's declaration half.
pub(crate) struct DeclSite {
    /// Source range of the declaring `~ temp` statement.
    pub(crate) range: TextRange,
    /// The definition the declaration sits in, already spelled the way a
    /// message wants it — "knot `k`", "stitch `k.s`", or "the file's root
    /// content".
    pub(crate) owner: String,
}

/// One bare-name read collected from a block tree.
pub(crate) struct Read {
    pub(crate) name: String,
    pub(crate) range: TextRange,
}

/// Run the E193 definite-assignment check over one file.
///
/// Each region — a knot's own root body, one of its stitch bodies, or the
/// file's root content — is checked independently: a `~ temp` declared in
/// one region is never looked up for a read in another. (Runtime-wise a
/// knot and its stitches DO share one call frame and one `TempMap` —
/// `lir::lower::temps::alloc_temps` walks the whole thing — but a
/// cross-region reference is `E194`'s subject, issue #3373, not this
/// pass's: see the module doc's account of the shape this used to cover.)
///
/// `is_native` picks the message's vocabulary to match what the author
/// actually wrote (issue #3369 review: the message previously said
/// `` `~ temp n` `` and "knot" unconditionally, even for a native `.brink`
/// file spelling the same declaration `~ let n` inside a `flow`) — the
/// same flag `per_file_diagnostics` already threads to every other
/// surface-aware check.
pub fn check(file: FileId, hir: &HirFile, is_native: bool) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    check_frame(
        file,
        &hir.root_content,
        "the file's root content",
        &[],
        is_native,
        &mut out,
    );
    for knot in &hir.knots {
        check_knot(file, knot, is_native, &mut out);
    }
    out
}

fn check_knot(file: FileId, knot: &Knot, is_native: bool, out: &mut Vec<Diagnostic>) {
    let knot_noun = if is_native { "flow" } else { "knot" };
    let knot_owner = format!("{knot_noun} `{}`", knot.name.text);
    let knot_params: Vec<&str> = knot.params.iter().map(|p| p.name.text.as_str()).collect();
    check_frame(file, &knot.body, &knot_owner, &knot_params, is_native, out);

    for stitch in &knot.stitches {
        let stitch_owner = format!("stitch `{}.{}`", knot.name.text, stitch.name.text);
        let stitch_params: Vec<&str> = stitch.params.iter().map(|p| p.name.text.as_str()).collect();
        check_frame(
            file,
            &stitch.body,
            &stitch_owner,
            &stitch_params,
            is_native,
            out,
        );
    }
}

/// The per-region check: `block` is one definition's root body (a knot's,
/// one of its stitches', or the file's root content), `owner` names it the
/// way a message wants it.
fn check_frame(
    file: FileId,
    block: &Block,
    owner: &str,
    params: &[&str],
    is_native: bool,
    out: &mut Vec<Diagnostic>,
) {
    let decl_keyword = if is_native { "let" } else { "temp" };
    // Every classic `~ temp` this region declares, keyed by name. The first
    // declaration wins the message's declaration half — a second one of the
    // same name reuses the same slot (`alloc_temps` inserts once).
    let mut decls: LookupMap<String, DeclSite> = LookupMap::new();
    collect_decls(block, owner, &mut decls);
    if decls.is_empty() {
        return;
    }

    // Reads the declarations do dominate, by source range.
    let mut dominated: LookupSet<TextRange> = LookupSet::new();
    mark_dominated(block, &mut dominated);

    let mut reads = Vec::new();
    let mut skipped = LookupSet::new();
    let mut v = ReadCollector {
        reads: &mut reads,
        skipped: &mut skipped,
        lambda_depth: 0,
    };
    visit::walk_block(block, &mut v);

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
                "{}: `{name}` is read here, but the `~ {decl_keyword} {name}` that declares \
                 it (in {owner}) {when} — so the slot may still be unset, and an unset \
                 temp reads as `0`",
                DiagnosticCode::E193.title(),
            ),
            code: DiagnosticCode::E193,
        });
    }
}

/// Collect this region's classic `~ temp` declarations, mirroring
/// `brink_ir::lir::lower::temps`'s own collection (choice bodies and
/// continuation, conditional and sequence branches, labeled blocks) so the
/// set of names matches the set of slots exactly. Shared with
/// `brink_analyzer::compat_deny::knot_temp_from_stitch` (issue #3373),
/// which needs the identical "what counts as a declaration in this block"
/// answer for a knot's root and for a stitch's own body.
pub(crate) fn collect_decls(block: &Block, owner: &str, out: &mut LookupMap<String, DeclSite>) {
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
/// anything inside a lambda body. `pub(crate)` for
/// `brink_analyzer::compat_deny::knot_temp_from_stitch` (issue #3373),
/// which needs the identical "what counts as a read" answer over a
/// stitch's own body.
pub(crate) struct ReadCollector<'a> {
    pub(crate) reads: &'a mut Vec<Read>,
    pub(crate) skipped: &'a mut LookupSet<TextRange>,
    pub(crate) lambda_depth: u32,
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
