//! [`project_manifest`]: derive a [`SymbolManifest`] from an already-lowered
//! [`HirFile`] (B0.4, `docs/hir-admission-contract.md` Q3(b),
//! `docs/b0-sequencing.md` §B0.4, issue #1173).
//!
//! Before this pass existed, a frontend built the `HirFile` body and the
//! `SymbolManifest` as two independent, hand-kept-consistent artifacts
//! (D3, `docs/hir-admission-contract.md` §3) — every declared symbol, every
//! local, every unresolved reference had to be pushed to *both* the HIR tree
//! and the manifest, by hand, at the exact point of construction, and
//! nothing ever cross-checked that they agreed. `project_manifest` deletes
//! that obligation: given a well-formed `HirFile`, it derives the *entire*
//! `SymbolManifest` structurally. A frontend that emits correct HIR can no
//! longer emit an inconsistent manifest, because it never emits a manifest
//! at all — the pipeline projects one.
//!
//! # Design notes (judgment calls — see the B0.4 gate report)
//!
//! - **Per-reference scope, without a stored field.** The contract's named
//!   gap (Q3(b): "the gap is per-reference scope context") is closed by
//!   *structural derivation*, not by adding a `scope` field to every `Expr`.
//!   [`Scope`] is `{knot, stitch}` — exactly the two container levels the
//!   admission contract fixes for v1 (Q4(b)) — so a depth-bounded walk that
//!   tracks "which knot/stitch am I structurally inside" while descending
//!   the tree recovers the same scope the original interleaved lowering
//!   computed from `LowerScope::current_knot`/`current_stitch`. This is the
//!   same technique `brink_analyzer::admission`'s `Collector` already uses
//!   for its `prefix` string (proven correct: the B0.3 corpus-wide
//!   admission-clean gate is green against it) — this walker mirrors that
//!   one's traversal shape, extended to build manifest entries instead of
//!   just validating ranges.
//! - **Declaration-metadata fields are additive HIR fields, not
//!   re-derived.** `visibility`/`was`/`doc` (and, for `EXTERNAL`, per-param
//!   names) are computed at lowering time from a declaration's own local
//!   directive/`///`-comment syntax — there is no way to recover *which*
//!   directive attaches to *which* declaration from `HirFile.visibility`/
//!   `was_directives` alone (those are flat, file-level occurrence lists
//!   for the dialect gate, carrying no back-pointer to a declaration). So
//!   B0.4 added `doc`/`visibility`/`was` fields directly to `Knot`, `Stitch`,
//!   `VarDecl`, `ConstDecl`, `ListDecl`, `StructDecl` (visibility only —
//!   `STRUCT` never parses a `#@was`), `ExternalDecl` (plus `params`,
//!   replacing the name-losing `param_count: u8` as the manifest's source).
//! - **Vec order is NOT asserted byte-identical to the legacy manifest for
//!   `unresolved`/`locals`/`labels`.** The legacy interleaved lowering has
//!   at least two accidental orderings baked into its recursion (root
//!   content's refs land *last*, after every knot's; a knot's *stitches*'
//!   refs land before the knot's *own* body's) that are artifacts of
//!   `lower_knot_body`'s call sequencing, not a documented contract, and
//!   nothing downstream keys off manifest vector position (resolution joins
//!   by range/name — see `resolve.rs`'s `lookup_local_in_scope`, which
//!   picks the closest-*preceding* local by `range.start()`, not vector
//!   index). This walker uses natural top-down structural order instead.
//!   The differential burn-in test
//!   (`crates/internal/brink-test-harness/tests/b04_manifest_burn_in.rs`)
//!   compares those three fields order-insensitively (sorted by range) and
//!   every other field (which *does* provably match legacy order — see that
//!   test's module doc) byte-for-byte.

use rowan::TextRange;

use crate::hir::{
    Block, BlockStmt, Choice, ChoiceSet, CondKind, Conditional, Content, ContentPart, DivertPath,
    DivertTarget, ElseBranch, ForStmt, HirFile, IfStmt, Knot, LambdaBody, LambdaExpr, LogicBlock,
    Param, Path, Sequence, Stmt, StringPart, Tag, WhileStmt,
};
use crate::host_manifest::DocBlock;
use crate::{Expr, ParamInfo, Scope, SymbolKind, VisibilityMark};

use super::{DeclaredSymbol, LocalSymbol, RefKind, SymbolManifest, UnresolvedRef};

/// Derive the [`SymbolManifest`] a well-formed `HirFile` implies.
///
/// See the module doc for the design rationale. This function never emits
/// diagnostics — a `HirFile` handed to it is assumed already lowered
/// (diagnostics are the frontend's problem, not the projection's).
#[must_use]
pub fn project_manifest(hir: &HirFile) -> SymbolManifest {
    let mut p = Projector::default();

    // Root content precedes the first knot — no knot/stitch scope prefix,
    // same carve-out `brink_analyzer::admission`'s walker uses.
    p.walk_block(&hir.root_content, None, None);

    for v in &hir.variables {
        p.declare(
            SymbolKind::Variable,
            v.name.text.clone(),
            v.name.range,
            Vec::new(),
            None,
            v.visibility,
            v.was.clone(),
            v.doc.clone(),
        );
        p.walk_expr(&v.value, None, None);
    }
    for c in &hir.constants {
        p.declare(
            SymbolKind::Constant,
            c.name.text.clone(),
            c.name.range,
            Vec::new(),
            None,
            c.visibility,
            c.was.clone(),
            c.doc.clone(),
        );
        p.walk_expr(&c.value, None, None);
    }
    for l in &hir.lists {
        p.declare(
            SymbolKind::List,
            l.name.text.clone(),
            l.name.range,
            Vec::new(),
            None,
            l.visibility,
            l.was.clone(),
            l.doc.clone(),
        );
        for m in &l.members {
            let qualified = format!("{}.{}", l.name.text, m.name.text);
            p.manifest.list_items.push(DeclaredSymbol {
                name: qualified,
                range: m.name.range,
                params: Vec::new(),
                detail: None,
                visibility: None,
                was: None,
            });
        }
    }
    for s in &hir.structs {
        p.declare(
            SymbolKind::Struct,
            s.name.text.clone(),
            s.name.range,
            Vec::new(),
            None,
            s.visibility,
            None,
            s.doc.clone(),
        );
        // Field TypeExprs never register unresolved refs (`hir::lower::types`
        // takes no `sink` — a nominal-only grammar, resolved later by a
        // different mechanism), so there's nothing further to walk here.
    }
    for e in &hir.externals {
        p.declare(
            SymbolKind::External,
            e.name.text.clone(),
            e.name.range,
            e.params.clone(),
            None,
            e.visibility,
            e.was.clone(),
            e.doc.clone(),
        );
    }
    // `hir.includes`: `IncludeSite` carries no manifest entry (F-A — the
    // manifest has no `includes` bucket; the analyzer reads `HirFile`
    // directly for INCLUDE-graph wiring).

    for knot in &hir.knots {
        p.project_knot(knot);
    }

    p.manifest
}

// ─── The projector ──────────────────────────────────────────────────

#[derive(Default)]
struct Projector {
    manifest: SymbolManifest,
}

impl Projector {
    fn scope_of(knot: Option<&str>, stitch: Option<&str>) -> Scope {
        Scope {
            knot: knot.map(str::to_string),
            stitch: stitch.map(str::to_string),
        }
    }

    fn qualify_label(knot: Option<&str>, stitch: Option<&str>, label: &str) -> String {
        match (knot, stitch) {
            (Some(k), Some(s)) => format!("{k}.{s}.{label}"),
            (Some(k), None) => format!("{k}.{label}"),
            _ => label.to_string(),
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors EffectSink::declare_full's shape"
    )]
    fn declare(
        &mut self,
        kind: SymbolKind,
        name: String,
        range: TextRange,
        params: Vec<ParamInfo>,
        detail: Option<String>,
        visibility: Option<VisibilityMark>,
        was: Option<(String, TextRange)>,
        doc: Option<DocBlock>,
    ) {
        if let Some(doc) = doc {
            self.manifest.docs.insert((kind, name.clone()), doc);
        }
        let sym = DeclaredSymbol {
            name,
            range,
            params,
            detail,
            visibility,
            was,
        };
        match kind {
            SymbolKind::Knot => self.manifest.knots.push(sym),
            SymbolKind::Stitch => self.manifest.stitches.push(sym),
            SymbolKind::Variable => self.manifest.variables.push(sym),
            SymbolKind::Constant => self.manifest.constants.push(sym),
            SymbolKind::List => self.manifest.lists.push(sym),
            SymbolKind::Struct => self.manifest.structs.push(sym),
            SymbolKind::External => self.manifest.externals.push(sym),
            SymbolKind::Label => self.manifest.labels.push(sym),
            SymbolKind::ListItem => self.manifest.list_items.push(sym),
            // Param/Temp are never declared this way (see `push_local`).
            SymbolKind::Param | SymbolKind::Temp => {}
        }
    }

    fn push_ref(
        &mut self,
        path: String,
        range: TextRange,
        kind: RefKind,
        knot: Option<&str>,
        stitch: Option<&str>,
        arg_count: Option<usize>,
    ) {
        // Mirrors `EffectSink::add_unresolved`'s empty-path guard — a
        // malformed parse can yield an empty path/identifier.
        if path.is_empty() {
            return;
        }
        self.manifest.unresolved.push(UnresolvedRef {
            path,
            range,
            kind,
            scope: Self::scope_of(knot, stitch),
            arg_count,
        });
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors declare's shape (issue #530: annotation is a plain \
                  positional passthrough, not a new structural concern)"
    )]
    fn push_local(
        &mut self,
        name: String,
        range: TextRange,
        kind: SymbolKind,
        knot: Option<&str>,
        stitch: Option<&str>,
        param_detail: Option<ParamInfo>,
        annotation: Option<crate::TypeExpr>,
    ) {
        self.manifest.locals.push(LocalSymbol {
            name,
            range,
            scope: Self::scope_of(knot, stitch),
            kind,
            param_detail,
            annotation,
        });
    }

    fn push_label(&mut self, name: String, range: TextRange) {
        self.manifest.labels.push(DeclaredSymbol {
            name,
            range,
            params: Vec::new(),
            detail: None,
            visibility: None,
            was: None,
        });
    }

    // ─── Containers ─────────────────────────────────────────────────

    fn project_knot(&mut self, knot: &Knot) {
        let bucket = knot.symbol_kind();
        let detail = if knot.is_function {
            Some("function".to_owned())
        } else {
            None
        };
        self.declare(
            bucket,
            knot.name.text.clone(),
            knot.name.range,
            param_infos(&knot.params),
            detail,
            knot.visibility,
            knot.was.clone(),
            knot.doc.clone(),
        );

        // A container's own params are scoped under its own name as
        // `current_knot` — true for a real knot *and* for a promoted
        // top-level stitch (`lower_top_level_stitch` sets
        // `scope.current_knot = Some(name_text)` before registering its
        // params as locals, exactly like `lower_knot`).
        for param in &knot.params {
            self.push_local(
                param.name.text.clone(),
                param.name.range,
                SymbolKind::Param,
                Some(knot.name.text.as_str()),
                None,
                Some(param_info(param)),
                param.annotation.clone(),
            );
        }

        self.walk_block(&knot.body, Some(&knot.name.text), None);

        for st in &knot.stitches {
            let qualified = format!("{}.{}", knot.name.text, st.name.text);
            self.declare(
                SymbolKind::Stitch,
                qualified,
                st.name.range,
                param_infos(&st.params),
                None,
                st.visibility,
                st.was.clone(),
                st.doc.clone(),
            );
            for param in &st.params {
                self.push_local(
                    param.name.text.clone(),
                    param.name.range,
                    SymbolKind::Param,
                    Some(knot.name.text.as_str()),
                    Some(st.name.text.as_str()),
                    Some(param_info(param)),
                    param.annotation.clone(),
                );
            }
            self.walk_block(&st.body, Some(&knot.name.text), Some(&st.name.text));
        }
    }

    // ─── Blocks / statements (mirrors `brink_analyzer::admission`'s
    // `Collector` traversal shape) ────────────────────────────────────

    fn walk_block(&mut self, block: &Block, knot: Option<&str>, stitch: Option<&str>) {
        if let Some(name) = &block.label {
            let qualified = Self::qualify_label(knot, stitch, &name.text);
            self.push_label(qualified, name.range);
        }
        for stmt in &block.stmts {
            self.walk_stmt(stmt, knot, stitch);
        }
    }

    fn walk_stmt(&mut self, stmt: &Stmt, knot: Option<&str>, stitch: Option<&str>) {
        match stmt {
            Stmt::Content(c) => self.walk_content(c, knot, stitch),
            Stmt::Divert(d) => self.walk_divert_target(&d.target, knot, stitch),
            Stmt::TunnelCall(t) => {
                for target in &t.targets {
                    self.walk_divert_target(target, knot, stitch);
                }
            }
            Stmt::ThreadStart(t) => self.walk_divert_target(&t.target, knot, stitch),
            Stmt::TempDecl(t) => {
                if let Some(e) = &t.value {
                    self.walk_expr(e, knot, stitch);
                }
                self.push_local(
                    t.name.text.clone(),
                    t.name.range,
                    SymbolKind::Temp,
                    knot,
                    stitch,
                    None,
                    t.annotation.clone(),
                );
            }
            Stmt::Assignment(a) => {
                self.walk_expr(&a.target, knot, stitch);
                self.walk_expr(&a.value, knot, stitch);
            }
            Stmt::Return(r) => {
                if let Some(e) = &r.value {
                    self.walk_expr(e, knot, stitch);
                }
                for e in &r.onwards_args {
                    self.walk_expr(e, knot, stitch);
                }
            }
            Stmt::ChoiceSet(cs) => self.walk_choice_set(cs, knot, stitch),
            Stmt::LabeledBlock(b) => self.walk_block(b, knot, stitch),
            Stmt::Conditional(c) => self.walk_conditional(c, knot, stitch),
            Stmt::Sequence(s) => self.walk_sequence(s, knot, stitch),
            Stmt::ExprStmt(e) => self.walk_expr(e, knot, stitch),
            Stmt::EndOfLine => {}
            Stmt::LogicBlock(lb) => self.walk_logic_block(lb, knot, stitch),
            Stmt::Await(a) => {
                if let Some(e) = &a.condition {
                    self.walk_expr(e, knot, stitch);
                }
            }
        }
    }

    fn walk_divert_target(
        &mut self,
        target: &DivertTarget,
        knot: Option<&str>,
        stitch: Option<&str>,
    ) {
        if let DivertPath::Path(p) = &target.path {
            // Issue #2156: carry the divert's own call-arg count through so
            // `brink-analyzer::resolve::resolve_divert` can arity-check it
            // (`E176`) exactly like `RefKind::Function` already does for an
            // ordinary call — this used to be hardcoded `None` regardless
            // of `target.args.len()`, which is why the check could never
            // fire for a divert on either dialect (see `E176`'s own doc
            // comment in `hir::diagnostics` for the full history).
            self.push_ref(
                path_text(p),
                p.range,
                RefKind::Divert,
                knot,
                stitch,
                Some(target.args.len()),
            );
        }
        for e in &target.args {
            self.walk_expr(e, knot, stitch);
        }
    }

    fn walk_content(&mut self, content: &Content, knot: Option<&str>, stitch: Option<&str>) {
        for part in &content.parts {
            self.walk_content_part(part, knot, stitch);
        }
        for tag in &content.tags {
            self.walk_tag(tag, knot, stitch);
        }
    }

    fn walk_content_part(&mut self, part: &ContentPart, knot: Option<&str>, stitch: Option<&str>) {
        match part {
            ContentPart::Interpolation(e) => self.walk_expr(e, knot, stitch),
            ContentPart::InlineConditional(c) => self.walk_conditional(c, knot, stitch),
            ContentPart::InlineSequence(s) => self.walk_sequence(s, knot, stitch),
            // A span is presentational (§4.3) — its children still carry
            // real references (an interpolation may sit inside `<b>…</b>`),
            // so the symbol index must see into it, same reasoning as
            // `hir::visit::walk_content_part`.
            ContentPart::Span(span) => {
                for child in &span.children {
                    self.walk_content_part(child, knot, stitch);
                }
            }
            ContentPart::Text(_) | ContentPart::Glue | ContentPart::Spring => {}
        }
    }

    /// Tag contents are one of `hir::visit`'s documented walker gaps — tag
    /// interpolations register real refs during lowering, same carve-out
    /// `brink_analyzer::admission` documents.
    fn walk_tag(&mut self, tag: &Tag, knot: Option<&str>, stitch: Option<&str>) {
        for part in &tag.parts {
            self.walk_content_part(part, knot, stitch);
        }
    }

    fn walk_choice_set(&mut self, cs: &ChoiceSet, knot: Option<&str>, stitch: Option<&str>) {
        for choice in &cs.choices {
            self.walk_choice(choice, knot, stitch);
        }
        self.walk_block(&cs.continuation, knot, stitch);
    }

    fn walk_choice(&mut self, choice: &Choice, knot: Option<&str>, stitch: Option<&str>) {
        if let Some(name) = &choice.label {
            let qualified = Self::qualify_label(knot, stitch, &name.text);
            self.push_label(qualified, name.range);
        }
        if let Some(e) = &choice.condition {
            self.walk_expr(e, knot, stitch);
        }
        if let Some(c) = &choice.start_content {
            self.walk_content(c, knot, stitch);
        }
        if let Some(c) = &choice.bracket_content {
            self.walk_content(c, knot, stitch);
        }
        if let Some(c) = &choice.inner_content {
            self.walk_content(c, knot, stitch);
        }
        for tag in &choice.tags {
            self.walk_tag(tag, knot, stitch);
        }
        // Guard-`as` binding (issue #1508) — same treatment as
        // `walk_conditional`'s `branch.binding`/`walk_if_stmt`'s
        // `i.binding`: index it as an ordinary local, scoped to the
        // choice's own body, so a `{n}` read inside resolves instead of
        // raising E025.
        self.push_as_binding(choice.binding.as_ref(), knot, stitch);
        self.walk_block(&choice.body, knot, stitch);
    }

    fn walk_conditional(&mut self, cond: &Conditional, knot: Option<&str>, stitch: Option<&str>) {
        if let CondKind::Switch(e) = &cond.kind {
            self.walk_expr(e, knot, stitch);
        }
        for branch in &cond.branches {
            if let Some(e) = &branch.condition {
                self.walk_expr(e, knot, stitch);
            }
            self.push_as_binding(branch.binding.as_ref(), knot, stitch);
            self.walk_block(&branch.body, knot, stitch);
        }
    }

    /// Index an `as` binding (B1b, issue #1475) as an ordinary local, so
    /// reads of the bound name inside the success arm resolve — and so
    /// hover/go-to-def/rename see it exactly as they see a `for`-loop
    /// variable or a block `let` (`walk_for_stmt`'s precedent).
    fn push_as_binding(
        &mut self,
        binding: Option<&crate::Name>,
        knot: Option<&str>,
        stitch: Option<&str>,
    ) {
        if let Some(name) = binding {
            self.push_local(
                name.text.clone(),
                name.range,
                SymbolKind::Temp,
                knot,
                stitch,
                None,
                None,
            );
        }
    }

    fn walk_sequence(&mut self, seq: &Sequence, knot: Option<&str>, stitch: Option<&str>) {
        for branch in &seq.branches {
            self.walk_block(&branch.body, knot, stitch);
        }
    }

    fn walk_logic_block(&mut self, lb: &LogicBlock, knot: Option<&str>, stitch: Option<&str>) {
        for bs in &lb.stmts {
            self.walk_block_stmt(bs, knot, stitch);
        }
    }

    fn walk_block_stmt(&mut self, bs: &BlockStmt, knot: Option<&str>, stitch: Option<&str>) {
        match bs {
            BlockStmt::TempDecl(t) => {
                if let Some(e) = &t.value {
                    self.walk_expr(e, knot, stitch);
                }
                self.push_local(
                    t.name.text.clone(),
                    t.name.range,
                    SymbolKind::Temp,
                    knot,
                    stitch,
                    None,
                    t.annotation.clone(),
                );
            }
            BlockStmt::Assignment(a) => {
                self.walk_expr(&a.target, knot, stitch);
                self.walk_expr(&a.value, knot, stitch);
            }
            BlockStmt::Return(r) => {
                if let Some(e) = &r.value {
                    self.walk_expr(e, knot, stitch);
                }
                for e in &r.onwards_args {
                    self.walk_expr(e, knot, stitch);
                }
            }
            BlockStmt::If(i) => self.walk_if_stmt(i, knot, stitch),
            BlockStmt::While(w) => self.walk_while_stmt(w, knot, stitch),
            BlockStmt::For(f) => self.walk_for_stmt(f, knot, stitch),
            BlockStmt::Break(_) | BlockStmt::Continue(_) => {}
            BlockStmt::ExprStmt(e) => self.walk_expr(e, knot, stitch),
            BlockStmt::Await(a) => {
                if let Some(e) = &a.condition {
                    self.walk_expr(e, knot, stitch);
                }
            }
        }
    }

    fn walk_if_stmt(&mut self, i: &IfStmt, knot: Option<&str>, stitch: Option<&str>) {
        self.walk_expr(&i.condition, knot, stitch);
        self.push_as_binding(i.binding.as_ref(), knot, stitch);
        for s in &i.body {
            self.walk_block_stmt(s, knot, stitch);
        }
        match &i.else_branch {
            Some(ElseBranch::ElseIf(inner)) => self.walk_if_stmt(inner, knot, stitch),
            Some(ElseBranch::Else(stmts)) => {
                for s in stmts {
                    self.walk_block_stmt(s, knot, stitch);
                }
            }
            None => {}
        }
    }

    fn walk_while_stmt(&mut self, w: &WhileStmt, knot: Option<&str>, stitch: Option<&str>) {
        self.walk_expr(&w.condition, knot, stitch);
        self.push_as_binding(w.binding.as_ref(), knot, stitch);
        for s in &w.body {
            self.walk_block_stmt(s, knot, stitch);
        }
    }

    fn walk_for_stmt(&mut self, f: &ForStmt, knot: Option<&str>, stitch: Option<&str>) {
        self.walk_expr(&f.iterable, knot, stitch);
        self.push_local(
            f.var_name.text.clone(),
            f.var_name.range,
            SymbolKind::Temp,
            knot,
            stitch,
            None,
            None,
        );
        // Two-binding map iteration (`for k, v in m`, B2 issue #1461): the
        // second binding is a local too, for hover/goto-def/rename parity
        // with the first.
        if let Some(val_name) = &f.val_name {
            self.push_local(
                val_name.text.clone(),
                val_name.range,
                SymbolKind::Temp,
                knot,
                stitch,
                None,
                None,
            );
        }
        for s in &f.body {
            self.walk_block_stmt(s, knot, stitch);
        }
    }

    // ─── Expressions ────────────────────────────────────────────────

    fn walk_expr(&mut self, expr: &Expr, knot: Option<&str>, stitch: Option<&str>) {
        match expr {
            Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::Null => {}
            Expr::String(s) => {
                for part in &s.parts {
                    if let StringPart::Interpolation(e) = part {
                        self.walk_expr(e, knot, stitch);
                    }
                }
            }
            Expr::Path(p) => {
                self.push_ref(path_text(p), p.range, RefKind::Variable, knot, stitch, None);
            }
            Expr::DivertTarget(p) => {
                self.push_ref(path_text(p), p.range, RefKind::Divert, knot, stitch, None);
            }
            Expr::ListLiteral(items) => {
                for p in items {
                    self.push_ref(path_text(p), p.range, RefKind::List, knot, stitch, None);
                }
            }
            Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => {
                self.walk_expr(inner, knot, stitch);
            }
            Expr::Infix(ie) => {
                self.walk_expr(&ie.lhs, knot, stitch);
                self.walk_expr(&ie.rhs, knot, stitch);
            }
            Expr::Call(path, args) => {
                // `path.range` here — the callee `Path`'s own *whole* span —
                // is the origin of the call-path `ResolvedRef::range`
                // contract four downstream consumers key lookups on
                // unchanged; see that field's doc (issue #1561). Never
                // narrow this to a sub-segment.
                self.push_ref(
                    path_text(path),
                    path.range,
                    RefKind::Function,
                    knot,
                    stitch,
                    Some(args.len()),
                );
                for a in args {
                    self.walk_expr(a, knot, stitch);
                }
            }
            Expr::ArrayLiteral(a) => {
                for e in &a.elements {
                    self.walk_expr(e, knot, stitch);
                }
            }
            Expr::MapLiteral(m) => {
                for (k, v) in &m.entries {
                    self.walk_expr(k, knot, stitch);
                    self.walk_expr(v, knot, stitch);
                }
            }
            Expr::Index(idx) => {
                self.walk_expr(&idx.base, knot, stitch);
                self.walk_expr(&idx.index, knot, stitch);
            }
            Expr::Range(r) => {
                self.walk_expr(&r.start, knot, stitch);
                self.walk_expr(&r.end, knot, stitch);
            }
            Expr::StructLiteral(sl) => {
                self.push_ref(
                    sl.shape.text.clone(),
                    sl.shape.range,
                    RefKind::Struct,
                    knot,
                    stitch,
                    None,
                );
                for (_, v) in &sl.fields {
                    self.walk_expr(v, knot, stitch);
                }
            }
            Expr::FieldAccess(fa) => self.walk_expr(&fa.base, knot, stitch),
            Expr::FnLiteral(fl) => {
                // `arg_count` stays `None` here (never `Some(fl.args.len())`)
                // — `#fn` binds a *prefix* of the param row, unlike a direct
                // call, so full-arity checking doesn't apply (see
                // `hir::lower::expr::sigils`'s `FnLiteral` lowering doc).
                self.push_ref(
                    path_text(&fl.target),
                    fl.target.range,
                    RefKind::Function,
                    knot,
                    stitch,
                    None,
                );
                for a in &fl.args {
                    self.walk_expr(a, knot, stitch);
                }
            }
            Expr::RefArg(ra) => self.walk_expr(&ra.operand, knot, stitch),
            // A lambda (issue #1685) introduces locals — its params — and
            // then a body that reads them. The params are recorded with the
            // same `SymbolKind::Temp` a `for` binding and a `let` get: they
            // are bindings a construct introduces inside a body, not
            // declaration-header params, and recording them is what lets a
            // reference to `g` inside `|g| g.awake` resolve at all (as well
            // as giving hover/goto-def/rename the same handle they have on
            // every other local).
            Expr::Lambda(l) => self.walk_lambda(l, knot, stitch),
            // Block capture (issue #1839): the captured run is real body
            // content — a reference/call inside it needs the identical
            // symbol-table entries (hover/goto-def/rename) it would get at
            // its original top-level position, so it walks through
            // `walk_stmt` exactly as it did before capture.
            Expr::Fragment(stmts) => {
                for s in stmts {
                    self.walk_stmt(s, knot, stitch);
                }
            }
        }
    }

    /// A lambda (issue #1685) introduces locals — its params — and then a
    /// body that reads them. The params are recorded with the same
    /// `SymbolKind::Temp` a `for` binding and a `let` get: they are
    /// bindings a construct introduces inside a body, not
    /// declaration-header params, and recording them is what lets a
    /// reference to `g` inside `|g| g.awake` resolve at all (as well as
    /// giving hover/goto-def/rename the same handle they have on every
    /// other local).
    fn walk_lambda(&mut self, l: &LambdaExpr, knot: Option<&str>, stitch: Option<&str>) {
        for p in &l.params {
            self.push_local(
                p.name.text.clone(),
                p.name.range,
                SymbolKind::Temp,
                knot,
                stitch,
                None,
                None,
            );
        }
        match &l.body {
            LambdaBody::Expr(e) => self.walk_expr(e, knot, stitch),
            LambdaBody::Block { stmts, tail } => {
                for s in stmts {
                    self.walk_block_stmt(s, knot, stitch);
                }
                if let Some(t) = tail {
                    self.walk_expr(t, knot, stitch);
                }
            }
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────

fn param_info(p: &Param) -> ParamInfo {
    ParamInfo {
        name: p.name.text.clone(),
        is_ref: p.is_ref,
        is_divert: p.is_divert,
    }
}

fn param_infos(params: &[Param]) -> Vec<ParamInfo> {
    params.iter().map(param_info).collect()
}

/// Dot-joined path text (`crate::hir::lower::helpers::path_full_name`'s
/// twin — duplicated rather than reached-into, since that helper is
/// `pub(crate)` to `hir::lower` and this module has no other reason to
/// depend on the lowering module tree).
fn path_text(path: &Path) -> String {
    path.segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

// ─── Tests ──────────────────────────────────────────────────────────
//
// These are the projection's *own* unit tests — the ones B0.3's admission
// check #1 (manifest⇄HIR agreement) retires into, per
// `docs/b0-sequencing.md` §B0.4: "you cannot disagree with yourself once
// the manifest IS a projection of HIR". Direct, fixture-driven assertions
// on `project_manifest`'s output for the shapes that matter — declared
// symbols with every metadata channel (doc/visibility/was/params), locals
// with their scope, every `RefKind`, and label qualification. The
// differential burn-in test
// (`crates/internal/brink-test-harness/tests/b04_manifest_burn_in.rs`) is
// the corpus-wide proof this matches production; these are the readable,
// single-purpose regression tests for each shape.

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test-only unwrap_or_else(|| panic!(...)) assertion helpers"
)]
mod tests {
    use brink_syntax::parse;

    use super::*;
    use crate::FileId;

    fn lower(source: &str) -> HirFile {
        let parsed = parse(source);
        let tree = parsed.tree();
        let (hir, _legacy_manifest, diags) = crate::hir::lower(FileId(0), &tree);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        hir
    }

    #[test]
    fn docs_project_for_every_declaration_kind() {
        let hir = lower(
            "\
/// An external.
EXTERNAL ping(x)
/// A variable.
VAR health = 100
/// A constant.
CONST SPEED = 0.5
/// A list.
LIST mood = happy, sad
/// A knot.
== hub ==
intro
/// A nested stitch.
= market
stalls
/// A function knot.
== function damage(weapon) ==
~ return 1
",
        );
        let manifest = project_manifest(&hir);

        let doc_text = |kind: SymbolKind, name: &str| {
            manifest
                .docs
                .get(&(kind, name.to_string()))
                .unwrap_or_else(|| panic!("doc for {kind:?} {name}"))
                .doc
                .clone()
        };
        assert_eq!(
            doc_text(SymbolKind::External, "ping").as_deref(),
            Some("An external.")
        );
        assert_eq!(
            doc_text(SymbolKind::Variable, "health").as_deref(),
            Some("A variable.")
        );
        assert_eq!(
            doc_text(SymbolKind::Constant, "SPEED").as_deref(),
            Some("A constant.")
        );
        assert_eq!(
            doc_text(SymbolKind::List, "mood").as_deref(),
            Some("A list.")
        );
        assert_eq!(
            doc_text(SymbolKind::Knot, "hub").as_deref(),
            Some("A knot.")
        );
        assert_eq!(
            doc_text(SymbolKind::Stitch, "hub.market").as_deref(),
            Some("A nested stitch."),
            "nested stitch docs are keyed by qualified name"
        );
        assert_eq!(
            doc_text(SymbolKind::Knot, "damage").as_deref(),
            Some("A function knot.")
        );
    }

    #[test]
    fn visibility_and_was_project_onto_declared_symbols() {
        let hir = lower(
            "\
== hub ==
#@was(old_hub)
#@private
Hello
-> END

#@was(old_health)
VAR health = 100
",
        );
        let manifest = project_manifest(&hir);

        assert_eq!(
            manifest.variables[0].was.as_ref().map(|(n, _)| n.as_str()),
            Some("old_health")
        );
        assert_eq!(manifest.knots[0].visibility, Some(VisibilityMark::Private));
        assert_eq!(
            manifest.knots[0].was.as_ref().map(|(n, _)| n.as_str()),
            Some("old_hub")
        );
    }

    #[test]
    fn external_params_keep_their_names_not_just_a_count() {
        let hir = lower("EXTERNAL greet(name, times)\n");
        let manifest = project_manifest(&hir);

        let ext = &manifest.externals[0];
        assert_eq!(
            ext.params
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            vec!["name", "times"]
        );
    }

    #[test]
    fn list_items_project_with_qualified_names() {
        let hir = lower("LIST mood = happy, (sad), angry\n");
        let manifest = project_manifest(&hir);

        let names: Vec<_> = manifest
            .list_items
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, vec!["mood.happy", "mood.sad", "mood.angry"]);
    }

    #[test]
    fn promoted_top_level_stitch_declares_bare_stitch_not_knot() {
        let hir = lower("= market\nstalls\n-> END\n");
        let manifest = project_manifest(&hir);

        assert!(manifest.knots.is_empty(), "no real knot in this file");
        assert_eq!(manifest.stitches.len(), 1);
        assert_eq!(manifest.stitches[0].name, "market");
    }

    #[test]
    fn params_project_as_locals_scoped_to_their_container() {
        let hir = lower(
            "\
== hub(gold) ==
= market(item)
buy {item} for {gold}
-> END
",
        );
        let manifest = project_manifest(&hir);

        let hub_gold = manifest
            .locals
            .iter()
            .find(|l| l.name == "gold")
            .expect("knot param `gold` is a local");
        assert_eq!(hub_gold.kind, SymbolKind::Param);
        assert_eq!(hub_gold.scope.knot.as_deref(), Some("hub"));
        assert_eq!(hub_gold.scope.stitch, None);

        let market_item = manifest
            .locals
            .iter()
            .find(|l| l.name == "item")
            .expect("stitch param `item` is a local");
        assert_eq!(market_item.scope.knot.as_deref(), Some("hub"));
        assert_eq!(market_item.scope.stitch.as_deref(), Some("market"));
    }

    #[test]
    fn temp_decl_and_for_loop_binding_project_as_temp_locals() {
        let hir = lower(
            "\
== hub ==
~ temp x = 1
~ { for y in #[1, 2, 3] { } }
-> END
",
        );
        let manifest = project_manifest(&hir);

        let temp_names: Vec<_> = manifest
            .locals
            .iter()
            .filter(|l| l.kind == SymbolKind::Temp)
            .map(|l| l.name.as_str())
            .collect();
        assert!(temp_names.contains(&"x"), "{temp_names:?}");
        assert!(temp_names.contains(&"y"), "{temp_names:?}");
    }

    #[test]
    fn every_ref_kind_projects_with_the_right_scope_and_arg_count() {
        let hir = lower(
            "\
VAR g = 0
LIST L = a, b
STRUCT Point = #{ x: int }
EXTERNAL beep(n)

== hub ==
{g}
~ beep(1, 2)
~ temp chosen = (a, b)
~ temp p = Point#{ x: 1 }
-> away

=== away ===
-> END
",
        );
        let manifest = project_manifest(&hir);

        let find = |kind: RefKind, path: &str| {
            manifest
                .unresolved
                .iter()
                .find(|r| r.kind == kind && r.path == path)
                .unwrap_or_else(|| {
                    panic!(
                        "expected a {kind:?} ref to `{path}`: {:?}",
                        manifest.unresolved
                    )
                })
        };

        let variable = find(RefKind::Variable, "g");
        assert_eq!(variable.scope.knot.as_deref(), Some("hub"));
        assert_eq!(variable.arg_count, None);

        let func = find(RefKind::Function, "beep");
        assert_eq!(func.arg_count, Some(2));

        let list = find(RefKind::List, "a");
        assert_eq!(list.arg_count, None);

        let strukt = find(RefKind::Struct, "Point");
        assert_eq!(strukt.arg_count, None);

        // Issue #2156: a bare `-> away` (no call-args syntax) now records
        // `Some(0)`, not `None` — `arg_count` is always `Some(target.args.len())`
        // for a divert ref (0 for a bare divert), so `resolve_divert`'s arity
        // check (`E176`) can run uniformly rather than being permanently
        // gated off by a hardcoded `None`.
        let divert = find(RefKind::Divert, "away");
        assert_eq!(divert.arg_count, Some(0));
    }

    #[test]
    fn choice_and_gather_labels_project_with_qualified_names() {
        let hir = lower(
            "\
== hub ==
* (opener) [Go] Onward.
- (settle) Settled.
-> END
",
        );
        let manifest = project_manifest(&hir);

        let names: Vec<_> = manifest.labels.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"hub.opener"), "{names:?}");
        assert!(names.contains(&"hub.settle"), "{names:?}");
    }
}
