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
//! `insert`/`remove`); the TM-3-completion pure conversion intrinsics
//! `int`/`float`/`string` (docs/typed-mode-spec.md §4, issue #659) ride the
//! same mechanism, "per the stdlib slice-1 pattern". Unlike blocks/sigils/
//! indexing, a *call* to one of these names isn't self-evidently extension
//! syntax — `len(x)` parses identically whether `len` means the builtin or
//! an author's own knot — so this gate needs the resolution result too: a
//! call that resolved to a real symbol is an ordinary (possibly shadowing)
//! function call, never flagged; a call that didn't resolve is only valid
//! because `brink-analyzer::resolve` silently treats these ten names as the
//! builtin regardless of dialect (mirroring how LIR lowering always
//! succeeds for blocks/sigils/indexing too) — so *this* gate is where
//! `strict-ink` rejection actually happens for them.

use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{
    BlockStmt, Diagnostic, DiagnosticCode, ElseBranch, Expr, FileId, HirFile, IfStmt, Knot,
    ResolutionMap, Stitch, Stmt,
};

use crate::determinism::LookupSet;

// `Dialect` is defined in `brink-project-config` — it is a project-policy
// type, more primitive than the analyzer that consumes it, and keeping it
// there is what lets that crate publish without depending on this one
// (#1234). Re-exported here so every existing `brink_analyzer::Dialect` /
// `dialect_gate::Dialect` path keeps working unchanged.
pub use brink_project_config::Dialect;

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
    let resolved: LookupSet<(FileId, rowan::TextRange)> =
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

        // TM-2 (docs/typed-mode-spec.md §3): `VAR name: type = expr` and
        // `CONST name: type = expr`. File-level declarations aren't part of
        // the block-tree walk `visit::visit` covers (see its module doc) —
        // iterated directly here, same pattern `signature.rs`/
        // `infer::collect_globals` use.
        for var in &hir.variables {
            if let Some(ann) = &var.annotation {
                v.flag(ann.range(), "type annotation");
            }
        }
        for c in &hir.constants {
            if let Some(ann) = &c.annotation {
                v.flag(ann.range(), "type annotation");
            }
        }
        // TM-4b (docs/typed-mode-spec.md §6): `STRUCT Name = #{ … }`. Also a
        // file-level declaration outside `visit::visit`'s block-tree walk —
        // same reason and same pattern as `VAR`/`CONST` annotations above.
        for s in &hir.structs {
            v.flag(s.ptr.text_range(), "`STRUCT` declaration");
        }
        // M-1 (docs/modules-spec.md §3): `#@module(name)` is brink-only —
        // `IMPORT`/`#@module`/`#@private`/`#@public` are the dialect-gated
        // module surface (INCLUDE stays ungated). Under strict-ink the
        // directive degrades to an inert tag in inklecate, so the superset
        // parse always succeeds and rejection lands here as the standard
        // E051-class diagnostic.
        if let Some(module) = &hir.module {
            v.flag(module.range, "`#@module` declaration");
        }
        // M-2 (docs/modules-spec.md §2/§4): `IMPORT` and `#@private`/
        // `#@public` complete the dialect-gated module surface. Same
        // superset-parse-then-reject shape as `#@module`.
        for import in &hir.imports {
            v.flag(import.range, "`IMPORT` statement");
        }
        for vis in &hir.visibility {
            let name = match vis.mark {
                brink_ir::VisibilityMark::Private => "`#@private` directive",
                brink_ir::VisibilityMark::Public => "`#@public` directive",
            };
            v.flag(vis.range, name);
        }
        // M-3 (docs/modules-spec.md §5): `#@was` is brink-only, same
        // superset-parse-then-reject shape as `#@module`/`#@private`/
        // `#@public`. One flat flag per occurrence (module-level and
        // definition-level alike) — `hir.was_directives` already covers
        // every placement.
        for range in &hir.was_directives {
            v.flag(*range, "`#@was` directive");
        }
    }
    out
}

struct GateVisitor<'a> {
    file: FileId,
    dialect: Dialect,
    resolved: &'a LookupSet<(FileId, rowan::TextRange)>,
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
                BlockStmt::While(w) => {
                    // `while await cond { … }` — the persistent-await marker is
                    // itself a brink extension (docs/flow-suspension-spec.md
                    // §3), flagged under strict-ink like a bare `await`.
                    if w.is_await {
                        self.flag(w.ptr.text_range(), "`await` suspension point");
                    }
                    self.flag_block_stmts(&w.body);
                }
                BlockStmt::For(f) => self.flag_block_stmts(&f.body),
                BlockStmt::Await(a) => {
                    self.flag(a.ptr.text_range(), "`await` suspension point");
                }
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

/// T1b stdlib slice 1 function names (`docs/t1b-surface-spec.md` §5) plus
/// the TM-3-completion conversion intrinsics (issue #659). Kept in sync by
/// hand with `resolve::is_t1b_stdlib_name` — same name, same list,
/// different call site (that one gates resolution; this one gates
/// `strict-ink`), not worth a shared constant across the two passes for ten
/// literals.
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
        // T2-2 (docs/effects-spec.md §10, issue #861): `#@effects(…)` is
        // brink-only, same superset-parse-then-reject shape as `#@module`.
        if let Some(assertion) = &knot.effects_assertion {
            self.flag(assertion.range, "`@[effects(…)]` assertion");
        }
    }

    fn enter_stitch(&mut self, stitch: &Stitch) {
        self.flag_params(&stitch.params);
        if let Some(assertion) = &stitch.effects_assertion {
            self.flag(assertion.range, "`@[effects(…)]` assertion");
        }
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
            // `~ await <cond>` — a FlowFrame suspension point
            // (docs/flow-suspension-spec.md §3), a brink extension rejected
            // under strict-ink like every other superset construct.
            Stmt::Await(a) => self.flag(a.ptr.text_range(), "`await` suspension point"),
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
            // NS-A1 (`docs/stdlib-spec.md` §1.4): a bare `none` in
            // expression position is the brink-dialect Option absence
            // literal — like the stdlib *call* names below it parses
            // identically to an ordinary reference, so the gate needs the
            // resolution result: a `none` that resolved to a real symbol
            // (a LIST item, VAR, temp…) is an ordinary reference, never
            // flagged; only the unresolved-therefore-the-literal case is
            // brink extension surface.
            Expr::Path(p) => {
                if let [seg] = p.segments.as_slice()
                    && seg.text == "none"
                    && !self.resolved.contains(&(self.file, p.range))
                {
                    self.flag(p.range, "`none` Option literal");
                }
            }
            Expr::Index(i) => self.flag(i.ptr.text_range(), "postfix indexing `[…]`"),
            Expr::StructLiteral(sl) => {
                self.flag(sl.ptr.text_range(), "struct construction literal");
            }
            Expr::FieldAccess(fa) => {
                self.flag(fa.ptr.text_range(), "postfix field access `.field`");
            }
            // T1c (docs/t1c-spec.md §2): `#fn(…)` is brink-dialect-gated
            // "under the T1b superset-grammar rule (strict-ink rejects at
            // analysis with the standard E051-class diagnostic; parse never
            // fails)".
            Expr::FnLiteral(fl) => {
                self.flag(fl.ptr.text_range(), "`#fn(…)` function-value creation");
            }
            // T1e (docs/t1e-spec.md §2): `ref lvalue-path` is brink-dialect-
            // gated the same way, same "superset grammar always parses,
            // dialect decides legality" rule.
            Expr::RefArg(ra) => {
                self.flag(ra.ptr.text_range(), "`ref` path-projection expression");
            }
            // NS-A5 (docs/stdlib-spec.md §7, F7): `a..b` / `a..=b` range
            // literals are brink-dialect-gated the same way — self-evident
            // extension syntax, no resolution consult needed.
            Expr::Range(r) => {
                self.flag(r.ptr.text_range(), "`..`/`..=` range literal");
            }
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
    fn strict_ink_flags_module_directive() {
        // M-1 (docs/modules-spec.md §3): `#@module` is brink-only.
        let hir = lower_src("#@module(quest)\nHi\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E051);
        assert!(diags[0].message.contains("#@module"));
    }

    #[test]
    fn brink_dialect_allows_module_directive() {
        let hir = lower_src("#@module(quest)\nHi\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::Brink);
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── T2-2 `#@effects(…)` assertion surface (docs/effects-spec.md §10,
    // issue #861) ──────────────────────────────────────────────────

    #[test]
    fn strict_ink_flags_effects_directive_on_knot() {
        let hir = lower_src("== guard ==\n@[effects(pure)]\nHalt!\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E051);
        assert!(diags[0].message.contains("effects"));
    }

    #[test]
    fn strict_ink_flags_effects_directive_on_stitch() {
        let hir = lower_src("== guard ==\nHalt!\n= mood\n@[effects(reads(gold))]\ngrumpy\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E051);
    }

    #[test]
    fn brink_dialect_allows_effects_directive() {
        let hir = lower_src("== guard ==\n#@effects(reads: gold, calls: audio)\nHalt!\n");
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

    // ── NS-A1 Option surface (docs/stdlib-spec.md §1.4, issue #1107) ───

    #[test]
    fn strict_ink_flags_unresolved_option_verb_calls() {
        for src in [
            "~ x = find(s, \"a\")\n",
            "~ x = get(m, \"k\")\n",
            "~ x = min(a)\n",
            "~ x = some(1)\n",
        ] {
            let hir = lower_src(src);
            let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
            assert!(
                diags.iter().any(|d| d.code == DiagnosticCode::E051),
                "expected E051 for {src:?}, got {diags:?}"
            );
        }
    }

    // ── NS-A6 rand verbs (docs/stdlib-spec.md §7, issue #1112) ─────────

    #[test]
    fn strict_ink_flags_unresolved_rand_verb_calls() {
        for src in [
            "~ x = float()
",
            "~ x = chance(0.5)
",
            "~ x = pick(a)
",
            "~ x = shuffled(a)
",
            "~ shuffle(a)
",
            "~ seed(42)
",
        ] {
            let hir = lower_src(src);
            let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
            assert!(
                diags.iter().any(|d| d.code == DiagnosticCode::E051),
                "expected E051 for {src:?}, got {diags:?}"
            );
        }
    }

    #[test]
    fn brink_dialect_does_not_flag_the_rand_surface() {
        let hir = lower_src(
            "~ x = chance(0.5)
~ y = pick(a)
~ shuffle(a)
~ seed(42)
~ z = float()
",
        );
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::Brink);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn strict_ink_flags_a_bare_unresolved_none() {
        let hir = lower_src("~ x = none\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E051);
        assert!(diags[0].message.contains("none"), "{diags:?}");
    }

    #[test]
    fn brink_dialect_does_not_flag_the_option_surface() {
        let hir = lower_src("~ x = find(s, \"a\")\n~ y = none\n~ z = some(1)\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::Brink);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn resolved_none_reference_is_never_flagged() {
        // A `none` that resolved to a real symbol (e.g. a LIST item) is an
        // ordinary reference in either dialect.
        let hir = lower_src("~ x = none\n");
        let Some(Stmt::Assignment(assign)) = hir.root_content.stmts.first() else {
            unreachable!("~ x = none lowers to an Assignment")
        };
        let Expr::Path(p) = &assign.value else {
            unreachable!("assignment value is the bare none path")
        };
        let resolutions = vec![brink_ir::ResolvedRef {
            file: FileId(0),
            range: p.range,
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
    fn strict_ink_flags_const_annotation() {
        // #641: CONST mirrors VAR's annotation gating end to end.
        let hir = lower_src("CONST speed: float = 0.5\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E051);
    }

    #[test]
    fn brink_dialect_does_not_flag_const_annotation() {
        let hir = lower_src("CONST speed: float = 0.5\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::Brink);
        assert!(diags.is_empty(), "{diags:?}");
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
        let hir = lower_src(
            "=== heal(hp) ===\nVAR gold = 100\nCONST speed = 0.5\n~ temp t = 1\n~ return hp\n",
        );
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── T1c function values (docs/t1c-spec.md §2) ─────────────────────

    #[test]
    fn strict_ink_flags_fn_literal() {
        let hir = lower_src("~ f = #fn(heal, hp)\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E051);
        assert!(diags[0].message.contains("#fn"), "{diags:?}");
    }

    #[test]
    fn strict_ink_flags_fn_literal_nested_in_a_call_argument() {
        // "every extension construct... at its exact span" — a #fn nested
        // inside an ordinary call argument still gets its own diagnostic.
        let hir = lower_src("~ x = apply(#fn(heal, hp), 5)\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E051);
    }

    #[test]
    fn brink_dialect_does_not_flag_fn_literal_at_the_gate() {
        // Under `Brink` the gate stays silent — T1c-1's "not yet
        // implemented" rejection is LIR lowering's non-suppressible E052
        // (`lir::lower::expr::reject_fn_literal`), not a gate concern.
        let hir = lower_src("~ f = #fn(heal, hp)\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::Brink);
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── T1e path projections (docs/t1e-spec.md §2, issue #831) ────────

    #[test]
    fn strict_ink_flags_range_literal() {
        // NS-A5 (docs/stdlib-spec.md §7, F7): `a..b` / `a..=b` are brink
        // extension syntax — E051 under strict-ink, at the literal's span.
        let hir = lower_src("~ x = 1..=6\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E051);
        assert!(diags[0].message.contains("range"), "{diags:?}");
    }

    #[test]
    fn brink_dialect_does_not_flag_range_literal() {
        let hir = lower_src("~ x = 0..10\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::Brink);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn strict_ink_flags_ref_expr() {
        let hir = lower_src("~ x = alter(ref gold, 5)\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E051);
        assert!(diags[0].message.contains("ref"), "{diags:?}");
    }

    #[test]
    fn brink_dialect_does_not_flag_ref_expr_at_the_gate() {
        // Under `Brink` the gate stays silent — legality (ref-argument
        // position, durable root) is `ref_projection`'s own E097/E080, not
        // a gate concern, same split T1c's `#fn` already established.
        let hir = lower_src("~ x = alter(ref gold, 5)\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::Brink);
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── TM-4b structs (docs/typed-mode-spec.md §6) ────────────────────

    #[test]
    fn strict_ink_flags_struct_decl() {
        let hir = lower_src("STRUCT Point = #{x: float, y: float}\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E051);
    }

    #[test]
    fn brink_dialect_does_not_flag_struct_decl() {
        let hir = lower_src("STRUCT Point = #{x: float, y: float}\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::Brink);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn strict_ink_flags_struct_literal() {
        let hir = lower_src("STRUCT Point = #{x: float, y: float}\n~ p = Point#{x: 1.0, y: 2.0}\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        // The decl AND the construction literal are each flagged.
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E051));
    }

    #[test]
    fn strict_ink_flags_field_access() {
        let hir =
            lower_src("STRUCT Point = #{x: float, y: float}\n~ x = Point#{x: 1.0, y: 2.0}.x\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        // The decl, the literal, AND the field access are each flagged.
        assert_eq!(diags.len(), 3, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E051));
    }

    #[test]
    fn brink_dialect_does_not_flag_struct_literal_or_field_access() {
        let hir =
            lower_src("STRUCT Point = #{x: float, y: float}\n~ x = Point#{x: 1.0, y: 2.0}.x\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::Brink);
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ─── M-2 module surface (docs/modules-spec.md §2/§4) ─────────────

    #[test]
    fn strict_ink_flags_import() {
        let hir = lower_src("IMPORT quest_3\nHi\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E051);
        assert!(diags[0].message.contains("IMPORT"));
    }

    #[test]
    fn strict_ink_flags_visibility_directive() {
        let hir = lower_src("#@private\nVAR secret = 0\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::StrictInk);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E051);
        assert!(diags[0].message.contains("#@private"));
    }

    #[test]
    fn brink_dialect_does_not_flag_module_surface() {
        let hir = lower_src("IMPORT quest_3\n#@private\nVAR secret = 0\nHi\n");
        let diags = check(&[(FileId(0), &hir)], &no_resolutions(), Dialect::Brink);
        assert!(diags.is_empty(), "{diags:?}");
    }
}
