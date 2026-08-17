//! B1 `or`-coalescing typing: the recorded operand/result types
//! ([`resolve`]) and the static mismatch check derived from them
//! ([`check`]) — `docs/stdlib-spec.md` §1.6a, issues #1460 and #1492.
//!
//! ## Two halves of one pass (issue #1492)
//!
//! RULED (maintainer, 2026-07-26, `docs/decision-log.md` "Lowering consumes
//! analyzer types"): **typing verdicts belong to the analyzer; lowering
//! consumes recorded types, never re-derives them.** A syntactic
//! shape-sniff in LIR lowering cannot see through an `Expr::Call` to its
//! declared return type, nor through a bare `Path` to a `VAR`/temp declared
//! `Option[T]` — both are type questions, and the answer already exists
//! here.
//!
//! So this pass produces two things from one walk:
//!
//! - a [`CoalesceTable`] — the `node → verdict` side channel LIR lowering
//!   reads to pick a chain's code shape ("inner stays `Option`" vs "unwrap
//!   at the end"), reusing #1482's [`SideTable`] plumbing verbatim rather
//!   than inventing a second mechanism; and
//! - the `E066` diagnostics, which are now *derived from the recorded
//!   verdicts* rather than computed alongside them, so a chain that lowers
//!   and a chain that is rejected can never disagree about its own types.
//!
//! ## Chains, and why the table is keyed at the chain root
//!
//! `a or b or c` is a left-associative `Expr::Infix(Infix(a, or, b), or,
//! c)`. One entry is recorded per **chain root**, carrying every step's
//! verdict in innermost-first order ([`CoalesceChain::steps`]), because the
//! fold is what produces the verdicts: a step's left-hand type is the
//! previous step's *result*, not anything re-derivable from the spine node
//! in isolation.
//!
//! The root's [`NodeKey`] is `brink_ir::hir::expr_span` of the root — since
//! issue #1517 the root `Expr::Infix`'s **own `Provenance` range**, which
//! strictly contains its left operand's, so a chain and its own left spine
//! are always distinct keys. Before #1517 they were not (an infix node had
//! no provenance and a trailing scalar literal contributed no range, so
//! `some(a) or f() or 99` keyed identically to `some(a) or f()`), and this
//! pass had to poison any key two roots would share. That workaround is
//! gone; nothing here drops an entry to avoid an ambiguous key.
//!
//! Absence is still always safe: a consumer with no verdict falls back to
//! the runtime check, which is what gradual mode does anyway.
//!
//! ## The old shape, retained
//!
//! `infer::ty::coalesce`'s two failure shapes (`CoalesceError::LeftNotOption`,
//! `CoalesceError::Mismatch`) were being silently absorbed into
//! `Ty::Conflicted` by `infer::body::InferPass::infer_infix`'s
//! `InfixOp::Coalesce` arm, with no diagnostic ever raised at the
//! coalescing expression itself. The arm's own doc comment claimed the
//! generic `E066` Conflicted-escape check (`strict::check`) was a
//! sufficient backstop — it is not: that check only fires once a
//! `Conflicted` value reaches a *signature or body-local slot* boundary,
//! which a coalescing expression used directly in content/argument
//! position (`{some(1) or "text"}`, never bound to a slot) never does. This
//! module closes that gap directly, at the coalescing expression's own
//! site, mirroring `conversions`/`range_refinement`/`option_conditions`'s
//! own strict-mode-only, expression-position posture exactly (the same
//! `strict::check` wiring point, the same `structs::classify_expr_ty`
//! inference-substrate classification, the same "Unknown never disagrees —
//! stays silently unchecked, the runtime fault is the residual backstop"
//! posture for anything not statically classifiable).
//!
//! Folding the chain (issue #1492) *widens* that check: before, only a
//! chain's innermost step was ever judged, because `classify_expr_ty`
//! returns `None` for an `Expr::Infix` operand, so `{some(1) or none or
//! "text"}` passed analysis silently. The fold feeds each step's recorded
//! result type in as the next step's left-hand type, so every step is
//! judged — the issue's "an ill-typed chain never reaches lowering".
//!
//! Strict-mode-only: under `types = gradual` (including native's
//! un-overridden default — B0.10's dialect-keyed strict-only wiring has not
//! landed, `strict.rs`'s own `native_strict_only_error` doc) this module is
//! never invoked, and the runtime `TypeError` fault
//! (`brink_runtime::value_ops::coalesce_unwrap_some`, backing
//! `Opcode::CoalesceSome` — issue #1471 replaced the binary `Coalesce`
//! opcode this originally named with a short-circuiting branch) is the sole
//! backstop — see that
//! function's doc for the fault's actual (narrower-than-previously-claimed)
//! coverage: it only catches a non-Option left-hand side, not a mismatched
//! fallback type.
//!
//! Reuses `infer::ty::coalesce` itself — the identical typing rule
//! `infer::body::InferPass::infer_infix`'s `InfixOp::Coalesce` arm calls —
//! rather than re-deriving a parallel mismatch rule, so the two can never
//! drift apart.

use std::collections::{BTreeMap, BTreeSet};

use brink_format::DefinitionId;
use brink_ir::hir::expr_span;
use brink_ir::hir::visit::{self, ContentContext, HirVisitor};
use brink_ir::{
    Choice, ConstDecl, Content, Diagnostic, DiagnosticCode, Expr, FileId, HirFile, InfixOp, Knot,
    ResolutionMap, Stitch, Stmt, SymbolIndex, SymbolKind, VarDecl,
};
use rowan::TextRange;

use crate::annotations;
use crate::infer::{self, CoalesceError, InferenceResult, InferredSig, Ty};
use crate::structs::{self, MistypeCtx};
use crate::ufcs::{NodeKey, SideTable};

// ─── The recorded verdict (issue #1492) ──────────────────────────────

/// Which of `infer::ty::coalesce`'s three outcomes one `or` step took —
/// the shape question LIR lowering asks, answered from types instead of
/// syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoalesceShape {
    /// `Option[T] or Option[U]` — optionality survives the step, so the
    /// step's value stays an `Option` for whatever consumes it (the next
    /// step of a chain, or the expression's own consumer).
    PreserveOption,
    /// `Option[T] or U` — the step collapses to the plain value type.
    Collapse,
    /// The left-hand type is not statically pinned (`Ty::Unknown` /
    /// `Ty::Conflicted` — gradual mode, or a strict escape already reported
    /// by `E065`/`E066`).
    ///
    /// **The runtime check is the semantics here** (RULED 2026-07-26, and
    /// documented on `brink_format::Opcode::CoalesceSome`): an `Option` value
    /// coalesces, a plain value faults, exactly like every other gradual
    /// runtime check. A consumer must not statically commit to either
    /// shape on this verdict.
    RuntimeCheck,
}

/// The recorded types of one `or` step, in the left-associative order the
/// grammar builds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoalesceStep {
    /// The left-hand type. For the innermost step this is the classified
    /// operand; for every later step it is the previous step's `result`.
    pub lhs: Ty,
    /// The fallback operand's classified type.
    pub rhs: Ty,
    /// `infer::ty::coalesce(lhs, rhs)` — the step's value type.
    pub result: Ty,
    /// The shape [`Self::result`] implies for a consumer.
    pub shape: CoalesceShape,
}

/// Every step of one `or`-coalescing chain, innermost first.
///
/// `a or b or c` records two steps: `[a or b, (that) or c]`. A consumer
/// walking the HIR meets the chain root *first* and descends its left
/// spine, so it consumes this vector back-to-front; the order is fixed
/// here (and only here) so producer and consumer cannot drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoalesceChain {
    /// The chain's steps, innermost first. Never empty.
    pub steps: Vec<CoalesceStep>,
}

/// Every `or`-coalescing chain's recorded typing, keyed at the chain root
/// (see the module doc for why not per step).
pub type CoalesceTable = SideTable<CoalesceChain>;

/// Translate a [`CoalesceTable`] into `brink-ir`'s own lowering-facing
/// mirror — the **one** translation point between the two crates (issue
/// #1471), exactly as [`crate::ufcs_lir_lookup`] is for the UFCS table.
///
/// `brink-ir` sits below `brink-analyzer` in the crate graph, so it cannot
/// name [`CoalesceShape`]; `brink_ir::lir::CoalesceShape`'s own doc explains
/// the mirror. Only the per-step *shape* crosses — the recorded `Ty`s
/// themselves are analysis detail lowering has no use for. Step order
/// (innermost first) is preserved verbatim: it is the order
/// `lir::lower::expr::lower_coalesce_chain` folds a chain in.
#[must_use]
pub fn to_lir_lookup(table: &CoalesceTable) -> brink_ir::lir::CoalesceLookup {
    let entries = table
        .iter()
        .map(|(key, chain)| {
            let range = TextRange::new(key.range.0.into(), key.range.1.into());
            let shapes = chain
                .steps
                .iter()
                .map(|step| match step.shape {
                    CoalesceShape::PreserveOption => brink_ir::lir::CoalesceShape::PreserveOption,
                    CoalesceShape::Collapse => brink_ir::lir::CoalesceShape::Collapse,
                    CoalesceShape::RuntimeCheck => brink_ir::lir::CoalesceShape::RuntimeCheck,
                })
                .collect();
            (key.file, range, shapes)
        })
        .collect();
    brink_ir::lir::CoalesceLookup::from_entries(entries)
}

/// Cheap structural scan: does any expression in `hir` coalesce? The
/// laziness gate for [`resolve`]'s caller — a project with no `or`-coalescing
/// anywhere (every ink-dialect project, by construction: `InfixOp::Coalesce`
/// is native-lowering-only) never triggers whole-project inference on this
/// pass's account, mirroring [`crate::project_has_ufcs_call`]'s own shape.
///
/// Covers the file-level `VAR`/`CONST` initializers too (issue #2098: via
/// [`visit::visit_with_decl_initializers`], not a hand-rolled second walk —
/// `Scan` has no state that needs resetting per-decl, so the shared entry
/// point alone covers both the block tree and every initializer).
#[must_use]
pub fn project_has_coalesce(hir: &HirFile) -> bool {
    struct Scan {
        found: bool,
    }
    impl HirVisitor for Scan {
        fn visit_exprs(&self) -> bool {
            true
        }
        fn enter_expr(&mut self, expr: &Expr) {
            if coalesce_operands(expr).is_some() {
                self.found = true;
            }
        }
    }
    let mut scan = Scan { found: false };
    visit::visit_with_decl_initializers(hir, &mut scan);
    scan.found
}

/// Record every `or`-coalescing chain's operand/result types and report the
/// `E066` mismatches that fall out of the same fold.
///
/// Callers only reach this once `strict::config_error` has confirmed
/// `types = strict` + `dialect = brink` (mirrors `conversions::check`'s own
/// entry condition — same wiring point, `strict::check`) — *for the
/// diagnostics*. The table half is served separately through
/// [`crate::coalesce_types`], mirroring `ufcs_resolution`'s split for the
/// same reason: the two consumers want opposite halves of one result and
/// neither should pay for the other's.
#[must_use]
pub fn resolve(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
    resolutions: &ResolutionMap,
) -> (CoalesceTable, Vec<Diagnostic>) {
    let globals = crate::infer::collect_globals(files, index, None);
    let mut out = Vec::new();
    let mut table = CoalesceTable::new();
    for &(file, hir) in files {
        let resolution_by_range = resolution_index(resolutions, file);
        let mut v = CoalesceVisitor {
            file,
            index,
            globals: &globals,
            signatures: &inference.signatures,
            bodies: &inference.bodies,
            resolution_by_range: &resolution_by_range,
            current_knot_name: None,
            knot_locals: None,
            stitch_locals: None,
            fallback: TextRange::new(0.into(), 0.into()),
            spine: BTreeSet::new(),
            table: &mut table,
            lambda_locals: Vec::new(),
            diagnostics: &mut out,
        };
        // Issue #2098: `CoalesceVisitor::enter_var_decl`/`enter_const_decl`
        // reset `fallback` (and the knot/stitch locals) to the declaration's
        // own scope before its initializer's expressions arrive, so the
        // shared entry point covers the block tree and every file-level
        // declaration's own initializer in one drive — the hand-rolled
        // `check_expr`/`expr_children` mirror of `visit::visit`'s own
        // descent this used to need is gone.
        visit::visit_with_decl_initializers(hir, &mut v);
    }
    (table, out)
}

/// Strict-mode-only `or`-coalescing mismatch checks over every
/// `InfixOp::Coalesce` expression in the project — [`resolve`]'s diagnostic
/// half, the shape `strict::check` wires in.
#[must_use]
pub fn check(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
    resolutions: &ResolutionMap,
) -> Vec<Diagnostic> {
    resolve(files, index, inference, resolutions).1
}

struct CoalesceVisitor<'a> {
    file: FileId,
    index: &'a SymbolIndex,
    globals: &'a BTreeMap<DefinitionId, Ty>,
    signatures: &'a BTreeMap<DefinitionId, InferredSig>,
    bodies: &'a BTreeMap<DefinitionId, crate::infer::BodyTypes>,
    resolution_by_range: &'a BTreeMap<(u32, u32), DefinitionId>,
    /// The currently-open knot's own name — `enter_stitch` needs it to
    /// reconstruct the qualified `knot.stitch` name a stitch is indexed
    /// under. Mirrors `structs::ConstructionVisitor`'s identical field.
    current_knot_name: Option<String>,
    /// The enclosing knot's own finalized locals, set for the duration of
    /// its body (and every stitch nested inside it, until `enter_stitch`
    /// overrides it with the stitch's own). Mirrors
    /// `structs::ConstructionVisitor`'s identical field.
    knot_locals: Option<&'a BTreeMap<String, Ty>>,
    /// The currently-open stitch's own finalized locals, if any — takes
    /// priority over `knot_locals` while set.
    stitch_locals: Option<&'a BTreeMap<String, Ty>>,
    /// Diagnostic anchor of last resort: the nearest enclosing statement's
    /// (or content line's, or choice's) own `Provenance` range, updated as
    /// the walk descends. A coalescing operand carries its own tighter
    /// range whenever [`expr_anchor`] can find one (a path, a call's
    /// callee); this is only reached for operand shapes with none of their
    /// own (a bare literal — `{5 or 9}`, the review-finding fixture).
    fallback: TextRange,
    /// Addresses of the coalescing nodes already consumed as part of an
    /// enclosing chain. `walk_expr` calls `enter_expr` on every node of a
    /// chain's left spine as well as on its root, but a chain is analysed
    /// (and recorded) exactly once, at its root; this is how the spine
    /// nodes are recognized on the way past. Addresses only — never
    /// dereferenced, and stable because the HIR is not mutated during the
    /// walk. Membership only — never iterated — but a `BTreeSet` anyway,
    /// per the crate's determinism lint.
    spine: BTreeSet<usize>,
    /// The chain-root verdicts recorded so far.
    table: &'a mut CoalesceTable,
    /// Issue #2773: a stack of pruned-locals frames, one per currently-open
    /// lambda literal (innermost last). Mirrors
    /// `structs::ConstructionVisitor`'s identical field/hook pair exactly —
    /// see that field's own doc.
    lambda_locals: Vec<BTreeMap<String, Ty>>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl CoalesceVisitor<'_> {
    fn current_locals(&self) -> Option<&BTreeMap<String, Ty>> {
        self.lambda_locals
            .last()
            .or_else(|| self.stitch_locals.or(self.knot_locals))
    }

    /// The `DefinitionId` a knot/stitch's own name resolves to — mirrors
    /// `structs::ConstructionVisitor::knot_def_id` exactly (same #626
    /// top-level-stitch-promoted-to-knot rationale).
    fn knot_def_id(&self, knot: &Knot) -> Option<DefinitionId> {
        let kind = knot.symbol_kind();
        annotations::def_id_for(self.index, self.file, kind, &knot.name.text)
    }
}

impl HirVisitor for CoalesceVisitor<'_> {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_knot(&mut self, knot: &Knot) {
        self.current_knot_name = Some(knot.name.text.clone());
        self.knot_locals = self
            .knot_def_id(knot)
            .and_then(|id| self.bodies.get(&id))
            .map(|b| &b.locals);
    }

    fn exit_knot(&mut self, _knot: &Knot) {
        self.current_knot_name = None;
        self.knot_locals = None;
    }

    fn enter_stitch(&mut self, stitch: &Stitch) {
        // Stitches are indexed by qualified `knot.stitch` name — mirrors
        // `structs::ConstructionVisitor::enter_stitch` exactly.
        self.stitch_locals = self.current_knot_name.as_ref().and_then(|knot_name| {
            let qualified = format!("{knot_name}.{}", stitch.name.text);
            annotations::def_id_for(self.index, self.file, SymbolKind::Stitch, &qualified)
                .and_then(|id| self.bodies.get(&id))
                .map(|b| &b.locals)
        });
    }

    fn exit_stitch(&mut self, _stitch: &Stitch) {
        self.stitch_locals = None;
    }

    /// Issue #2098: a file-level `VAR` sits outside any knot/stitch, so its
    /// initializer needs the same "no enclosing def" reset `exit_knot`/
    /// `exit_stitch` already give the walk when it *leaves* one — plus its
    /// own range as the diagnostic anchor of last resort (this replaces the
    /// hand-rolled `check_expr` recursion's explicit `fallback` parameter,
    /// which passed `var.ptr.text_range()` for exactly this reason).
    fn enter_var_decl(&mut self, var: &VarDecl) {
        self.fallback = var.ptr.text_range();
        self.current_knot_name = None;
        self.knot_locals = None;
        self.stitch_locals = None;
    }

    /// [`HirVisitor::enter_var_decl`]'s `CONST` twin.
    fn enter_const_decl(&mut self, konst: &ConstDecl) {
        self.fallback = konst.ptr.text_range();
        self.current_knot_name = None;
        self.knot_locals = None;
        self.stitch_locals = None;
    }

    fn enter_stmt(&mut self, stmt: &Stmt) {
        if let Some(range) = stmt_anchor(stmt) {
            self.fallback = range;
        }
    }

    fn enter_content(&mut self, content: &Content, _ctx: ContentContext) {
        if let Some(ptr) = content.ptr {
            self.fallback = ptr.text_range();
        }
    }

    fn enter_choice(&mut self, choice: &Choice) {
        self.fallback = choice.ptr.text_range();
    }

    fn enter_expr(&mut self, expr: &Expr) {
        // A chain's left spine is analysed at its root, so the spine nodes
        // `walk_expr` hands over on the way down are consumed and dropped.
        if self.spine.remove(&std::ptr::from_ref(expr).addr()) {
            return;
        }
        if coalesce_operands(expr).is_none() {
            return;
        }
        for node in chain_spine(expr).iter().skip(1) {
            self.spine.insert(std::ptr::from_ref(*node).addr());
        }
        // Built from direct field projections (not `self.ctx()`) so the
        // borrow checker sees this only borrows the locals-shaped fields,
        // disjoint from the `self.table`/`self.diagnostics` reborrows below
        // — see `structs::ConstructionVisitor::enter_expr`'s identical
        // comment.
        let ctx = MistypeCtx {
            index: self.index,
            globals: self.globals,
            signatures: self.signatures,
            resolution_by_range: self.resolution_by_range,
            locals: self
                .lambda_locals
                .last()
                .or_else(|| self.stitch_locals.or(self.knot_locals)),
        };
        analyze_chain(
            expr,
            self.fallback,
            self.file,
            &ctx,
            self.table,
            self.diagnostics,
        );
    }

    fn enter_lambda(&mut self, l: &brink_ir::LambdaExpr) {
        let pruned = structs::pruned_locals_for_lambda(l, self.index, self.current_locals());
        self.lambda_locals.push(pruned);
    }

    fn exit_lambda(&mut self, _l: &brink_ir::LambdaExpr) {
        self.lambda_locals.pop();
    }
}

/// The two operands of a coalescing node, or `None` for anything else.
fn coalesce_operands(expr: &Expr) -> Option<(&Expr, &Expr)> {
    match expr {
        Expr::Infix(ie) if ie.op == InfixOp::Coalesce => Some((&ie.lhs, &ie.rhs)),
        _ => None,
    }
}

/// The coalescing chain rooted at `root`, **outermost first**: `root`
/// itself, then its left-hand operand for as long as that is a coalescing
/// node too. `a or b or c` yields `[(… or c), (a or b)]`.
///
/// Empty when `root` is not a coalescing node at all.
fn chain_spine(root: &Expr) -> Vec<&Expr> {
    let mut spine = Vec::new();
    let mut cursor = root;
    while let Some((lhs, _)) = coalesce_operands(cursor) {
        spine.push(cursor);
        cursor = lhs;
    }
    spine
}

/// Fold the coalescing chain rooted at `root`, left-associatively: classify
/// the innermost left-hand operand once, then feed each step's result type
/// in as the next step's left-hand type ([`infer::coalesce`] — the same rule
/// `infer::body`'s own `InfixOp::Coalesce` arm calls, never a parallel one).
///
/// A step that types cleanly is recorded; the first step that disagrees
/// raises `E066` and abandons the chain (nothing is recorded — an ill-typed
/// chain must never hand a consumer a verdict). The innermost left-hand
/// operand not classifying to a statically-known [`Ty`] (an untyped
/// parameter with no other use, say) does **not** abandon the chain: it is
/// recorded as [`Ty::Unknown`], which `infer::coalesce` always accepts
/// (`(Unknown, _) -> Ok(Unknown)`, never an error), so the step is recorded
/// with [`CoalesceShape::RuntimeCheck`] — the unpinned-`lhs` posture
/// `Opcode::CoalesceSome`'s own doc describes, and it propagates: every later
/// step folds from `Unknown` too. A step whose *fallback* operand does not
/// classify is different — the shape question ("is the fallback
/// `Option`-shaped?") has no safe unpinned answer, so that abandons the
/// chain silently, the same "Unknown never disagrees" posture every
/// sibling module in this crate takes; the runtime fault remains the
/// backstop.
///
/// The verdict is keyed by [`expr_span`] of the root — the derivation LIR
/// lowering shares, in `brink-ir`, so producer and consumer cannot drift.
/// Since issue #1517 that is the root `Expr::Infix`'s own `Provenance`
/// range, so every chain root in a file has its own key and there is no
/// ambiguity to guard against.
fn analyze_chain(
    root: &Expr,
    fallback: TextRange,
    file: FileId,
    ctx: &MistypeCtx<'_>,
    table: &mut CoalesceTable,
    out: &mut Vec<Diagnostic>,
) {
    let spine = chain_spine(root);
    let mut steps = Vec::with_capacity(spine.len());
    let mut carried: Option<Ty> = None;
    for node in spine.iter().rev() {
        let Some((lhs_expr, rhs_expr)) = coalesce_operands(node) else {
            return;
        };
        let lhs = match carried.take() {
            Some(ty) => ty,
            // An unclassifiable innermost left-hand operand is recorded as
            // `Unknown`, not bailed on: `infer::coalesce`'s `(Unknown, _)`
            // arm always accepts it, so this yields a real
            // `CoalesceShape::RuntimeCheck` step instead of silently
            // recording nothing (see this function's own doc).
            None => classify_coalesce_operand(lhs_expr, ctx).unwrap_or(Ty::Unknown),
        };
        let Some(rhs) = classify_coalesce_operand(rhs_expr, ctx) else {
            return;
        };
        match infer::coalesce(&lhs, &rhs) {
            Ok(result) => {
                let shape = step_shape(&lhs, &rhs);
                carried = Some(result.clone());
                steps.push(CoalesceStep {
                    lhs,
                    rhs,
                    result,
                    shape,
                });
            }
            Err(err) => {
                let range = expr_anchor(lhs_expr)
                    .or_else(|| expr_anchor(rhs_expr))
                    .unwrap_or(fallback);
                out.push(Diagnostic {
                    file,
                    range,
                    message: coalesce_error_message(&err),
                    code: DiagnosticCode::E066,
                });
                return;
            }
        }
    }
    if steps.is_empty() {
        return;
    }
    let Some(range) = expr_span(root) else {
        return;
    };
    table.insert(NodeKey::new(file, range), CoalesceChain { steps });
}

/// Which shape one step's operand types imply, per the `docs/stdlib-spec.md`
/// §1.6a rule [`infer::coalesce`] encodes: an `Option` fallback keeps
/// optionality, a plain fallback collapses, and an unpinned left-hand type
/// commits to neither.
fn step_shape(lhs: &Ty, rhs: &Ty) -> CoalesceShape {
    if matches!(lhs, Ty::Unknown | Ty::Conflicted) {
        return CoalesceShape::RuntimeCheck;
    }
    if matches!(rhs, Ty::Option(_)) {
        CoalesceShape::PreserveOption
    } else {
        CoalesceShape::Collapse
    }
}

fn coalesce_error_message(err: &CoalesceError) -> String {
    match err {
        CoalesceError::LeftNotOption(ty) => format!(
            "{}: `or`-coalescing requires an `Option[T]` left-hand side (docs/stdlib-spec.md \
             §1.6a) — found `{}`",
            DiagnosticCode::E066.title(),
            ty.display(),
        ),
        CoalesceError::Mismatch { element, fallback } => format!(
            "{}: `or`-coalescing's fallback type disagrees with the `Option`'s element type \
             (docs/stdlib-spec.md §1.6a) — `{}` vs `{}`",
            DiagnosticCode::E066.title(),
            element.display(),
            fallback.display(),
        ),
    }
}

/// Classify a coalescing operand's own statically-known type —
/// [`structs::classify_expr_ty`]'s existing inference-substrate
/// classification (Path/resolved-Call/Index/literals) first, extended with
/// the two shapes it doesn't cover and
/// `option_conditions::condition_is_option` already special-cases for the
/// identical reason: an unresolved (builtin, not author-shadowed) call to
/// an Option-returning intrinsic, and the bare unresolved `none` literal.
/// `some(x)` additionally classifies its own inner element, recursively —
/// the one extra step `condition_is_option` doesn't need (it only cares
/// whether the type is `Option`, not what element it carries).
fn classify_coalesce_operand(expr: &Expr, ctx: &MistypeCtx<'_>) -> Option<Ty> {
    match expr {
        Expr::Call(path, args) => {
            // `path.range` here is the same call-path `ResolvedRef::range`
            // key `lir::lower::expr::lower_call`/`ufcs_receiver_path` and
            // `strict::check_void_root` also key on unchanged (issue
            // #1561; see that field's doc). This particular lookup is a
            // *negative* check — presence means a real user symbol shadows
            // the `some`/intrinsic pseudo-function name, so this call must
            // not be treated as the built-in coalescing sugar.
            if let [seg] = path.segments.as_slice()
                && !ctx.resolution_by_range.contains_key(&range_key(path.range))
            {
                if seg.text == "some" {
                    let elem = args
                        .first()
                        .and_then(|a| classify_coalesce_operand(a, ctx))
                        .unwrap_or(Ty::Unknown);
                    return Some(Ty::Option(Box::new(elem)));
                }
                if crate::infer::intrinsic_returns_option(&seg.text) {
                    return Some(Ty::Option(Box::new(Ty::Unknown)));
                }
            }
            structs::classify_expr_ty(expr, ctx)
        }
        Expr::Path(p) => {
            if let [seg] = p.segments.as_slice()
                && seg.text == "none"
                && !ctx.resolution_by_range.contains_key(&range_key(p.range))
            {
                return Some(Ty::Option(Box::new(Ty::Unknown)));
            }
            structs::classify_expr_ty(expr, ctx)
        }
        _ => structs::classify_expr_ty(expr, ctx),
    }
}

/// A best-effort own-range for an operand expression, for diagnostic
/// anchoring — mirrors `option_conditions::expr_anchor` exactly (same
/// shapes carry a source range: a path, a call's callee path, and the
/// roots reachable through unary/index/field/infix wrappers). `None` falls
/// back to the enclosing statement/content/choice's own span.
fn expr_anchor(expr: &Expr) -> Option<TextRange> {
    match expr {
        Expr::Path(p) => Some(p.range),
        Expr::Call(path, _) => Some(path.range),
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => expr_anchor(inner),
        Expr::Index(idx) => expr_anchor(&idx.base),
        Expr::FieldAccess(fa) => expr_anchor(&fa.base),
        Expr::Infix(ie) => expr_anchor(&ie.lhs).or_else(|| expr_anchor(&ie.rhs)),
        _ => None,
    }
}

/// The nearest source range a statement carries on its own `Provenance`, if
/// any — `enter_stmt`'s fallback-anchor update. `ChoiceSet` and
/// `LabeledBlock` carry no `ptr` of their own (their children — `Choice`,
/// nested statements — do, picked up by `enter_choice`/their own
/// `enter_stmt`); `ExprStmt`/`EndOfLine` likewise have nothing to offer.
fn stmt_anchor(stmt: &Stmt) -> Option<TextRange> {
    match stmt {
        Stmt::Content(c) => c.ptr.map(|p| p.text_range()),
        Stmt::Divert(d) => d.ptr.map(|p| p.text_range()),
        Stmt::TunnelCall(t) => Some(t.ptr.text_range()),
        Stmt::ThreadStart(t) => Some(t.ptr.text_range()),
        Stmt::TempDecl(t) => Some(t.ptr.text_range()),
        Stmt::Assignment(a) => Some(a.ptr.text_range()),
        Stmt::Return(r) => r.ptr.map(|p| p.text_range()),
        Stmt::Conditional(c) => Some(c.ptr.text_range()),
        Stmt::Sequence(s) => Some(s.ptr.text_range()),
        Stmt::LogicBlock(lb) => Some(lb.ptr.text_range()),
        Stmt::Await(a) => Some(a.ptr.text_range()),
        // Issue #2108: `AttachElement`/`EndElementRun` carry no
        // `Provenance`/`ptr` field of their own — same "nothing to offer"
        // posture as `ExprStmt`/`EndOfLine`.
        Stmt::ChoiceSet(_)
        | Stmt::LabeledBlock(_)
        | Stmt::ExprStmt(_)
        | Stmt::EndOfLine
        | Stmt::AttachElement(_)
        | Stmt::EndElementRun => None,
    }
}

fn range_key(range: TextRange) -> (u32, u32) {
    (range.start().into(), range.end().into())
}

/// This file's own reference resolutions, projected to a range-keyed lookup
/// — mirrors `conversions::resolution_index`/`option_conditions`'s own copy.
fn resolution_index(
    resolutions: &ResolutionMap,
    file: FileId,
) -> BTreeMap<(u32, u32), DefinitionId> {
    resolutions
        .iter()
        .filter(|r| r.file == file)
        .map(|r| (range_key(r.range), r.target))
        .collect()
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test-only assertions; see sibling test modules"
)]
mod tests {
    use super::*;
    use brink_ir::{FileId as HirFileId, SymbolIndex};

    /// Native-lowered `(HirFile, SymbolIndex, ResolutionMap, InferenceResult)`
    /// — `InfixOp::Coalesce` is produced only by `hir::lower_native`
    /// (B1, issue #1460), so unlike every other check module's test harness
    /// in this crate (which parses through `brink_syntax`, the ink/brink-
    /// extension frontend), this one must go through the native frontend.
    fn build_native(src: &str) -> (HirFile, SymbolIndex, ResolutionMap, InferenceResult) {
        let parse = brink_syntax_native::parse(src);
        assert!(
            parse.errors().is_empty(),
            "fixture must parse cleanly: {:?}",
            parse.errors()
        );
        let tree = parse.tree();
        let (hir, manifest, _diag) = brink_ir::hir::lower_native::lower(HirFileId(0), &tree);
        let (index, _diag) = crate::symbol_index(&[(HirFileId(0), &manifest)]);
        let (resolutions, _diag) = crate::resolve(
            HirFileId(0),
            &manifest,
            &index,
            &crate::ImportScope::default(),
        );
        let inference = crate::infer_project(
            &[(HirFileId(0), &hir)],
            &index,
            &resolutions,
            None,
            &BTreeMap::new(),
        );
        (hir, (*index).clone(), (*resolutions).clone(), inference)
    }

    fn check_all(src: &str) -> Vec<Diagnostic> {
        let (hir, index, resolutions, inference) = build_native(src);
        check(&[(HirFileId(0), &hir)], &index, &inference, &resolutions)
    }

    // ── issue #1764: a lambda's statements in a VAR/CONST initializer ────

    /// Coverage for a lambda's statements in a VAR/CONST initializer comes
    /// from `visit::visit_with_decl_initializers` (which reaches the
    /// initializer at all) composed with `walk_expr`'s `Expr::Lambda` arm
    /// (which already descends a lambda's statements) — there is no
    /// separate hand-rolled recursion for this position (issue #2098).
    #[test]
    fn a_bad_chain_in_a_lambda_statement_of_a_var_initializer_is_e066() {
        let diags = check_all("var f = ||: int {\n  let x = 5 or 9;\n  0\n};\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E066);
    }

    /// The same walk answers "does this project coalesce at all?", the gate
    /// on building a `CoalesceTable` at all. Missing the chain there dropped
    /// its *shape* from the table, not just its diagnostic. Before issue
    /// #1774 (RULED 2026-08-01) this table never actually reached
    /// `CoalesceLookup` in LIR for a lambda in a VAR/CONST initializer
    /// specifically — the position was a hard `E083` (see
    /// `hir::visit::walk_expr`'s `Expr::Lambda` arm, which descends a
    /// lambda's statements) — so this pinned only the analyzer-layer
    /// shape. #1774 lifted that gate, so this chain's shape is now
    /// LIR-reachable too: `brink-ir`'s
    /// `coalesce_chain_in_lambda_decl_default_gets_its_real_recorded_shape`
    /// (`tests/lir_lowering/lambda_literal_declaration_default.rs`) is the
    /// sibling pin on that end of the pipeline.
    #[test]
    fn a_chain_in_a_lambda_statement_of_a_var_initializer_trips_the_project_gate() {
        let (hir, _index, _res, _inf) =
            build_native("var f = ||: int {\n  let x = some(1) or 2;\n  0\n};\n");
        assert!(project_has_coalesce(&hir));
    }

    /// …and is recorded in the table with its real collapsed shape.
    #[test]
    fn a_chain_in_a_lambda_statement_of_a_var_initializer_is_recorded_in_the_table() {
        let chain = only_chain("var f = ||: int {\n  let x = some(1) or 2;\n  0\n};\n");
        assert_eq!(chain.steps.len(), 1, "{chain:?}");
        assert_eq!(chain.steps[0].rhs, Ty::Int);
    }

    #[test]
    fn non_option_left_hand_side_is_e066() {
        let diags = check_all("flow main() {\n  {5 or 9}\n  -> END\n}\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E066);
    }

    #[test]
    fn mismatched_fallback_type_is_e066() {
        let diags = check_all("flow main() {\n  {some(1) or \"text\"}\n  -> END\n}\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E066);
    }

    #[test]
    fn collapse_form_with_agreeing_types_is_clean() {
        let diags = check_all("flow main() {\n  {some(1) or 2}\n  -> END\n}\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn two_option_form_with_agreeing_types_is_clean() {
        let diags = check_all("flow main() {\n  {some(1) or none}\n  -> END\n}\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unclassifiable_operand_stays_silently_unchecked() {
        // `x`'s type isn't statically known here (no other use to infer
        // from) — "Unknown never disagrees", same posture every sibling
        // check in this crate takes.
        let diags = check_all("flow main(x) {\n  {x or 9}\n  -> END\n}\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// Companion to the diagnostics-only assertion above: an unpinned
    /// left-hand type is not merely diagnostic-clean, it is recorded as a
    /// real `CoalesceShape::RuntimeCheck` step — the unpinned-`lhs` posture
    /// `Opcode::CoalesceSome`'s own doc claims. Pins the review finding that
    /// this branch was previously unreachable (the operand fell through a
    /// silent `return` instead of ever reaching `step_shape`).
    ///
    /// `!x` (an `Expr::Prefix` wrapping the param, carrying its span) rather
    /// than the bare param path itself: a bare single-segment param/temp
    /// path used *directly* as a coalescing `lhs` gets narrowed to
    /// `Option[…]` by `infer::body`'s own feedback
    /// (`coalesce_lhs_param_narrows_to_option_of_the_rhs_type`), so it
    /// always classifies. `Expr::Prefix` is neither a shape `observe`
    /// narrows (only a bare `Path`) nor one `classify_expr_ty` handles at
    /// all (only Path/Call/Index beyond literals) — genuinely
    /// unclassifiable, and (like every infix node since #1517) keyable
    /// from the operation's own provenance.
    #[test]
    fn unpinned_left_hand_side_records_a_runtime_check_step() {
        // `fn`'s plain `{ }` routes through the code-ground `stmt_block`
        // (unlike `flow`'s, which is content-ground and reads a leading `!`
        // inside `{ }` as a once-only sequence marker, not boolean negation
        // — this fixture must be a `fn` body to get a real `Expr::Prefix`).
        let src = concat!(
            "fn f(x) {\n  return !x or 9;\n}\n",
            "flow main() {\n  -> END\n}\n",
        );
        let chain = only_chain(src);
        assert_eq!(chain.steps.len(), 1, "{chain:?}");
        let step = &chain.steps[0];
        assert_eq!(step.lhs, Ty::Unknown);
        assert_eq!(step.rhs, Ty::Int);
        assert_eq!(step.result, Ty::Unknown);
        assert_eq!(step.shape, CoalesceShape::RuntimeCheck);
    }

    // ─── The recorded side channel (issue #1492) ──────────────────────

    fn table_of(src: &str) -> CoalesceTable {
        let (hir, index, resolutions, inference) = build_native(src);
        resolve(&[(HirFileId(0), &hir)], &index, &inference, &resolutions).0
    }

    /// The single recorded chain in a one-chain fixture.
    fn only_chain(src: &str) -> CoalesceChain {
        let table = table_of(src);
        assert_eq!(table.len(), 1, "expected exactly one chain: {table:?}");
        let (_key, chain) = table.iter().next().expect("one entry");
        chain.clone()
    }

    fn opt(inner: Ty) -> Ty {
        Ty::Option(Box::new(inner))
    }

    #[test]
    fn collapse_form_records_the_collapsed_value_type() {
        let chain = only_chain("flow main() {\n  {some(1) or 2}\n  -> END\n}\n");
        assert_eq!(chain.steps.len(), 1);
        let step = &chain.steps[0];
        assert_eq!(step.lhs, opt(Ty::Int));
        assert_eq!(step.rhs, Ty::Int);
        assert_eq!(step.result, Ty::Int);
        assert_eq!(step.shape, CoalesceShape::Collapse);
    }

    #[test]
    fn two_option_form_records_preserved_optionality() {
        let chain = only_chain("flow main() {\n  {some(1) or none}\n  -> END\n}\n");
        assert_eq!(chain.steps.len(), 1);
        let step = &chain.steps[0];
        assert_eq!(step.shape, CoalesceShape::PreserveOption);
        assert!(
            matches!(step.result, Ty::Option(_)),
            "optionality survives: {:?}",
            step.result
        );
    }

    /// The verdict LIR lowering needs and no syntactic shape-sniff can
    /// reach: the fallback is a *call*, so "is the fallback `Option`-shaped"
    /// is answerable only from the callee's recorded return type.
    #[test]
    fn a_call_fallback_is_typed_from_its_return_type_not_its_syntax() {
        let src = concat!(
            "fn maybe() {\n  return some(7);\n}\n",
            "flow main() {\n  {some(1) or maybe()}\n  -> END\n}\n",
        );
        let chain = only_chain(src);
        assert_eq!(chain.steps.len(), 1);
        assert_eq!(chain.steps[0].rhs, opt(Ty::Int));
        assert_eq!(chain.steps[0].shape, CoalesceShape::PreserveOption);
    }

    /// The chain the whole side channel exists for: an `Option`-returning
    /// call in the middle keeps the inner step optional, and only the final
    /// plain fallback collapses.
    #[test]
    fn a_chain_records_every_step_innermost_first() {
        let src = concat!(
            "fn maybe() {\n  return some(7);\n}\n",
            "flow main() {\n  {some(1) or maybe() or 99}\n  -> END\n}\n",
        );
        let chain = only_chain(src);
        assert_eq!(chain.steps.len(), 2, "{chain:?}");
        assert_eq!(chain.steps[0].shape, CoalesceShape::PreserveOption);
        assert_eq!(chain.steps[0].result, opt(Ty::Int));
        // The inner step's result is the outer step's left-hand type — the
        // fold, not a re-classification of the `Expr::Infix` node (which
        // `classify_expr_ty` cannot type at all).
        assert_eq!(chain.steps[1].lhs, opt(Ty::Int));
        assert_eq!(chain.steps[1].rhs, Ty::Int);
        assert_eq!(chain.steps[1].result, Ty::Int);
        assert_eq!(chain.steps[1].shape, CoalesceShape::Collapse);
    }

    /// A bare `Path` to a declared `Option[int]` temp as the fallback — the
    /// w56 scope finding folded into #1492. Syntax says "an identifier";
    /// the recorded type says `Option[int]`, so optionality is preserved.
    #[test]
    fn an_option_typed_path_fallback_preserves_optionality() {
        let src = concat!(
            "fn pick() {\n",
            "  let fallback = some(3);\n",
            "  return some(1) or fallback;\n",
            "}\n",
            "flow main() {\n  -> END\n}\n",
        );
        let chain = only_chain(src);
        assert_eq!(chain.steps.len(), 1, "{chain:?}");
        assert_eq!(chain.steps[0].rhs, opt(Ty::Int));
        assert_eq!(chain.steps[0].shape, CoalesceShape::PreserveOption);
    }

    /// The widening the fold buys: before #1492 only a chain's innermost
    /// step was judged (`classify_expr_ty` returns `None` for an
    /// `Expr::Infix` left-hand operand), so this compiled silently.
    #[test]
    fn a_mismatch_at_a_later_chain_step_is_now_e066() {
        let diags = check_all("flow main() {\n  {some(1) or none or \"text\"}\n  -> END\n}\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E066);
    }

    #[test]
    fn an_ill_typed_chain_records_no_verdict() {
        let table = table_of("flow main() {\n  {some(1) or \"text\"}\n  -> END\n}\n");
        assert!(table.is_empty(), "{table:?}");
    }

    #[test]
    fn an_all_literal_chain_is_keyable_but_still_ill_typed() {
        // Before #1517 this chain could not be keyed at all — neither
        // operand carried a range and the infix node had none of its own,
        // so `expr_span` yielded `None`. It is keyable now (the operation's
        // own provenance), and records nothing purely because it is `E066`.
        let src = "flow main() {\n  {5 or 9}\n  -> END\n}\n";
        let (hir, ..) = build_native(src);
        let root = first_coalesce_root(&hir).expect("one chain");
        assert!(expr_span(root).is_some(), "the operation is keyable now");
        assert!(table_of(src).is_empty(), "but it is ill-typed");
    }

    /// The #1517 refactor's payoff, at the producer: a chain root's key is
    /// its **own** provenance range, so it covers the trailing literal that
    /// used to contribute nothing, and the chain's own left spine derives a
    /// *different* key that the table simply misses. Before #1517 the two
    /// were the same key, which is why this pass had to poison any key two
    /// roots could share.
    #[test]
    fn a_chain_root_and_its_left_spine_derive_different_keys() {
        let src = concat!(
            "fn maybe() {\n  return some(7);\n}\n",
            "flow main() {\n  {some(1) or maybe() or 99}\n  -> END\n}\n",
        );
        let table = table_of(src);
        assert_eq!(table.len(), 1, "{table:?}");
        let (key, _) = table.iter().next().expect("one entry");
        let start = usize::try_from(key.range.0).unwrap();
        let end = usize::try_from(key.range.1).unwrap();
        assert_eq!(&src[start..end], "some(1) or maybe() or 99");

        // Derive the spine's key from the HIR itself, not from a fabricated
        // `src.find(...)` range: the stamped range includes trailing
        // whitespace trivia before the next operator (see the #1517 comment
        // in `hir::spans`), so a hand-picked substring range would not be
        // the spine's *real* key and would trivially miss the table for the
        // wrong reason.
        let (hir, ..) = build_native(src);
        let root_expr = first_coalesce_root(&hir).expect("one chain");
        let Expr::Infix(root) = root_expr else {
            panic!("expected a left-associative chain, got {root_expr:?}");
        };
        let spine_range = expr_span(&root.lhs).expect("the left spine is an infix too");
        assert_ne!(
            spine_range,
            TextRange::new(key.range.0.into(), key.range.1.into())
        );
        assert!(
            table.at(HirFileId(0), spine_range).is_none(),
            "a spine node must miss, never inherit the root's verdict: {table:?}"
        );
    }

    /// The first coalescing chain root in a file's knot bodies.
    fn first_coalesce_root(hir: &HirFile) -> Option<&Expr> {
        for knot in &hir.knots {
            for stmt in &knot.body.stmts {
                if let Stmt::Content(c) = stmt {
                    for part in &c.parts {
                        if let brink_ir::ContentPart::Interpolation(e) = part
                            && coalesce_operands(e).is_some()
                        {
                            return Some(e);
                        }
                    }
                }
            }
        }
        None
    }

    #[test]
    fn each_chain_is_recorded_once_at_its_root() {
        let src = concat!(
            "fn maybe() {\n  return some(7);\n}\n",
            "flow main() {\n  {some(1) or maybe() or 99}\n  {some(2) or 3}\n  -> END\n}\n",
        );
        let table = table_of(src);
        assert_eq!(table.len(), 2, "one entry per chain root: {table:?}");
    }

    /// Two textually identical chains in the same file stay separately
    /// addressable: the key is each root's own source range, not its text.
    #[test]
    fn sibling_chains_keep_distinct_keys() {
        let src = "flow main() {\n  {some(1) or 2}\n  {some(1) or 2}\n  -> END\n}\n";
        let table = table_of(src);
        assert_eq!(table.len(), 2, "{table:?}");
    }

    #[test]
    fn a_var_initializer_chain_is_recorded_too() {
        let src = "var v = some(1) or 2\nflow main() {\n  -> END\n}\n";
        let chain = only_chain(src);
        assert_eq!(chain.steps.len(), 1);
        assert_eq!(chain.steps[0].shape, CoalesceShape::Collapse);
    }

    /// Issue #2098: a bare-literal operand (`5 or 9`) carries no `Provenance`
    /// of its own for [`stmt_anchor`]/`enter_content`/`enter_choice` to pick
    /// up (see `CoalesceVisitor::fallback`'s own doc) — inside a VAR
    /// initializer specifically, that means the diagnostic's anchor can only
    /// come from `CoalesceVisitor::enter_var_decl`'s reset. Before the
    /// migration this was the hand-rolled `check_expr` recursion's explicit
    /// `fallback: var.ptr.text_range()` parameter; this pins the same
    /// resulting range through the shared `HirVisitor` entry point instead.
    #[test]
    fn a_bare_literal_chain_in_a_var_initializer_anchors_on_the_declaration() {
        let src = "var v = 5 or 9\nflow main() {\n  -> END\n}\n";
        let (hir, index, resolutions, inference) = build_native(src);
        let diags = check(&[(HirFileId(0), &hir)], &index, &inference, &resolutions);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E066);
        assert_eq!(
            diags[0].range,
            hir.variables[0].ptr.text_range(),
            "a bare-literal chain's fallback anchor must be the VAR's own \
             range when nothing narrower is available"
        );
    }
}
