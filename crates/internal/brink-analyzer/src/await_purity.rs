//! FS-2 `await`-condition **purity gate** (docs/flow-suspension-spec.md
//! §3/§5, issue #928), built on the effects machinery (#859).
//!
//! An `await <cond>` / `while await <cond>` condition is captured as a
//! compiler-synthesized *pure* function (spec §5): the runtime re-evaluates it
//! to decide whether to wake, so it must be **read-only**. Reads are the wake
//! map's dependency set and are fine; a transitive **write** to a global cell,
//! or an effectful host **call**, makes re-evaluation itself observable, which
//! the wake contract forbids — that is the sole thing this pass rejects
//! (`E105`).
//!
//! The condition's effect is computed from the whole-project effect-row table
//! ([`crate::infer::effects_project`], already transitively closed): every
//! call in the condition is resolved through the same [`ResolutionMap`] every
//! other reference uses, and its callee's row is consulted. A call to a
//! pure knot/stitch contributes an empty row (fine); a call to one that writes
//! a global or performs an effectful call carries that through; a direct
//! `EXTERNAL` call is itself a call-atom (not read-only). A bare fn-value
//! *reference* used as a dynamic condition (`await some_fn_value`, no call
//! syntax) contributes no call atom and is read-only by construction — spec §3
//! lists it as a valid form, so it is never flagged.
//!
//! Brink-only, same posture as the other effect passes: under strict-ink the
//! whole `await` is already rejected (`E051`), so critiquing its condition
//! would be noise.

use std::collections::BTreeMap;

use brink_format::DefinitionId;
use brink_ir::{
    Block, BlockStmt, Diagnostic, DiagnosticCode, Expr, FileId, HirFile, ResolutionMap, Stmt,
    SymbolIndex, SymbolKind,
};
use rowan::TextRange;

use crate::infer::EffectRow;

/// Check every `await` condition in `hir` against the whole-project effect
/// rows `rows` (docs/flow-suspension-spec.md §3/§5). Returns an `E105` for
/// each condition that is not effect-free (read-only).
#[must_use]
pub fn check(
    file: FileId,
    hir: &HirFile,
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    rows: &BTreeMap<DefinitionId, EffectRow>,
) -> Vec<Diagnostic> {
    // This file's use-site → definition map, keyed by range (the same shape
    // the effect harvester builds).
    let by_range: BTreeMap<(u32, u32), DefinitionId> = resolutions
        .iter()
        .filter(|r| r.file == file)
        .map(|r| ((r.range.start().into(), r.range.end().into()), r.target))
        .collect();

    let ctx = Ctx {
        index,
        rows,
        by_range,
    };
    let mut sites: Vec<AwaitSite<'_>> = Vec::new();
    for knot in &hir.knots {
        collect_block(&knot.body, &mut sites);
        for stitch in &knot.stitches {
            collect_block(&stitch.body, &mut sites);
        }
    }

    let mut out = Vec::new();
    for site in sites {
        if ctx.condition_is_effectful(site.condition) {
            out.push(Diagnostic {
                file,
                range: site.range,
                message: format!(
                    "{}: an `await` suspension point re-evaluates its condition to decide when \
                     to wake, so the condition must be read-only (docs/flow-suspension-spec.md \
                     §5)",
                    DiagnosticCode::E105.title(),
                ),
                code: DiagnosticCode::E105,
            });
        }
    }
    out
}

/// Cheap structural scan: does any knot/stitch body in `hir` contain an
/// `await` suspension point? The laziness gate for the whole-project purity
/// pass — an await-free project never triggers effect inference for this
/// pass, mirroring the `#@effects` exceedance pass's own laziness gate.
#[must_use]
pub fn hir_has_await(hir: &HirFile) -> bool {
    let mut sites = Vec::new();
    for knot in &hir.knots {
        collect_block(&knot.body, &mut sites);
        for stitch in &knot.stitches {
            collect_block(&stitch.body, &mut sites);
        }
        if !sites.is_empty() {
            return true;
        }
    }
    !sites.is_empty()
}

/// Every [`DefinitionId`] called (directly) from any `await` condition in
/// `hir`, resolved through `resolutions`. The salsa path
/// (`brink-db`'s `await_purity_diagnostics_query`) uses this to fetch exactly
/// those defs' memoized per-def effect rows — the incremental analogue of the
/// monolithic path handing [`check`] the whole-project [`crate::infer::effects_project`]
/// table. A callee that resolves to a non-inferable symbol (an `EXTERNAL`, a
/// VAR fn-value) simply has no per-def row; [`check`] handles it via the same
/// resolution it does here, so the two paths agree.
#[must_use]
pub fn condition_callees(
    file: FileId,
    hir: &HirFile,
    resolutions: &ResolutionMap,
) -> std::collections::BTreeSet<DefinitionId> {
    let by_range: BTreeMap<(u32, u32), DefinitionId> = resolutions
        .iter()
        .filter(|r| r.file == file)
        .map(|r| ((r.range.start().into(), r.range.end().into()), r.target))
        .collect();

    let mut sites: Vec<AwaitSite<'_>> = Vec::new();
    for knot in &hir.knots {
        collect_block(&knot.body, &mut sites);
        for stitch in &knot.stitches {
            collect_block(&stitch.body, &mut sites);
        }
    }

    let mut out = std::collections::BTreeSet::new();
    for site in sites {
        collect_call_callees(site.condition, &by_range, &mut out);
    }
    out
}

fn collect_call_callees(
    expr: &Expr,
    by_range: &BTreeMap<(u32, u32), DefinitionId>,
    out: &mut std::collections::BTreeSet<DefinitionId>,
) {
    match expr {
        Expr::Call(path, args) => {
            let key = (path.range.start().into(), path.range.end().into());
            if let Some(&def) = by_range.get(&key) {
                out.insert(def);
            }
            for a in args {
                collect_call_callees(a, by_range, out);
            }
        }
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => {
            collect_call_callees(inner, by_range, out);
        }
        Expr::Infix(l, _, r) => {
            collect_call_callees(l, by_range, out);
            collect_call_callees(r, by_range, out);
        }
        Expr::Index(idx) => {
            collect_call_callees(&idx.base, by_range, out);
            collect_call_callees(&idx.index, by_range, out);
        }
        Expr::FieldAccess(fa) => collect_call_callees(&fa.base, by_range, out),
        Expr::ArrayLiteral(a) => {
            for e in &a.elements {
                collect_call_callees(e, by_range, out);
            }
        }
        Expr::MapLiteral(m) => {
            for (k, v) in &m.entries {
                collect_call_callees(k, by_range, out);
                collect_call_callees(v, by_range, out);
            }
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Null
        | Expr::Path(_)
        | Expr::DivertTarget(_)
        | Expr::ListLiteral(_)
        | Expr::StructLiteral(_)
        | Expr::FnLiteral(_)
        | Expr::RefArg(_) => {}
    }
}

/// One `await`/`while await` site: the statement range (the diagnostic
/// anchor) and its condition expression.
struct AwaitSite<'a> {
    range: TextRange,
    condition: &'a Expr,
}

struct Ctx<'a> {
    index: &'a SymbolIndex,
    rows: &'a BTreeMap<DefinitionId, EffectRow>,
    by_range: BTreeMap<(u32, u32), DefinitionId>,
}

impl Ctx<'_> {
    /// Whether `cond` performs any non-read-only effect: a transitive write,
    /// an effectful/opaque call, or a direct `EXTERNAL` call.
    fn condition_is_effectful(&self, cond: &Expr) -> bool {
        let mut effectful = false;
        self.walk_expr(cond, &mut effectful);
        effectful
    }

    fn walk_expr(&self, expr: &Expr, effectful: &mut bool) {
        if *effectful {
            return; // short-circuit — one violation is enough.
        }
        match expr {
            Expr::Call(path, args) => {
                if self.call_is_effectful(path.range) {
                    *effectful = true;
                    return;
                }
                for a in args {
                    self.walk_expr(a, effectful);
                }
            }
            Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => self.walk_expr(inner, effectful),
            Expr::Infix(l, _, r) => {
                self.walk_expr(l, effectful);
                self.walk_expr(r, effectful);
            }
            Expr::Index(idx) => {
                self.walk_expr(&idx.base, effectful);
                self.walk_expr(&idx.index, effectful);
            }
            Expr::FieldAccess(fa) => self.walk_expr(&fa.base, effectful),
            Expr::ArrayLiteral(a) => {
                for e in &a.elements {
                    self.walk_expr(e, effectful);
                }
            }
            Expr::MapLiteral(m) => {
                for (k, v) in &m.entries {
                    self.walk_expr(k, effectful);
                    self.walk_expr(v, effectful);
                }
            }
            // Leaves and other expression kinds carry no call atoms of their
            // own (a bare `Path` — including a fn-value reference used as a
            // dynamic condition — is read-only by construction, spec §3).
            Expr::Int(_)
            | Expr::Float(_)
            | Expr::Bool(_)
            | Expr::String(_)
            | Expr::Null
            | Expr::Path(_)
            | Expr::DivertTarget(_)
            | Expr::ListLiteral(_)
            | Expr::StructLiteral(_)
            | Expr::FnLiteral(_)
            | Expr::RefArg(_) => {}
        }
    }

    /// Whether a call whose callee is at `range` performs a non-read-only
    /// effect. Resolves the callee through the resolution map: a knot/stitch
    /// callee is judged by its (transitively closed) effect row; a direct
    /// `EXTERNAL` callee is a call-atom and therefore not read-only; an
    /// unresolved callee (already an error elsewhere) or a fn-value call is
    /// left to the LIR fence and not double-reported here.
    fn call_is_effectful(&self, range: TextRange) -> bool {
        let key = (range.start().into(), range.end().into());
        let Some(&def) = self.by_range.get(&key) else {
            return false;
        };
        if let Some(row) = self.rows.get(&def) {
            return row.opaque || !row.writes.is_empty() || !row.calls.is_empty();
        }
        // Not an inferable knot/stitch — is it a declared EXTERNAL?
        matches!(
            self.index.symbols.get(&def).map(|s| s.kind),
            Some(SymbolKind::External)
        )
    }
}

fn collect_block<'a>(block: &'a Block, out: &mut Vec<AwaitSite<'a>>) {
    for stmt in &block.stmts {
        collect_stmt(stmt, out);
    }
}

fn collect_stmt<'a>(stmt: &'a Stmt, out: &mut Vec<AwaitSite<'a>>) {
    match stmt {
        Stmt::Await(a) => {
            if let Some(cond) = &a.condition {
                out.push(AwaitSite {
                    range: a.ptr.text_range(),
                    condition: cond,
                });
            }
        }
        Stmt::LogicBlock(lb) => {
            for bs in &lb.stmts {
                collect_block_stmt(bs, out);
            }
        }
        Stmt::ChoiceSet(cs) => {
            for choice in &cs.choices {
                collect_block(&choice.body, out);
            }
            collect_block(&cs.continuation, out);
        }
        Stmt::LabeledBlock(b) => collect_block(b, out),
        Stmt::Conditional(c) => {
            for branch in &c.branches {
                collect_block(&branch.body, out);
            }
        }
        Stmt::Sequence(s) => {
            for branch in &s.branches {
                collect_block(branch, out);
            }
        }
        Stmt::Content(_)
        | Stmt::Divert(_)
        | Stmt::TunnelCall(_)
        | Stmt::ThreadStart(_)
        | Stmt::TempDecl(_)
        | Stmt::Assignment(_)
        | Stmt::Return(_)
        | Stmt::ExprStmt(_)
        | Stmt::EndOfLine => {}
    }
}

fn collect_block_stmt<'a>(bs: &'a BlockStmt, out: &mut Vec<AwaitSite<'a>>) {
    match bs {
        BlockStmt::Await(a) => {
            if let Some(cond) = &a.condition {
                out.push(AwaitSite {
                    range: a.ptr.text_range(),
                    condition: cond,
                });
            }
        }
        BlockStmt::While(w) => {
            if w.is_await {
                out.push(AwaitSite {
                    range: w.ptr.text_range(),
                    condition: &w.condition,
                });
            }
            for s in &w.body {
                collect_block_stmt(s, out);
            }
        }
        BlockStmt::If(i) => collect_if(i, out),
        BlockStmt::For(f) => {
            for s in &f.body {
                collect_block_stmt(s, out);
            }
        }
        BlockStmt::TempDecl(_)
        | BlockStmt::Assignment(_)
        | BlockStmt::Return(_)
        | BlockStmt::ExprStmt(_)
        | BlockStmt::Break(_)
        | BlockStmt::Continue(_) => {}
    }
}

fn collect_if<'a>(i: &'a brink_ir::IfStmt, out: &mut Vec<AwaitSite<'a>>) {
    for s in &i.body {
        collect_block_stmt(s, out);
    }
    match &i.else_branch {
        Some(brink_ir::ElseBranch::ElseIf(inner)) => collect_if(inner, out),
        Some(brink_ir::ElseBranch::Else(stmts)) => {
            for s in stmts {
                collect_block_stmt(s, out);
            }
        }
        None => {}
    }
}
