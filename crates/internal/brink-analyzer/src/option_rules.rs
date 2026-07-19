//! NS-A1 Option-package declaration rule (`docs/stdlib-spec.md` §1.4,
//! issue #1107): **a bare `none` needs a type from context** — E107.
//!
//! The ruled sentence: "A bare `none` needs a type from context (concrete
//! sites fine; a fresh un-annotated `var x = none` errors — the
//! empty-collection posture)." A *declaration* site is the slot's own type
//! origin, so there is no surrounding context to take the element type
//! from — flagged here, at the exact declaration span. Every other `none`
//! position (an assignment to an existing slot, a call argument, an
//! equality operand) has context by construction and is left to the
//! ordinary inference lattice (`Ty::Option(Unknown)` joins at the use
//! site; strict mode's escape checks own the residue).
//!
//! Fired in **both** dialects and both `types` policies: the rule is part
//! of the Option package itself, not a strict-mode refinement — and under
//! `strict-ink` (where declarations aren't part of the dialect gate's
//! block-tree walk) this is also what keeps `VAR x = none` an error at
//! all, preserving the pre-Option posture where an unresolved `none`
//! reference could never compile.
//!
//! Shadowing: a `none` that *resolved* to a real user symbol (a LIST item,
//! VAR, param, …) is an ordinary reference, not the literal — the check
//! consults the resolution map exactly like the dialect gate's stdlib-call
//! check, so `LIST mood = none, happy` + `VAR m = none` stays legal
//! (E035-warned at the declaration of the shadowing symbol, not here).

use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{
    BlockStmt, Diagnostic, DiagnosticCode, ElseBranch, Expr, FileId, HirFile, IfStmt,
    ResolutionMap, Stmt,
};

use crate::determinism::LookupSet;

/// Walk every file's declarations (`VAR`/`CONST`, classic `~ temp`, and
/// block-scoped `temp`) and emit E107 for each fresh un-annotated
/// declaration whose initializer is the bare, unresolved `none` literal.
pub fn check(files: &[(FileId, &HirFile)], resolutions: &ResolutionMap) -> Vec<Diagnostic> {
    let resolved: LookupSet<(FileId, rowan::TextRange)> =
        resolutions.iter().map(|r| (r.file, r.range)).collect();
    let mut out = Vec::new();
    for &(file, hir) in files {
        let mut v = NoneDeclVisitor {
            file,
            resolved: &resolved,
            diagnostics: &mut out,
        };
        // File-level declarations aren't part of the block-tree walk
        // `visit::visit` covers — iterated directly, same pattern the
        // dialect gate uses for `VAR`/`CONST` annotations.
        for var in &hir.variables {
            if var.annotation.is_none() {
                v.check_init(&var.name.text, Some(&var.value));
            }
        }
        for c in &hir.constants {
            if c.annotation.is_none() {
                v.check_init(&c.name.text, Some(&c.value));
            }
        }
        visit::visit(hir, &mut v);
    }
    out
}

struct NoneDeclVisitor<'a> {
    file: FileId,
    resolved: &'a LookupSet<(FileId, rowan::TextRange)>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl NoneDeclVisitor<'_> {
    /// Flag `init` if it is the bare, unresolved `none` literal.
    fn check_init(&mut self, decl_name: &str, init: Option<&Expr>) {
        let Some(Expr::Path(p)) = init else { return };
        let [seg] = p.segments.as_slice() else {
            return;
        };
        if seg.text != "none" || self.resolved.contains(&(self.file, p.range)) {
            return;
        }
        self.diagnostics.push(Diagnostic {
            file: self.file,
            range: p.range,
            message: format!(
                "{}: `{decl_name}` is declared from a bare `none`, which carries no \
                 element type — initialize from `some(x)` or an Option-returning \
                 verb (`find`/`get`/`pop`/…) instead",
                DiagnosticCode::E107.title(),
            ),
            code: DiagnosticCode::E107,
        });
    }

    /// Descend a `~ { … }` block's statement list — the shared `HirVisitor`
    /// walk doesn't fire `enter_stmt` for `BlockStmt`s (T1b's closed
    /// block-statement set), so this pass descends by hand, mirroring the
    /// dialect gate's `flag_block_stmts`.
    fn check_block_stmts(&mut self, stmts: &[BlockStmt]) {
        for s in stmts {
            match s {
                BlockStmt::TempDecl(t) => {
                    if t.annotation.is_none() {
                        self.check_init(&t.name.text, t.value.as_ref());
                    }
                }
                BlockStmt::If(i) => self.check_if_stmt(i),
                BlockStmt::While(w) => self.check_block_stmts(&w.body),
                BlockStmt::For(f) => self.check_block_stmts(&f.body),
                BlockStmt::Assignment(_)
                | BlockStmt::Return(_)
                | BlockStmt::ExprStmt(_)
                | BlockStmt::Break(_)
                | BlockStmt::Continue(_)
                | BlockStmt::Await(_) => {}
            }
        }
    }

    fn check_if_stmt(&mut self, i: &IfStmt) {
        self.check_block_stmts(&i.body);
        match &i.else_branch {
            Some(ElseBranch::ElseIf(inner)) => self.check_if_stmt(inner),
            Some(ElseBranch::Else(stmts)) => self.check_block_stmts(stmts),
            None => {}
        }
    }
}

impl HirVisitor for NoneDeclVisitor<'_> {
    fn enter_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::TempDecl(t) => {
                if t.annotation.is_none() {
                    self.check_init(&t.name.text, t.value.as_ref());
                }
            }
            Stmt::LogicBlock(lb) => self.check_block_stmts(&lb.stmts),
            _ => {}
        }
    }
}
