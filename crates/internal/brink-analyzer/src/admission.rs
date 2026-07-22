//! The HIR admission validator (docs/hir-admission-contract.md §4.2,
//! docs/b0-sequencing.md §B0.3, issue #1172).
//!
//! [`validate_admission`] is a loud, **non-suppressible** pass that checks
//! the invariants the pipeline has historically trusted a frontend to
//! uphold by construction (`docs/hir-admission-contract.md` §3, D2–D8) —
//! tolerable only because there has been exactly one frontend. A second
//! frontend (the native parser, B0.5+) makes silent failure a certainty, so
//! this pass converts every such invariant into a hard, loud diagnostic at
//! the single AST→HIR seam (`lowered_query`, F-B) instead of leaving it to
//! misfire silently downstream (wrong resolution, wrong effects, wrong
//! codegen — never a panic, never a diagnostic).
//!
//! # What is checked (§4.2)
//!
//! 1. **Manifest ⇄ HIR agreement** — every [`UnresolvedRef::range`] matches
//!    a real referencing-expression range in the HIR body (`E121`); every
//!    declared symbol has a same-name HIR declaration node (`E122`);
//!    `Knot::is_function` agrees with the manifest's `"function"` sentinel
//!    (`E123`, F-I#4).
//! 2. **Range well-formedness + join-key uniqueness** — every node range is
//!    non-empty and in-bounds (`E124`), and no two `UnresolvedRef`s share a
//!    range (`E125`, Q2(a)'s ratified join key).
//! 3. **Name-convention conformance** — declared names match the
//!    qualification shape their [`SymbolKind`] requires (`E126`, F-I#3).
//! 4. **Control-flow classification** — a terminal (`Divert`/`Return`) must
//!    be the last statement in an inline conditional/sequence branch
//!    (`E127`, F-I#7).
//! 5. **Provenance-kind ⇄ `SymbolKind` consistency** — a `Knot`/`Stitch`
//!    HIR node's provenance class agrees with the manifest bucket its
//!    declared symbol was indexed under (`E128`, F-I#5, the #626
//!    floating-stitch trap).
//!
//! # What is deliberately NOT checked (§4.3)
//!
//! Resolvability of a reference, type correctness, range↔source-text
//! fidelity, and provenance resolvability are all downstream/frontend
//! concerns this pass does not second-guess — see the contract §4.3.
//!
//! # Scope notes (judgment calls, flagged for the coordinator)
//!
//! - **Reality vs. the contract's simplified wording**: the contract states
//!   stitches qualify as `knot.stitch` and labels as `knot[.stitch].label`.
//!   In the actual lowering, a *promoted* top-level stitch (`= stitch` with
//!   no enclosing `==knot==`) is declared under `SymbolKind::Stitch` with
//!   its **bare** name (see `hir::lower::structure::stitch::lower_top_level_stitch`),
//!   and a label declared before the first knot (`hir.root_content`) is
//!   bare too (`LowerScope::qualify_label` with no enclosing knot). Both are
//!   legitimate, corpus-real shapes, not bugs — [`conforms_to_convention`]
//!   accepts the wider shape rather than flagging real corpus code (per the
//!   "if a check trips on real code, the check is wrong" rule). Flagged for
//!   a contract wording fix.
//! - **`Param`/`Temp` locals are out of scope for check 1** — the contract's
//!   "every declared symbol has a corresponding HIR declaration node" reads
//!   most naturally against [`brink_ir::symbols::DeclaredSymbol`] (the
//!   `SymbolManifest` field is literally named "Declared knot/variable/…
//!   names"), a distinct type from `LocalSymbol` (params/temps) with no
//!   `detail`/`visibility`/`was` fields to agree on. Locals are exercised by
//!   the existing `E054`/`E082` shadowing checks elsewhere in the pipeline.
//! - This pass extends (not replaces) `hir::visit`'s traversal shape but is
//!   a **purpose-built walker**, not a `HirVisitor` impl: `hir::visit`'s own
//!   doc comment lists two deliberate gaps — it does not descend into tag
//!   contents (`Content.tags`/`Choice.tags`) or the flat top-level
//!   declaration vecs (`variables`/`constants`) — and both carry real
//!   `UnresolvedRef`-registering expressions (tag interpolations, VAR/CONST
//!   initializers) that check 1 must see. Reusing the shared visitor and
//!   patching those two gaps in an ad hoc way would still need the same
//!   knot/stitch scope-prefix threading `LowerScope::qualify_label` uses
//!   (for label qualification, needed by checks 1/3), so a dedicated walker
//!   ended up simpler than bolting extra state onto the shared one.

use rowan::{TextRange, TextSize};

use brink_ir::hir::{
    Block, BlockStmt, Choice, ChoiceSet, CondKind, Conditional, Content, ContentPart, DivertPath,
    DivertTarget, ElseBranch, ForStmt, HirFile, IfStmt, LogicBlock, Sequence, Stmt, StringPart,
    Tag, WhileStmt,
};
use brink_ir::symbols::{DeclaredSymbol, SymbolManifest};
use brink_ir::{Diagnostic, DiagnosticCode, Expr, FileId, SymbolKind};

use crate::determinism::{LookupMap, LookupSet};

/// Run every §4.2 admission check over one file's already-lowered
/// `(HirFile, SymbolManifest)` pair.
///
/// `file_len` bounds the well-formedness check (every range must end at or
/// before it) — supplied by the caller from the parsed source (`lowered_query`
/// has the `Parse` tree in scope; `HirFile`/`SymbolManifest` alone carry no
/// notion of the file's total length). Non-suppressible by construction:
/// callers must not route the result through `apply_suppressions`.
#[must_use]
pub fn validate_admission(
    file_id: FileId,
    hir: &HirFile,
    manifest: &SymbolManifest,
    file_len: TextSize,
) -> Vec<Diagnostic> {
    let mut c = Collector {
        file_id,
        file_len,
        ref_ranges: LookupSet::new(),
        labels: Vec::new(),
        diags: Vec::new(),
    };

    // Root content precedes the first knot — no knot/stitch scope prefix.
    c.walk_block(&hir.root_content, "");

    for v in &hir.variables {
        c.check_range(v.ptr.text_range());
        c.check_range(v.name.range);
        c.walk_expr(&v.value);
    }
    for cst in &hir.constants {
        c.check_range(cst.ptr.text_range());
        c.check_range(cst.name.range);
        c.walk_expr(&cst.value);
    }
    for l in &hir.lists {
        c.check_range(l.ptr.text_range());
        c.check_range(l.name.range);
        for m in &l.members {
            c.check_range(m.name.range);
        }
    }
    for s in &hir.structs {
        c.check_range(s.ptr.text_range());
        c.check_range(s.name.range);
        for f in &s.fields {
            c.check_range(f.name.range);
        }
    }
    for e in &hir.externals {
        c.check_range(e.ptr.text_range());
        c.check_range(e.name.range);
    }
    for inc in &hir.includes {
        c.check_range(inc.ptr.text_range());
    }

    for knot in &hir.knots {
        c.check_range(knot.ptr.text_range());
        c.check_range(knot.name.range);
        c.walk_block(&knot.body, &knot.name.text);
        for stitch in &knot.stitches {
            c.check_range(stitch.ptr.text_range());
            c.check_range(stitch.name.range);
            let prefix = format!("{}.{}", knot.name.text, stitch.name.text);
            c.walk_block(&stitch.body, &prefix);
        }
    }

    let Collector {
        ref_ranges,
        labels,
        mut diags,
        ..
    } = c;

    check_unresolved_refs(&ref_ranges, manifest, file_id, &mut diags); // E121
    check_ref_uniqueness(manifest, file_id, &mut diags); // E125
    check_declared_symbols_have_hir_nodes(hir, manifest, &labels, file_id, &mut diags); // E122
    check_is_function_sentinel(hir, manifest, file_id, &mut diags); // E123
    check_name_conventions(manifest, file_id, &mut diags); // E126
    check_provenance_kind_consistency(hir, manifest, file_id, &mut diags); // E128

    diags
}

// ─── The walker: range well-formedness (E124), reference-range collection
// (feeds E121/E125), label collection (feeds E122/E126), and inline
// terminal-last checking (E127) — one pass over the whole file. ──────────

struct Collector {
    file_id: FileId,
    file_len: TextSize,
    /// Every referencing-expression range found anywhere in the HIR body —
    /// candidates an `UnresolvedRef.range` must appear in (check 1a).
    ref_ranges: LookupSet<TextRange>,
    /// Every label declaration found, already qualified to match
    /// `LowerScope::qualify_label`'s convention (feeds checks 1b/3).
    labels: Vec<(String, TextRange)>,
    diags: Vec<Diagnostic>,
}

impl Collector {
    fn check_range(&mut self, range: TextRange) {
        if range.is_empty() || range.end() > self.file_len {
            self.diags.push(Diagnostic {
                file: self.file_id,
                range,
                message: DiagnosticCode::E124.title().to_string(),
                code: DiagnosticCode::E124,
            });
        }
    }

    /// Record a referencing-expression range (a candidate `UnresolvedRef`
    /// target) and check its well-formedness.
    fn push_ref(&mut self, range: TextRange) {
        self.check_range(range);
        self.ref_ranges.insert(range);
    }

    fn qualify_label(prefix: &str, label: &str) -> String {
        if prefix.is_empty() {
            label.to_string()
        } else {
            format!("{prefix}.{label}")
        }
    }

    fn walk_block(&mut self, block: &Block, prefix: &str) {
        if let Some(name) = &block.label {
            self.check_range(name.range);
            self.labels
                .push((Self::qualify_label(prefix, &name.text), name.range));
        }
        for stmt in &block.stmts {
            self.walk_stmt(stmt, prefix);
        }
    }

    fn walk_stmt(&mut self, stmt: &Stmt, prefix: &str) {
        match stmt {
            Stmt::Content(c) => self.walk_content(c, prefix),
            Stmt::Divert(d) => {
                if let Some(p) = &d.ptr {
                    self.check_range(p.text_range());
                }
                self.walk_divert_target(&d.target);
            }
            Stmt::TunnelCall(t) => {
                self.check_range(t.ptr.text_range());
                for target in &t.targets {
                    self.walk_divert_target(target);
                }
            }
            Stmt::ThreadStart(t) => {
                self.check_range(t.ptr.text_range());
                self.walk_divert_target(&t.target);
            }
            Stmt::TempDecl(t) => {
                self.check_range(t.ptr.text_range());
                self.check_range(t.name.range);
                if let Some(e) = &t.value {
                    self.walk_expr(e);
                }
            }
            Stmt::Assignment(a) => {
                self.check_range(a.ptr.text_range());
                self.walk_expr(&a.target);
                self.walk_expr(&a.value);
            }
            Stmt::Return(r) => {
                if let Some(p) = &r.ptr {
                    self.check_range(p.text_range());
                }
                if let Some(e) = &r.value {
                    self.walk_expr(e);
                }
                for e in &r.onwards_args {
                    self.walk_expr(e);
                }
            }
            Stmt::ChoiceSet(cs) => self.walk_choice_set(cs, prefix),
            Stmt::LabeledBlock(b) => self.walk_block(b, prefix),
            Stmt::Conditional(c) => self.walk_conditional(c, prefix),
            Stmt::Sequence(s) => self.walk_sequence(s, prefix),
            Stmt::ExprStmt(e) => self.walk_expr(e),
            Stmt::EndOfLine => {}
            Stmt::LogicBlock(lb) => self.walk_logic_block(lb),
            Stmt::Await(a) => {
                self.check_range(a.ptr.text_range());
                if let Some(e) = &a.condition {
                    self.walk_expr(e);
                }
            }
        }
    }

    fn walk_divert_target(&mut self, target: &DivertTarget) {
        if let DivertPath::Path(p) = &target.path {
            self.push_ref(p.range);
        }
        for e in &target.args {
            self.walk_expr(e);
        }
    }

    fn walk_content(&mut self, content: &Content, prefix: &str) {
        if let Some(p) = &content.ptr {
            self.check_range(p.text_range());
        }
        for part in &content.parts {
            self.walk_content_part(part, prefix);
        }
        for tag in &content.tags {
            self.walk_tag(tag, prefix);
        }
    }

    fn walk_content_part(&mut self, part: &ContentPart, prefix: &str) {
        match part {
            ContentPart::Interpolation(e) => self.walk_expr(e),
            ContentPart::InlineConditional(c) => self.walk_conditional(c, prefix),
            ContentPart::InlineSequence(s) => self.walk_sequence(s, prefix),
            ContentPart::Text(_) | ContentPart::Glue | ContentPart::Spring => {}
        }
    }

    /// Tag contents are one of `hir::visit`'s two documented walker gaps
    /// (module docs above) — tag interpolations register real
    /// `UnresolvedRef`s during lowering, so admission must see them too.
    fn walk_tag(&mut self, tag: &Tag, prefix: &str) {
        self.check_range(tag.ptr.text_range());
        for part in &tag.parts {
            self.walk_content_part(part, prefix);
        }
    }

    fn walk_choice_set(&mut self, cs: &ChoiceSet, prefix: &str) {
        for choice in &cs.choices {
            self.walk_choice(choice, prefix);
        }
        self.walk_block(&cs.continuation, prefix);
    }

    fn walk_choice(&mut self, choice: &Choice, prefix: &str) {
        self.check_range(choice.ptr.text_range());
        if let Some(name) = &choice.label {
            self.check_range(name.range);
            self.labels
                .push((Self::qualify_label(prefix, &name.text), name.range));
        }
        if let Some(e) = &choice.condition {
            self.walk_expr(e);
        }
        if let Some(c) = &choice.start_content {
            self.walk_content(c, prefix);
        }
        if let Some(c) = &choice.bracket_content {
            self.walk_content(c, prefix);
        }
        if let Some(c) = &choice.inner_content {
            self.walk_content(c, prefix);
        }
        for tag in &choice.tags {
            self.walk_tag(tag, prefix);
        }
        self.walk_block(&choice.body, prefix);
    }

    fn walk_conditional(&mut self, cond: &Conditional, prefix: &str) {
        self.check_range(cond.ptr.text_range());
        if let CondKind::Switch(e) = &cond.kind {
            self.walk_expr(e);
        }
        for branch in &cond.branches {
            if let Some(e) = &branch.condition {
                self.walk_expr(e);
            }
            self.check_terminal_last(&branch.body.stmts); // E127
            self.walk_block(&branch.body, prefix);
        }
    }

    fn walk_sequence(&mut self, seq: &Sequence, prefix: &str) {
        self.check_range(seq.ptr.text_range());
        for branch in &seq.branches {
            self.check_terminal_last(&branch.stmts); // E127
            self.walk_block(branch, prefix);
        }
    }

    /// Contract §4.2 check 4 / F-I#7: a terminal (`Divert`/`Return`) must be
    /// the last statement in an inline conditional/sequence branch. Trailing
    /// `EndOfLine` markers don't count as "after" (they're end-of-line
    /// bookkeeping, not authored content — same exemption
    /// `validate::has_meaningful_stmts_after` uses for the sibling E033/E029
    /// checks).
    fn check_terminal_last(&mut self, stmts: &[Stmt]) {
        let last_meaningful = stmts.iter().rposition(|s| !matches!(s, Stmt::EndOfLine));
        let Some(last) = last_meaningful else {
            return;
        };
        for stmt in &stmts[..last] {
            if let Some(range) = terminal_range(stmt) {
                self.diags.push(Diagnostic {
                    file: self.file_id,
                    range,
                    message: DiagnosticCode::E127.title().to_string(),
                    code: DiagnosticCode::E127,
                });
            }
        }
    }

    fn walk_logic_block(&mut self, lb: &LogicBlock) {
        self.check_range(lb.ptr.text_range());
        for bs in &lb.stmts {
            self.walk_block_stmt(bs);
        }
    }

    fn walk_block_stmt(&mut self, bs: &BlockStmt) {
        match bs {
            BlockStmt::TempDecl(t) => {
                self.check_range(t.ptr.text_range());
                self.check_range(t.name.range);
                if let Some(e) = &t.value {
                    self.walk_expr(e);
                }
            }
            BlockStmt::Assignment(a) => {
                self.check_range(a.ptr.text_range());
                self.walk_expr(&a.target);
                self.walk_expr(&a.value);
            }
            BlockStmt::Return(r) => {
                if let Some(p) = &r.ptr {
                    self.check_range(p.text_range());
                }
                if let Some(e) = &r.value {
                    self.walk_expr(e);
                }
                for e in &r.onwards_args {
                    self.walk_expr(e);
                }
            }
            BlockStmt::If(i) => self.walk_if_stmt(i),
            BlockStmt::While(w) => self.walk_while_stmt(w),
            BlockStmt::For(f) => self.walk_for_stmt(f),
            BlockStmt::Break(p) | BlockStmt::Continue(p) => self.check_range(p.text_range()),
            BlockStmt::ExprStmt(e) => self.walk_expr(e),
            BlockStmt::Await(a) => {
                self.check_range(a.ptr.text_range());
                if let Some(e) = &a.condition {
                    self.walk_expr(e);
                }
            }
        }
    }

    fn walk_if_stmt(&mut self, i: &IfStmt) {
        self.check_range(i.ptr.text_range());
        self.walk_expr(&i.condition);
        for s in &i.body {
            self.walk_block_stmt(s);
        }
        match &i.else_branch {
            Some(ElseBranch::ElseIf(inner)) => self.walk_if_stmt(inner),
            Some(ElseBranch::Else(stmts)) => {
                for s in stmts {
                    self.walk_block_stmt(s);
                }
            }
            None => {}
        }
    }

    fn walk_while_stmt(&mut self, w: &WhileStmt) {
        self.check_range(w.ptr.text_range());
        self.walk_expr(&w.condition);
        for s in &w.body {
            self.walk_block_stmt(s);
        }
    }

    fn walk_for_stmt(&mut self, f: &ForStmt) {
        self.check_range(f.ptr.text_range());
        self.check_range(f.var_name.range);
        self.walk_expr(&f.iterable);
        for s in &f.body {
            self.walk_block_stmt(s);
        }
    }

    fn walk_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::Null => {}
            Expr::String(s) => {
                for part in &s.parts {
                    if let StringPart::Interpolation(e) = part {
                        self.walk_expr(e);
                    }
                }
            }
            Expr::Path(p) | Expr::DivertTarget(p) => self.push_ref(p.range),
            Expr::ListLiteral(items) => {
                for p in items {
                    self.push_ref(p.range);
                }
            }
            Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => self.walk_expr(inner),
            Expr::Infix(l, _, r) => {
                self.walk_expr(l);
                self.walk_expr(r);
            }
            Expr::Call(path, args) => {
                self.push_ref(path.range);
                for a in args {
                    self.walk_expr(a);
                }
            }
            Expr::ArrayLiteral(a) => {
                self.check_range(a.ptr.text_range());
                for e in &a.elements {
                    self.walk_expr(e);
                }
            }
            Expr::MapLiteral(m) => {
                self.check_range(m.ptr.text_range());
                for (k, v) in &m.entries {
                    self.walk_expr(k);
                    self.walk_expr(v);
                }
            }
            Expr::Index(idx) => {
                self.check_range(idx.ptr.text_range());
                self.walk_expr(&idx.base);
                self.walk_expr(&idx.index);
            }
            Expr::Range(r) => {
                self.check_range(r.ptr.text_range());
                self.walk_expr(&r.start);
                self.walk_expr(&r.end);
            }
            Expr::StructLiteral(sl) => {
                self.check_range(sl.ptr.text_range());
                self.push_ref(sl.shape.range);
                for (_, v) in &sl.fields {
                    self.walk_expr(v);
                }
            }
            Expr::FieldAccess(fa) => {
                self.check_range(fa.ptr.text_range());
                self.walk_expr(&fa.base);
            }
            Expr::FnLiteral(fl) => {
                self.check_range(fl.ptr.text_range());
                self.push_ref(fl.target.range);
                for a in &fl.args {
                    self.walk_expr(a);
                }
            }
            Expr::RefArg(ra) => {
                self.check_range(ra.ptr.text_range());
                self.walk_expr(&ra.operand);
            }
        }
    }
}

/// A diagnostic anchor range for a `Divert`/`Return` statement, `None` for
/// anything else (mirrors `validate::stmt_range`'s `Option` shape — a
/// tunnel-flavored `Divert`/`Return` may legitimately carry no provenance,
/// same F-B2/D5 carve-out as the well-formedness check).
fn terminal_range(stmt: &Stmt) -> Option<TextRange> {
    match stmt {
        Stmt::Divert(d) => d.ptr.as_ref().map(brink_ir::Provenance::text_range),
        Stmt::Return(r) => r.ptr.as_ref().map(brink_ir::Provenance::text_range),
        _ => None,
    }
}

// ─── Check 1a / E121: every UnresolvedRef.range is a real referencing-expr
// range. ────────────────────────────────────────────────────────────────

fn check_unresolved_refs(
    ref_ranges: &LookupSet<TextRange>,
    manifest: &SymbolManifest,
    file_id: FileId,
    diags: &mut Vec<Diagnostic>,
) {
    for r in &manifest.unresolved {
        if !ref_ranges.contains(&r.range) {
            diags.push(Diagnostic {
                file: file_id,
                range: r.range,
                message: DiagnosticCode::E121.title().to_string(),
                code: DiagnosticCode::E121,
            });
        }
    }
}

// ─── Check 2b / E125: no two UnresolvedRefs share a range (the ratified
// Q2(a) join key must be unique). ─────────────────────────────────────

fn check_ref_uniqueness(manifest: &SymbolManifest, file_id: FileId, diags: &mut Vec<Diagnostic>) {
    let mut seen: LookupSet<TextRange> = LookupSet::new();
    for r in &manifest.unresolved {
        if !seen.insert(r.range) {
            diags.push(Diagnostic {
                file: file_id,
                range: r.range,
                message: DiagnosticCode::E125.title().to_string(),
                code: DiagnosticCode::E125,
            });
        }
    }
}

// ─── Check 1b / E122: every declared symbol has a corresponding HIR
// declaration node with the same name. ────────────────────────────────

fn check_declared_symbols_have_hir_nodes(
    hir: &HirFile,
    manifest: &SymbolManifest,
    labels: &[(String, TextRange)],
    file_id: FileId,
    diags: &mut Vec<Diagnostic>,
) {
    let mut hir_knots: LookupSet<&str> = LookupSet::new();
    let mut hir_stitches: LookupSet<String> = LookupSet::new();
    for knot in &hir.knots {
        match knot.symbol_kind() {
            SymbolKind::Stitch => {
                hir_stitches.insert(knot.name.text.clone());
            }
            _ => {
                hir_knots.insert(knot.name.text.as_str());
            }
        }
        for st in &knot.stitches {
            hir_stitches.insert(format!("{}.{}", knot.name.text, st.name.text));
        }
    }
    let hir_vars: LookupSet<&str> = hir.variables.iter().map(|v| v.name.text.as_str()).collect();
    let hir_consts: LookupSet<&str> = hir.constants.iter().map(|c| c.name.text.as_str()).collect();
    let hir_lists: LookupSet<&str> = hir.lists.iter().map(|l| l.name.text.as_str()).collect();
    let mut hir_list_items: LookupSet<String> = LookupSet::new();
    for l in &hir.lists {
        for m in &l.members {
            hir_list_items.insert(format!("{}.{}", l.name.text, m.name.text));
        }
    }
    let hir_structs: LookupSet<&str> = hir.structs.iter().map(|s| s.name.text.as_str()).collect();
    let hir_externals: LookupSet<&str> =
        hir.externals.iter().map(|e| e.name.text.as_str()).collect();
    let hir_labels: LookupSet<&str> = labels.iter().map(|(n, _)| n.as_str()).collect();

    let mut missing = |sym: &DeclaredSymbol| {
        diags.push(Diagnostic {
            file: file_id,
            range: sym.range,
            message: DiagnosticCode::E122.title().to_string(),
            code: DiagnosticCode::E122,
        });
    };

    for sym in &manifest.knots {
        if !hir_knots.contains(sym.name.as_str()) {
            missing(sym);
        }
    }
    for sym in &manifest.stitches {
        if !hir_stitches.contains(&sym.name) {
            missing(sym);
        }
    }
    for sym in &manifest.variables {
        if !hir_vars.contains(sym.name.as_str()) {
            missing(sym);
        }
    }
    for sym in &manifest.constants {
        if !hir_consts.contains(sym.name.as_str()) {
            missing(sym);
        }
    }
    for sym in &manifest.lists {
        if !hir_lists.contains(sym.name.as_str()) {
            missing(sym);
        }
    }
    for sym in &manifest.list_items {
        if !hir_list_items.contains(&sym.name) {
            missing(sym);
        }
    }
    for sym in &manifest.structs {
        if !hir_structs.contains(sym.name.as_str()) {
            missing(sym);
        }
    }
    for sym in &manifest.externals {
        if !hir_externals.contains(sym.name.as_str()) {
            missing(sym);
        }
    }
    for sym in &manifest.labels {
        if !hir_labels.contains(sym.name.as_str()) {
            missing(sym);
        }
    }
}

// ─── Check 1c / E123: Knot::is_function agrees with the manifest's
// "function" detail sentinel (F-I#4). ─────────────────────────────────

fn check_is_function_sentinel(
    hir: &HirFile,
    manifest: &SymbolManifest,
    file_id: FileId,
    diags: &mut Vec<Diagnostic>,
) {
    // Built once so the per-knot lookup below is O(1) — a `Vec::iter().find()`
    // per knot would make this whole check O(n^2) over a knot-heavy file
    // (caught by the NF-6 perf test: `perf_scales_linearly_with_file_size`).
    let by_name: LookupMap<&str, &DeclaredSymbol> = manifest
        .knots
        .iter()
        .map(|s| (s.name.as_str(), s))
        .collect();

    for knot in &hir.knots {
        // A promoted top-level stitch never carries a function header —
        // `is_function` is always `false` there and the manifest never
        // stamps the sentinel for it either (`lower_top_level_stitch`), so
        // there is nothing to disagree about.
        if knot.symbol_kind() != SymbolKind::Knot {
            continue;
        }
        let Some(sym) = by_name.get(knot.name.text.as_str()) else {
            continue; // absence is E122's concern, not this check's.
        };
        let sentinel = sym.detail.as_deref() == Some("function");
        if sentinel != knot.is_function {
            diags.push(Diagnostic {
                file: file_id,
                range: knot.name.range,
                message: DiagnosticCode::E123.title().to_string(),
                code: DiagnosticCode::E123,
            });
        }
    }
}

// ─── Check 3 / E126: declared-name qualification shape per SymbolKind
// (F-I#3). ──────────────────────────────────────────────────────────

fn check_name_conventions(manifest: &SymbolManifest, file_id: FileId, diags: &mut Vec<Diagnostic>) {
    let mut check = |sym: &DeclaredSymbol, kind: SymbolKind| {
        if !conforms_to_convention(kind, &sym.name) {
            diags.push(Diagnostic {
                file: file_id,
                range: sym.range,
                message: DiagnosticCode::E126.title().to_string(),
                code: DiagnosticCode::E126,
            });
        }
    };
    for sym in &manifest.knots {
        check(sym, SymbolKind::Knot);
    }
    for sym in &manifest.stitches {
        check(sym, SymbolKind::Stitch);
    }
    for sym in &manifest.variables {
        check(sym, SymbolKind::Variable);
    }
    for sym in &manifest.constants {
        check(sym, SymbolKind::Constant);
    }
    for sym in &manifest.lists {
        check(sym, SymbolKind::List);
    }
    for sym in &manifest.structs {
        check(sym, SymbolKind::Struct);
    }
    for sym in &manifest.externals {
        check(sym, SymbolKind::External);
    }
    for sym in &manifest.list_items {
        check(sym, SymbolKind::ListItem);
    }
    for sym in &manifest.labels {
        check(sym, SymbolKind::Label);
    }
}

/// The qualification shape §4.2 check 3 requires per kind — widened past
/// the contract's simplified wording where corpus reality legitimately
/// produces a shorter shape (see the module-doc scope note): a promoted
/// top-level stitch is bare (0 dots) as well as a real nested `knot.stitch`
/// (1 dot); a label before the first knot is bare too (0 dots), alongside
/// `knot.label` (1 dot) and `knot.stitch.label` (2 dots).
fn conforms_to_convention(kind: SymbolKind, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let segments: Vec<&str> = name.split('.').collect();
    if segments.iter().any(|s| s.is_empty()) {
        return false;
    }
    match kind {
        SymbolKind::Knot
        | SymbolKind::Variable
        | SymbolKind::Constant
        | SymbolKind::List
        | SymbolKind::Struct
        | SymbolKind::External => segments.len() == 1,
        SymbolKind::Stitch => segments.len() == 1 || segments.len() == 2,
        SymbolKind::ListItem => segments.len() == 2,
        SymbolKind::Label => (1..=3).contains(&segments.len()),
        // Not manifest-bucket kinds this check iterates — locals are
        // out of scope (see the module-doc scope note).
        SymbolKind::Param | SymbolKind::Temp => true,
    }
}

// ─── Check 5 / E128: a Knot/Stitch HIR node's provenance class agrees with
// the manifest bucket its declared symbol is indexed under (F-I#5, the #626
// floating-stitch trap). ──────────────────────────────────────────────

fn check_provenance_kind_consistency(
    hir: &HirFile,
    manifest: &SymbolManifest,
    file_id: FileId,
    diags: &mut Vec<Diagnostic>,
) {
    // Built once — see `check_is_function_sentinel`'s comment on the same
    // O(n^2)-via-repeated-linear-scan trap.
    let knot_names: LookupSet<&str> = manifest.knots.iter().map(|s| s.name.as_str()).collect();
    let stitch_names: LookupSet<&str> = manifest.stitches.iter().map(|s| s.name.as_str()).collect();

    for knot in &hir.knots {
        let expected = knot.symbol_kind();
        let in_knots = knot_names.contains(knot.name.text.as_str());
        let in_stitches = stitch_names.contains(knot.name.text.as_str());
        let ok = match expected {
            SymbolKind::Stitch => in_stitches && !in_knots,
            _ => in_knots && !in_stitches,
        };
        if !ok {
            diags.push(Diagnostic {
                file: file_id,
                range: knot.name.range,
                message: DiagnosticCode::E128.title().to_string(),
                code: DiagnosticCode::E128,
            });
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────
//
// #672-A posture: each malformed-triple fixture is constructed directly
// (not via a hand-rolled-from-scratch HirFile/SymbolManifest — that would
// be enormous and brittle) by lowering real, valid `.ink` source through
// the actual pipeline (`brink_ir::hir::lower`, the same entry point
// `lower_file` composes) and then corrupting exactly the one field the
// check under test cares about. This is "direct" (calls `validate_admission`
// directly, no salsa) and "pipeline" (the base HIR/manifest is real lowering
// output, not synthetic) at once.

#[cfg(test)]
mod tests {
    use rowan::TextSize;

    use brink_ir::hir::CondBranch;
    use brink_ir::provenance::NodeClass;
    use brink_ir::{Divert, Provenance};

    use super::*;

    fn lower_src(src: &str) -> (HirFile, SymbolManifest, TextSize) {
        let parsed = brink_syntax::parse(src);
        let tree = parsed.tree();
        let (hir, manifest, _diags) = brink_ir::hir::lower(FileId(0), &tree);
        (hir, manifest, TextSize::of(src))
    }

    fn codes(diags: &[Diagnostic]) -> Vec<DiagnosticCode> {
        diags.iter().map(|d| d.code).collect()
    }

    #[test]
    fn clean_file_has_no_admission_diagnostics() {
        let (hir, manifest, len) =
            lower_src("VAR x = 1\n== function foo ==\n~ return\n== knot ==\n{x}\n-> foo\n-> END\n");
        let diags = validate_admission(FileId(0), &hir, &manifest, len);
        assert!(diags.is_empty(), "expected a clean file, got: {diags:?}");
    }

    /// Corpus-reality regression (validate.rs's sibling
    /// `inline_branch_diverts_produce_no_spurious_structural_diagnostics`
    /// documents the same guarantee for E032/E033/E034): ink's own
    /// inline-branch lowering always places a divert last in the branch —
    /// `{cond: -> a text after divert}` lowers to `[Content, Divert]`, never
    /// `[Divert, Content]` — so E127 must never fire on real corpus shapes
    /// like this, only on a HIR a buggy frontend fabricated directly.
    #[test]
    fn inline_branch_diverts_produce_no_e127() {
        let cases = [
            "A {cond: -> away} B\n=== away ===\n-> END\n",
            "{cond: -> a | -> b}\n=== a ===\n-> END\n=== b ===\n-> END\n",
            "{cond: -> a text after divert}\n=== a ===\n-> END\n",
            "Line {cond: -> a} {other: -> b}\n=== a ===\n-> END\n=== b ===\n-> END\n",
        ];
        for src in cases {
            let (hir, manifest, len) = lower_src(src);
            let diags = validate_admission(FileId(0), &hir, &manifest, len);
            assert!(
                !codes(&diags).contains(&DiagnosticCode::E127),
                "inline-branch divert must not trigger E127: {src:?} -> {diags:?}"
            );
        }
    }

    /// Corpus-reality regression for the two scope-widened conventions
    /// documented in the module doc: a promoted top-level stitch and a
    /// pre-first-knot label are both legitimately bare (0 dots) — E126 must
    /// not flag them.
    #[test]
    fn widened_conventions_produce_no_e126() {
        let (hir, manifest, len) = lower_src("- (opening)\nHello\n= stitch_a\n-> END\n");
        let diags = validate_admission(FileId(0), &hir, &manifest, len);
        assert!(
            !codes(&diags).contains(&DiagnosticCode::E126),
            "bare promoted-stitch/pre-knot-label names must not trigger E126: {diags:?}"
        );
    }

    #[test]
    fn e121_unresolved_ref_with_no_matching_hir_range() {
        let (hir, mut manifest, len) = lower_src("VAR x = 1\n== knot ==\n{x}\n-> END\n");
        let r = manifest
            .unresolved
            .first_mut()
            .expect("one unresolved ref for `x`");
        // Move the recorded range off the real `{x}` reference entirely.
        r.range = TextRange::new(0.into(), 1.into());
        let diags = validate_admission(FileId(0), &hir, &manifest, len);
        assert!(codes(&diags).contains(&DiagnosticCode::E121), "{diags:?}");
    }

    #[test]
    fn e122_declared_symbol_with_no_hir_node() {
        let (hir, mut manifest, len) = lower_src("== knot ==\n-> END\n");
        let sym = manifest.knots.first_mut().expect("one declared knot");
        sym.name = "ghost".to_string();
        let diags = validate_admission(FileId(0), &hir, &manifest, len);
        assert!(codes(&diags).contains(&DiagnosticCode::E122), "{diags:?}");
    }

    #[test]
    fn e123_is_function_sentinel_disagreement() {
        let (mut hir, manifest, len) = lower_src("== function foo ==\n~ return\n");
        hir.knots[0].is_function = false; // manifest still stamps the "function" sentinel
        let diags = validate_admission(FileId(0), &hir, &manifest, len);
        assert!(codes(&diags).contains(&DiagnosticCode::E123), "{diags:?}");
    }

    #[test]
    fn e124_range_empty_is_malformed() {
        let (mut hir, manifest, len) = lower_src("== knot ==\n-> END\n");
        hir.knots[0].name.range = TextRange::new(len, len);
        let diags = validate_admission(FileId(0), &hir, &manifest, len);
        assert!(codes(&diags).contains(&DiagnosticCode::E124), "{diags:?}");
    }

    #[test]
    fn e124_range_out_of_bounds_is_malformed() {
        let (mut hir, manifest, len) = lower_src("== knot ==\n-> END\n");
        let past_eof = len + TextSize::from(1000);
        hir.knots[0].name.range = TextRange::new(len, past_eof);
        let diags = validate_admission(FileId(0), &hir, &manifest, len);
        assert!(codes(&diags).contains(&DiagnosticCode::E124), "{diags:?}");
    }

    #[test]
    fn e125_duplicate_unresolved_ref_ranges() {
        let (hir, mut manifest, len) =
            lower_src("VAR x = 1\nVAR y = 2\n== knot ==\n{x} {y}\n-> END\n");
        assert!(
            manifest.unresolved.len() >= 2,
            "need at least two refs: {manifest:?}"
        );
        let first_range = manifest.unresolved[0].range;
        manifest.unresolved[1].range = first_range;
        let diags = validate_admission(FileId(0), &hir, &manifest, len);
        assert!(codes(&diags).contains(&DiagnosticCode::E125), "{diags:?}");
    }

    #[test]
    fn e126_name_convention_violation() {
        let (hir, mut manifest, len) = lower_src("== knot ==\n-> END\n");
        manifest.knots[0].name = "a.b".to_string(); // knots must be bare (0 dots)
        let diags = validate_admission(FileId(0), &hir, &manifest, len);
        assert!(codes(&diags).contains(&DiagnosticCode::E126), "{diags:?}");
    }

    #[test]
    fn e127_divert_not_last_in_inline_branch() {
        let (mut hir, manifest, len) = lower_src("== knot ==\nHello\n-> END\n");
        let synthetic_range = TextRange::new(0.into(), 1.into());
        let branch_body = Block::from_stmts(vec![
            Stmt::Divert(Divert {
                ptr: Some(Provenance::synthetic(NodeClass::Divert, synthetic_range)),
                target: DivertTarget {
                    path: DivertPath::Done,
                    args: Vec::new(),
                },
            }),
            Stmt::Content(Content {
                ptr: None,
                parts: Vec::new(),
                tags: Vec::new(),
            }),
        ]);
        let cond = Conditional {
            ptr: Provenance::synthetic(NodeClass::Conditional, synthetic_range),
            kind: CondKind::InitialCondition,
            branches: vec![CondBranch {
                condition: None,
                body: branch_body,
                container_id: None,
            }],
        };
        hir.knots[0].body.stmts.insert(0, Stmt::Conditional(cond));
        let diags = validate_admission(FileId(0), &hir, &manifest, len);
        assert!(codes(&diags).contains(&DiagnosticCode::E127), "{diags:?}");
    }

    #[test]
    fn e128_provenance_kind_disagrees_with_manifest_bucket() {
        let (hir, mut manifest, len) = lower_src("== knot ==\n-> END\n");
        // Simulate a frontend that stamped `NodeClass::Knot` provenance but
        // indexed the declaration under the stitch bucket.
        let sym = manifest.knots.remove(0);
        manifest.stitches.push(sym);
        let diags = validate_admission(FileId(0), &hir, &manifest, len);
        assert!(codes(&diags).contains(&DiagnosticCode::E128), "{diags:?}");
    }
}
