//! T1b dialect gate (docs/t1b-surface-spec.md §1).
//!
//! `brink-syntax` always parses the full superset grammar — multi-line
//! `~ { … }` blocks, `#[…]`/`#{…}` sigil literals, postfix indexing — and
//! `brink-ir` always lowers it to HIR (shared, dialect-agnostic prefix of the
//! pipeline). Whether those constructs are *allowed* is decided here, after
//! HIR lowering, using the caller's declared [`Dialect`]:
//!
//! - `StrictInk` (the default): every extension construct is a targeted
//!   error at its exact span — "brink extension" (`E051`). This is the
//!   *only* strict-ink enforcement — like every other suppressible analysis
//!   diagnostic in this codebase, `// brink-disable-all` bypasses it, and a
//!   suppressed strict-ink project simply compiles the construct as brink
//!   dialect would (LIR lowering doesn't consult the dialect at all).
//! - `Brink`: every extension construct lowers to LIR since T1b-2 (#570) —
//!   `E052` ("not yet implemented") no longer fires for any construct this
//!   gate recognizes; nothing is flagged under `Brink` at all.
//!
//! Per docs/t1b-surface-spec.md §1, the dialect is an authoring-time/tooling
//! input only (mirrors the #368 dialogue-dialect precedent): it is never
//! embedded in `.inkb` and never delivered to the runtime.
//!
//! T1b-3 (docs/t1b-surface-spec.md §5) extends this gate to the stdlib slice
//! 1 lowercase free functions (`len`/`keys`/`values`/`contains`/`push`/
//! `insert`/`remove`). Unlike blocks/sigils/indexing, a *call* to one of
//! these names isn't self-evidently extension syntax — `len(x)` parses
//! identically whether `len` means the builtin or an author's own knot — so
//! this gate needs the resolution result too: a call that resolved to a
//! real symbol is an ordinary (possibly shadowing) function call, never
//! flagged; a call that didn't resolve is only valid because
//! `brink-analyzer::resolve` silently treats these seven names as the
//! builtin regardless of dialect (mirroring how LIR lowering always
//! succeeds for blocks/sigils/indexing too) — so *this* gate is where
//! `strict-ink` rejection actually happens for them.

use std::collections::HashSet;

use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{
    BlockStmt, Diagnostic, DiagnosticCode, ElseBranch, Expr, FileId, HirFile, IfStmt, Knot,
    ResolutionMap, Stitch, Stmt,
};

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
/// literal, postfix indexing, or an unresolved call to a T1b stdlib slice 1
/// name (§5) — anywhere in the tree, not just at statement top level (an
/// extension expression can nest inside an ordinary `~` line, a choice
/// condition, a string interpolation, …).
///
/// `resolutions` is the project's already-computed resolution result
/// (`brink_analyzer::resolve`/`analyze_with_options` runs resolution before
/// this gate) — needed only for the stdlib-call check, which can't tell
/// "the builtin" from "an author's own function of the same name" from
/// syntax alone.
pub fn check(
    files: &[(FileId, &HirFile)],
    resolutions: &ResolutionMap,
    dialect: Dialect,
) -> Vec<Diagnostic> {
    let resolved: HashSet<(FileId, rowan::TextRange)> =
        resolutions.iter().map(|r| (r.file, r.range)).collect();
    let mut out = Vec::new();
    for &(file, hir) in files {
        let mut v = GateVisitor {
            file,
            dialect,
            resolved: &resolved,
            diagnostics: &mut out,
        };
        visit::visit(hir, &mut v);

        // TM-2 (docs/typed-mode-spec.md §3): `VAR name: type = expr`. File-level
        // declarations aren't part of the block-tree walk `visit::visit`
        // covers (see its module doc) — iterated directly here, same
        // pattern `signature.rs`/`infer::collect_globals` use.
        for var in &hir.variables {
            if let Some(ann) = &var.annotation {
                v.flag(ann.range(), "type annotation");
            }
        }
    }
    out
}

struct GateVisitor<'a> {
    file: FileId,
    dialect: Dialect,
    resolved: &'a HashSet<(FileId, rowan::TextRange)>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl GateVisitor<'_> {
    /// Flag `construct` under `StrictInk` (E051); a no-op under `Brink` —
    /// every construct this gate recognizes lowers to LIR since T1b-2
    /// (#570), so there's nothing left to reject as "not yet implemented".
    fn flag(&mut self, range: rowan::TextRange, construct: &str) {
        if self.dialect != Dialect::StrictInk {
            return;
        }
        self.diagnostics.push(Diagnostic {
            file: self.file,
            range,
            message: format!(
                "{construct} is a brink extension — this project compiles \
                 strict ink (dialect = brink to enable)"
            ),
            code: DiagnosticCode::E051,
        });
    }

    /// Flag every param's type annotation (TM-2, docs/typed-mode-spec.md §3:
    /// `name: type`), if present.
    fn flag_params(&mut self, params: &[brink_ir::Param]) {
        for p in params {
            if let Some(ann) = &p.annotation {
                self.flag(ann.range(), "type annotation");
            }
        }
    }

    /// Flag every `~ temp name: type = expr` ascription inside a `~ { … }`
    /// block, recursing through `if`/`while`/`for` bodies — the shared
    /// `HirVisitor` walk doesn't fire `enter_stmt` for `BlockStmt`s (T1b's
    /// closed block-statement set, docs/t1b-surface-spec.md §2), so this
    /// gate descends by hand.
    fn flag_block_stmts(&mut self, stmts: &[BlockStmt]) {
        for s in stmts {
            match s {
                BlockStmt::TempDecl(t) => {
                    if let Some(ann) = &t.annotation {
                        self.flag(ann.range(), "type annotation");
                    }
                }
                BlockStmt::If(i) => self.flag_if_stmt(i),
                BlockStmt::While(w) => self.flag_block_stmts(&w.body),
                BlockStmt::For(f) => self.flag_block_stmts(&f.body),
                BlockStmt::Assignment(_)
                | BlockStmt::Return(_)
                | BlockStmt::ExprStmt(_)
                | BlockStmt::Break(_)
                | BlockStmt::Continue(_) => {}
            }
        }
    }

    fn flag_if_stmt(&mut self, i: &IfStmt) {
        self.flag_block_stmts(&i.body);
        match &i.else_branch {
            Some(ElseBranch::ElseIf(inner)) => self.flag_if_stmt(inner),
            Some(ElseBranch::Else(stmts)) => self.flag_block_stmts(stmts),
            None => {}
        }
    }
}

/// T1b stdlib slice 1 function names (`docs/t1b-surface-spec.md` §5). Kept
/// in sync by hand with `resolve::is_t1b_stdlib_name` — same name, same
/// list, different call site (that one gates resolution; this one gates
/// `strict-ink`), not worth a shared constant across the two passes for
/// seven literals.
fn is_t1b_stdlib_call_name(name: &str) -> bool {
    crate::resolve::is_t1b_stdlib_name(name)
}

impl HirVisitor for GateVisitor<'_> {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_knot(&mut self, knot: &Knot) {
        self.flag_params(&knot.params);
        if let Some(ret) = &knot.return_type {
            self.flag(ret.range(), "type annotation");
        }
    }

    fn enter_stitch(&mut self, stitch: &Stitch) {
        self.flag_params(&stitch.params);
    }

    fn enter_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::LogicBlock(lb) => {
                self.flag(lb.ptr.text_range(), "`~ { … }` multi-line logic block");
                self.flag_block_stmts(&lb.stmts);
            }
            Stmt::TempDecl(t) => {
                if let Some(ann) = &t.annotation {
                    self.flag(ann.range(), "type annotation");
                }
            }
            _ => {}
        }
    }

    fn enter_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::ArrayLiteral(a) => self.flag(a.ptr.text_range(), "`#[…]` array literal"),
            Expr::MapLiteral(m) => self.flag(m.ptr.text_range(), "`#{…}` map literal"),
            Expr::Call(path, _args) => {
                let Some(name) = path.segments.first().map(|s| s.text.as_str()) else {
                    return;
                };
                if is_t1b_stdlib_call_name(name)
                    && !self.resolved.contains(&(self.file, path.range))
                {
                    self.flag(path.range, &format!("`{name}` stdlib function"));
                }
            }
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

    fn no_resolutions() -> ResolutionMap {
        ResolutionMap::new()
    }

    #[test]
    fn strict_ink_flags_block() {
        let hir = lower_src("~ {\ntemp x = 0\n}\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E051);
        assert!(diags[0].message.contains("brink extension"));
    }

    #[test]
    fn brink_dialect_does_not_flag_block_since_it_lowers_in_t1b_2() {
        let hir = lower_src("~ {\ntemp x = 0\n}\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::Brink);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn strict_ink_flags_array_literal_in_ordinary_logic_line() {
        // Sigil literals can appear outside a block too — nested in a plain
        // `~` line's expression.
        let hir = lower_src("~ x = #[1, 2, 3]\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E051);
    }

    #[test]
    fn strict_ink_flags_map_literal() {
        let hir = lower_src("~ x = #{}\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E051);
    }

    #[test]
    fn strict_ink_flags_indexing() {
        let hir = lower_src("~ x = a[0]\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E051);
    }

    #[test]
    fn strict_ink_flags_indexed_assignment() {
        let hir = lower_src("~ a[0] = 5\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E051);
    }

    #[test]
    fn plain_ink_produces_no_dialect_diagnostics() {
        let hir = lower_src("~ x = 5\nHello world\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert!(diags.is_empty(), "no extension syntax used: {diags:?}");
    }

    #[test]
    fn nested_extension_inside_block_is_flagged_alongside_the_block() {
        // The block itself AND the indexing expression nested inside it each
        // get their own targeted diagnostic — "every extension construct...
        // at its span" (docs/t1b-surface-spec.md §1).
        let hir = lower_src("~ {\ntemp x = a[0]\n}\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E051));
    }

    // ── T1b-3 stdlib slice 1 (docs/t1b-surface-spec.md §5) ────────────────

    #[test]
    fn strict_ink_flags_unresolved_stdlib_call() {
        // No `len` knot defined anywhere — this can only be the builtin,
        // which strict-ink never sees.
        let hir = lower_src("~ x = len(a)\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E051);
        assert!(diags[0].message.contains("len"));
    }

    #[test]
    fn brink_dialect_does_not_flag_unresolved_stdlib_call() {
        let hir = lower_src("~ x = len(a)\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::Brink);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn resolved_stdlib_name_call_is_never_flagged_in_either_dialect() {
        // Simulates the shadowing case: an author-defined `len` knot
        // resolved this call site, so it's an ordinary function call, not
        // the builtin — never brink-extension syntax regardless of dialect.
        let hir = lower_src("~ x = len(a)\n");
        let Some(Stmt::Assignment(assign)) = hir.root_content.stmts.first() else {
            unreachable!("lower_src(\"~ x = len(a)\\n\") always lowers to an Assignment")
        };
        let Expr::Call(path, _) = &assign.value else {
            unreachable!("assignment value is always the len(a) call")
        };
        let call_range = path.range;
        let resolutions = vec![brink_ir::ResolvedRef {
            file: FileId(0),
            range: call_range,
            target: brink_format::DefinitionId::new(brink_format::DefinitionTag::Address, 1),
        }];
        let diags = check(&[(FileId(0), &hir)], &resolutions, Dialect::StrictInk);
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── TM-2 inline type annotations (docs/typed-mode-spec.md §3) ──────

    #[test]
    fn strict_ink_flags_param_annotation() {
        let hir = lower_src("=== heal(hp: int) ===\n~ return hp\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E051);
    }

    #[test]
    fn strict_ink_flags_return_type_annotation() {
        let hir = lower_src("=== function heal(hp): int ===\n~ return hp\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E051);
    }

    #[test]
    fn strict_ink_flags_both_param_and_return_annotations_separately() {
        let hir = lower_src("=== function heal(hp: int): int ===\n~ return hp\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E051));
    }

    #[test]
    fn strict_ink_flags_var_annotation() {
        let hir = lower_src("VAR gold: int = 100\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E051);
    }

    #[test]
    fn strict_ink_flags_temp_ascription() {
        let hir = lower_src("~ temp name: string = \"a\"\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E051);
    }

    #[test]
    fn strict_ink_flags_temp_ascription_inside_a_block() {
        let hir = lower_src("~ {\ntemp x: int = 1\n}\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        // The block itself AND the nested ascription are each flagged.
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E051));
    }

    #[test]
    fn brink_dialect_does_not_flag_type_annotations() {
        let hir = lower_src("=== function heal(hp: int): int ===\n~ return hp\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::Brink);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unannotated_declarations_produce_no_type_diagnostics() {
        let hir = lower_src("=== heal(hp) ===\nVAR gold = 100\n~ temp t = 1\n~ return hp\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert!(diags.is_empty(), "{diags:?}");
    }
}
