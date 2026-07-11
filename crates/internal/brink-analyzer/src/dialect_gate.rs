//! T1b-1 dialect gate (docs/t1b-surface-spec.md §1).
//!
//! `brink-syntax` always parses the full superset grammar — multi-line
//! `~ { … }` blocks, `#[…]`/`#{…}` sigil literals, postfix indexing — and
//! `brink-ir` always lowers it to HIR (shared, dialect-agnostic prefix of the
//! pipeline). Whether those constructs are *allowed* is decided here, after
//! HIR lowering, using the caller's declared [`Dialect`]:
//!
//! - `StrictInk` (the default): every extension construct is a targeted
//!   error at its exact span — "brink extension" (`E051`).
//! - `Brink`: every extension construct is still an error in T1b-1 — nothing
//!   lowers to LIR yet (lands in T1b-2) — but with a different message
//!   (`E052`) so authors know it's a roadmap gap, not a permanent rejection.
//!
//! Per docs/t1b-surface-spec.md §1, the dialect is an authoring-time/tooling
//! input only (mirrors the #368 dialogue-dialect precedent): it is never
//! embedded in `.inkb` and never delivered to the runtime.

use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{Diagnostic, DiagnosticCode, Expr, FileId, HirFile, Stmt};

/// Compiler dialect: gates T1b brink-extension syntax. Default `StrictInk` —
/// divergence from the oracle-anchored ink subset is a visible, one-time,
/// per-project choice (docs/t1b-surface-spec.md §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dialect {
    #[default]
    StrictInk,
    Brink,
}

/// Walk every file's HIR and emit a dialect-gate diagnostic for each brink
/// extension construct found: a `~ { … }` block, a `#[…]`/`#{…}` sigil
/// literal, or postfix indexing — anywhere in the tree, not just at
/// statement top level (an extension expression can nest inside an ordinary
/// `~` line, a choice condition, a string interpolation, …).
pub fn check(files: &[(FileId, &HirFile)], dialect: Dialect) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for &(file, hir) in files {
        let mut v = GateVisitor {
            file,
            dialect,
            diagnostics: &mut out,
        };
        visit::visit(hir, &mut v);
    }
    out
}

struct GateVisitor<'a> {
    file: FileId,
    dialect: Dialect,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl GateVisitor<'_> {
    fn flag(&mut self, range: rowan::TextRange, construct: &str) {
        let (code, message) = match self.dialect {
            Dialect::StrictInk => (
                DiagnosticCode::E051,
                format!(
                    "{construct} is a brink extension — this project compiles \
                     strict ink (dialect = brink to enable)"
                ),
            ),
            Dialect::Brink => (
                DiagnosticCode::E052,
                format!("{construct} is not yet implemented — lands in T1b-2"),
            ),
        };
        self.diagnostics.push(Diagnostic {
            file: self.file,
            range,
            message,
            code,
        });
    }
}

impl HirVisitor for GateVisitor<'_> {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_stmt(&mut self, stmt: &Stmt) {
        if let Stmt::LogicBlock(lb) = stmt {
            self.flag(lb.ptr.text_range(), "`~ { … }` multi-line logic block");
        }
    }

    fn enter_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::ArrayLiteral(a) => self.flag(a.ptr.text_range(), "`#[…]` array literal"),
            Expr::MapLiteral(m) => self.flag(m.ptr.text_range(), "`#{…}` map literal"),
            Expr::Index(i) => self.flag(i.ptr.text_range(), "postfix indexing `[…]`"),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_syntax::parse;

    fn lower_src(src: &str) -> HirFile {
        let parsed = parse(src);
        let tree = parsed.tree();
        let (hir, _, _) = brink_ir::hir::lower::lower(FileId(0), &tree);
        hir
    }

    #[test]
    fn strict_ink_flags_block() {
        let hir = lower_src("~ {\ntemp x = 0\n}\n");
        let diags = check(&[(FileId(0), &hir)], Dialect::StrictInk);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E051);
        assert!(diags[0].message.contains("brink extension"));
    }

    #[test]
    fn brink_dialect_flags_block_as_not_yet_implemented() {
        let hir = lower_src("~ {\ntemp x = 0\n}\n");
        let diags = check(&[(FileId(0), &hir)], Dialect::Brink);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E052);
        assert!(diags[0].message.contains("not yet implemented"));
    }

    #[test]
    fn strict_ink_flags_array_literal_in_ordinary_logic_line() {
        // Sigil literals can appear outside a block too — nested in a plain
        // `~` line's expression.
        let hir = lower_src("~ x = #[1, 2, 3]\n");
        let diags = check(&[(FileId(0), &hir)], Dialect::StrictInk);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E051);
    }

    #[test]
    fn strict_ink_flags_map_literal() {
        let hir = lower_src("~ x = #{}\n");
        let diags = check(&[(FileId(0), &hir)], Dialect::StrictInk);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E051);
    }

    #[test]
    fn strict_ink_flags_indexing() {
        let hir = lower_src("~ x = a[0]\n");
        let diags = check(&[(FileId(0), &hir)], Dialect::StrictInk);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E051);
    }

    #[test]
    fn strict_ink_flags_indexed_assignment() {
        let hir = lower_src("~ a[0] = 5\n");
        let diags = check(&[(FileId(0), &hir)], Dialect::StrictInk);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E051);
    }

    #[test]
    fn plain_ink_produces_no_dialect_diagnostics() {
        let hir = lower_src("~ x = 5\nHello world\n");
        let diags = check(&[(FileId(0), &hir)], Dialect::StrictInk);
        assert!(diags.is_empty(), "no extension syntax used: {diags:?}");
    }

    #[test]
    fn nested_extension_inside_block_is_flagged_alongside_the_block() {
        // The block itself AND the indexing expression nested inside it each
        // get their own targeted diagnostic — "every extension construct...
        // at its span" (docs/t1b-surface-spec.md §1).
        let hir = lower_src("~ {\ntemp x = a[0]\n}\n");
        let diags = check(&[(FileId(0), &hir)], Dialect::StrictInk);
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E051));
    }
}
