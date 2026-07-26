//! B1 `or`-coalescing static mismatch check (`docs/stdlib-spec.md` §1.6a,
//! issue #1460; review finding on PR #1469/#1460).
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
//! Strict-mode-only: under `types = gradual` (including native's
//! un-overridden default — B0.10's dialect-keyed strict-only wiring has not
//! landed, `strict.rs`'s own `native_strict_only_error` doc) this module is
//! never invoked, and the runtime `TypeError` fault
//! (`brink_runtime::value_ops::coalesce`) is the sole backstop — see that
//! function's doc for the fault's actual (narrower-than-previously-claimed)
//! coverage: it only catches a non-Option left-hand side, not a mismatched
//! fallback type.
//!
//! Reuses `infer::ty::coalesce` itself — the identical typing rule
//! `infer::body::InferPass::infer_infix`'s `InfixOp::Coalesce` arm calls —
//! rather than re-deriving a parallel mismatch rule, so the two can never
//! drift apart.

use std::collections::BTreeMap;

use brink_format::DefinitionId;
use brink_ir::hir::visit::{self, ContentContext, HirVisitor};
use brink_ir::{
    Choice, Content, Diagnostic, DiagnosticCode, Expr, FileId, HirFile, InfixOp, Knot,
    ResolutionMap, Stitch, Stmt, SymbolIndex, SymbolKind,
};
use rowan::TextRange;

use crate::annotations;
use crate::infer::{self, CoalesceError, InferenceResult, InferredSig, Ty};
use crate::structs::{self, MistypeCtx};

/// Strict-mode-only `or`-coalescing mismatch checks over every
/// `InfixOp::Coalesce` expression in the project. Callers only reach this
/// once `strict::config_error` has confirmed `types = strict` + `dialect =
/// brink` (mirrors `conversions::check`'s own entry condition — same
/// wiring point, `strict::check`).
#[must_use]
pub fn check(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
    resolutions: &ResolutionMap,
) -> Vec<Diagnostic> {
    let globals = crate::infer::collect_globals(files, index, None);
    let mut out = Vec::new();
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
            diagnostics: &mut out,
        };
        visit::visit(hir, &mut v);
        // File-level VAR/CONST initializers aren't part of `visit::visit`'s
        // block-tree walk — same gap `conversions::check`'s own doc
        // explains, same fix (a small hand recursion over just these).
        let ctx = MistypeCtx {
            index,
            globals: &globals,
            signatures: &inference.signatures,
            resolution_by_range: &resolution_by_range,
            locals: None,
        };
        for var in &hir.variables {
            check_expr(&var.value, var.ptr.text_range(), file, &ctx, &mut out);
        }
        for c in &hir.constants {
            check_expr(&c.value, c.ptr.text_range(), file, &ctx, &mut out);
        }
    }
    out
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
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl<'a> CoalesceVisitor<'a> {
    fn current_locals(&self) -> Option<&'a BTreeMap<String, Ty>> {
        self.stitch_locals.or(self.knot_locals)
    }

    fn ctx(&self) -> MistypeCtx<'a> {
        MistypeCtx {
            index: self.index,
            globals: self.globals,
            signatures: self.signatures,
            resolution_by_range: self.resolution_by_range,
            locals: self.current_locals(),
        }
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
        let ctx = self.ctx();
        check_coalesce(expr, self.fallback, self.file, &ctx, self.diagnostics);
    }
}

/// Recurse into `expr` looking for coalescing expressions — used only for
/// the file-level VAR/CONST initializers `visit::visit` doesn't cover;
/// every other position is already reached through the `HirVisitor` walk
/// above. Mirrors `conversions::check_expr`'s own shape.
fn check_expr(
    expr: &Expr,
    fallback: TextRange,
    file: FileId,
    ctx: &MistypeCtx<'_>,
    out: &mut Vec<Diagnostic>,
) {
    check_coalesce(expr, fallback, file, ctx, out);
    for child in expr_children(expr) {
        check_expr(child, fallback, file, ctx, out);
    }
}

/// Direct child expressions of `expr` — mirrors `conversions::expr_children`
/// (same rationale: needed only because `check_expr` runs outside the
/// `HirVisitor` walk).
fn expr_children(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => vec![inner],
        Expr::FieldAccess(fa) => vec![&fa.base],
        Expr::Infix(lhs, _, rhs) => vec![lhs, rhs],
        Expr::Call(_, args) => args.iter().collect(),
        Expr::ArrayLiteral(a) => a.elements.iter().collect(),
        Expr::MapLiteral(m) => m.entries.iter().flat_map(|(k, v)| [k, v]).collect(),
        Expr::Index(idx) => vec![&idx.base, &idx.index],
        Expr::StructLiteral(sl) => sl.fields.iter().map(|(_, v)| v).collect(),
        Expr::FnLiteral(fl) => fl.args.iter().collect(),
        Expr::RefArg(ra) => vec![&ra.operand],
        Expr::Range(r) => vec![&r.start, &r.end],
        Expr::String(s) => s
            .parts
            .iter()
            .filter_map(|p| match p {
                brink_ir::StringPart::Interpolation(e) => Some(e.as_ref()),
                brink_ir::StringPart::Literal(_) => None,
            })
            .collect(),
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Path(_)
        | Expr::DivertTarget(_)
        | Expr::ListLiteral(_) => Vec::new(),
    }
}

/// If `expr` is an `InfixOp::Coalesce` node whose two operands both
/// classify to a statically-known [`Ty`], re-run [`infer::coalesce`] (the
/// same rule `infer::body`'s own arm calls) and push a diagnostic when it
/// disagrees. Anything else — an unclassifiable operand on either side —
/// stays silently unchecked, the same "Unknown never disagrees" posture
/// every sibling module in this crate takes; the runtime fault remains the
/// backstop.
fn check_coalesce(
    expr: &Expr,
    fallback: TextRange,
    file: FileId,
    ctx: &MistypeCtx<'_>,
    out: &mut Vec<Diagnostic>,
) {
    let Expr::Infix(lhs, InfixOp::Coalesce, rhs) = expr else {
        return;
    };
    let Some(l) = classify_coalesce_operand(lhs, ctx) else {
        return;
    };
    let Some(r) = classify_coalesce_operand(rhs, ctx) else {
        return;
    };
    if let Err(err) = infer::coalesce(&l, &r) {
        let range = expr_anchor(lhs)
            .or_else(|| expr_anchor(rhs))
            .unwrap_or(fallback);
        out.push(Diagnostic {
            file,
            range,
            message: coalesce_error_message(&err),
            code: DiagnosticCode::E066,
        });
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
        Expr::Infix(lhs, _, rhs) => expr_anchor(lhs).or_else(|| expr_anchor(rhs)),
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
        Stmt::ChoiceSet(_) | Stmt::LabeledBlock(_) | Stmt::ExprStmt(_) | Stmt::EndOfLine => None,
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
}
