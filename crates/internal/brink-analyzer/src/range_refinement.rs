//! NS-A5: the inhabited-range refinement under `types = strict` (E117;
//! issue #1111, `docs/stdlib-spec.md` §7, F7/F8 ruled 2026-07-19).
//!
//! `rand::int` — spelled `int(r)` over a range — is **total by type**: its
//! parameter is the language's first value refinement, the inhabited range
//! (`NonEmptyRange`, the S2 spelling). This module is the strict-mode
//! evidence check, and deliberately THE TEMPLATE for every future value
//! refinement (F8's general rule):
//!
//! - **Strict-only.** Under `types = gradual` this module is never invoked
//!   — the refinement is inert and the runtime fault
//!   (`RuntimeError::EmptyRangeDraw`) is the residual. This is exactly the
//!   `int()`/E078 precedent (`conversions`), whose structure this module
//!   mirrors seam for seam.
//! - **Evidence is minted, never asserted.** A range literal in argument
//!   position with statically-foldable bounds (integer literals, unary
//!   minus, CONST refs — "CONST refs fold" per the F7 ruling) coerces in
//!   free when provably inhabited, and is E117 when provably empty
//!   (`int(0..0)` — the "statically-empty literal" compile error). A
//!   non-literal argument must carry `Ty::Range { non_empty: true }`
//!   evidence from the inference substrate — minted by a provably-inhabited
//!   literal initializer or by `non_empty(r)`'s `some` payload
//!   (parse-don't-validate: the Option tax sits once at the boundary).
//! - **Unknown stays unchecked.** An `Unknown`/`Conflicted`-typed argument
//!   is left to the escape checks (E065/E066) and the runtime backstop —
//!   the same "Unknown never disagrees" posture `conversions`/`structs`
//!   take. A non-range-typed argument is the *conversion* leg of `int(x)`
//!   and belongs to E078's domain check, not this one.
//!
//! Shadowing: an unresolved call to `int` is the builtin; a resolved one
//! (an author-defined `int` knot) is an ordinary call, never checked here.

use std::collections::BTreeMap;

use brink_format::DefinitionId;
use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{
    Diagnostic, DiagnosticCode, Expr, FileId, HirFile, Knot, PrefixOp, ResolutionMap, Stitch,
    SymbolIndex, SymbolKind,
};

use crate::annotations;
use crate::infer::{InferenceResult, InferredSig, Ty};
use crate::structs::{self, MistypeCtx};
use rowan::TextRange;

/// Strict-mode-only range-refinement checks over every `int(r)` call in the
/// project. Callers only reach this once `strict::config_error` has
/// confirmed `types = strict` + `dialect = brink` (mirrors
/// `conversions::check`'s entry condition — same wiring point,
/// `strict::check`).
#[must_use]
pub fn check(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
    resolutions: &ResolutionMap,
) -> Vec<Diagnostic> {
    let globals = crate::infer::collect_globals(files, index, None);
    // The CONST fold table (the "CONST refs fold" leg of the F7 evidence
    // rule): every project CONST whose initializer folds to an int, keyed
    // by `(declaring file, name)` — the pair a resolved `DefinitionId`'s
    // `SymbolInfo` gives back. Built once; deterministic (BTreeMap).
    let mut const_ints: BTreeMap<(FileId, String), i64> = BTreeMap::new();
    for &(file, hir) in files {
        for c in &hir.constants {
            if let Some(v) = fold_literal_bound(&c.value) {
                const_ints.insert((file, c.name.text.clone()), v);
            }
        }
    }
    let mut out = Vec::new();
    for &(file, hir) in files {
        let resolution_by_range = resolution_index(resolutions, file);
        let mut v = RefinementVisitor {
            file,
            index,
            globals: &globals,
            signatures: &inference.signatures,
            bodies: &inference.bodies,
            resolution_by_range: &resolution_by_range,
            const_ints: &const_ints,
            current_knot_name: None,
            knot_locals: None,
            stitch_locals: None,
            diagnostics: &mut out,
        };
        visit::visit(hir, &mut v);
        // File-level VAR/CONST initializers aren't part of `visit::visit`'s
        // walk — same pattern as `conversions::check`.
        let ctx = MistypeCtx {
            index,
            globals: &globals,
            signatures: &inference.signatures,
            resolution_by_range: &resolution_by_range,
            locals: None,
        };
        let fold = FoldCtx {
            index,
            resolution_by_range: &resolution_by_range,
            const_ints: &const_ints,
        };
        for var in &hir.variables {
            check_expr(&var.value, file, &ctx, &fold, &mut out);
        }
        for c in &hir.constants {
            check_expr(&c.value, file, &ctx, &fold, &mut out);
        }
    }
    out
}

/// Everything the bound folder needs to resolve a CONST reference.
struct FoldCtx<'a> {
    index: &'a SymbolIndex,
    resolution_by_range: &'a BTreeMap<(u32, u32), DefinitionId>,
    const_ints: &'a BTreeMap<(FileId, String), i64>,
}

struct RefinementVisitor<'a> {
    file: FileId,
    index: &'a SymbolIndex,
    globals: &'a BTreeMap<DefinitionId, Ty>,
    signatures: &'a BTreeMap<DefinitionId, InferredSig>,
    bodies: &'a BTreeMap<DefinitionId, crate::infer::BodyTypes>,
    resolution_by_range: &'a BTreeMap<(u32, u32), DefinitionId>,
    const_ints: &'a BTreeMap<(FileId, String), i64>,
    current_knot_name: Option<String>,
    knot_locals: Option<&'a BTreeMap<String, Ty>>,
    stitch_locals: Option<&'a BTreeMap<String, Ty>>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl<'a> RefinementVisitor<'a> {
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

    fn fold_ctx(&self) -> FoldCtx<'a> {
        FoldCtx {
            index: self.index,
            resolution_by_range: self.resolution_by_range,
            const_ints: self.const_ints,
        }
    }

    fn knot_def_id(&self, knot: &Knot) -> Option<DefinitionId> {
        let kind = match knot.ptr {
            brink_ir::ContainerPtr::Knot(_) => SymbolKind::Knot,
            brink_ir::ContainerPtr::Stitch(_) => SymbolKind::Stitch,
        };
        annotations::def_id_for(self.index, self.file, kind, &knot.name.text)
    }
}

impl HirVisitor for RefinementVisitor<'_> {
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

    fn enter_expr(&mut self, expr: &Expr) {
        let ctx = self.ctx();
        let fold = self.fold_ctx();
        check_call(expr, self.file, &ctx, &fold, self.diagnostics);
    }
}

/// Recurse into `expr` looking for `int(r)` calls — only for the file-level
/// VAR/CONST initializers `visit::visit` doesn't cover (mirrors
/// `conversions::check_expr`).
fn check_expr(
    expr: &Expr,
    file: FileId,
    ctx: &MistypeCtx<'_>,
    fold: &FoldCtx<'_>,
    out: &mut Vec<Diagnostic>,
) {
    check_call(expr, file, ctx, fold, out);
    for child in expr_children(expr) {
        check_expr(child, file, ctx, fold, out);
    }
}

/// Direct child expressions — mirrors `conversions::expr_children` (needed
/// only because `check_expr` runs outside the `HirVisitor` walk).
fn expr_children(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => vec![inner],
        Expr::FieldAccess(fa) => vec![&fa.base],
        Expr::Infix(lhs, _, rhs) => vec![lhs, rhs],
        Expr::Call(_, args) | Expr::FnLiteral(brink_ir::FnLiteral { args, .. }) => {
            args.iter().collect()
        }
        Expr::ArrayLiteral(a) => a.elements.iter().collect(),
        Expr::MapLiteral(m) => m.entries.iter().flat_map(|(k, v)| [k, v]).collect(),
        Expr::StructLiteral(sl) => sl.fields.iter().map(|(_, v)| v).collect(),
        Expr::Index(idx) => vec![&idx.base, &idx.index],
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

/// If `expr` is an unresolved (builtin) call to `int` whose single argument
/// is range-shaped, run the refinement check (module doc). Everything else
/// — resolved (shadowed) calls, other names, wrong arity (E031's job), a
/// conversion-leg argument (E078's domain), an `Unknown`-typed argument —
/// is silently clean here.
fn check_call(
    expr: &Expr,
    file: FileId,
    ctx: &MistypeCtx<'_>,
    fold: &FoldCtx<'_>,
    out: &mut Vec<Diagnostic>,
) {
    let Expr::Call(path, args) = expr else {
        return;
    };
    let [seg] = path.segments.as_slice() else {
        return;
    };
    if seg.text != "int" {
        return;
    }
    if ctx.resolution_by_range.contains_key(&range_key(path.range)) {
        return; // author-shadowed — an ordinary call
    }
    let [arg] = args.as_slice() else {
        return; // wrong arity — E031's job
    };

    // Leg (a): a range literal directly in argument position — fold the
    // bounds (literals + CONST refs).
    if let Expr::Range(r) = arg {
        match (fold_bound(&r.start, fold), fold_bound(&r.end, fold)) {
            (Some(start), Some(end)) => {
                let inhabited = if r.inclusive {
                    start <= end
                } else {
                    start < end
                };
                if !inhabited {
                    out.push(diag(
                        file,
                        path.range,
                        format!(
                            "{}: this range is provably empty — `int` draws one element, \
                             and there is nothing to draw",
                            DiagnosticCode::E117.title(),
                        ),
                    ));
                }
            }
            _ => {
                // Computed bounds written literally in position:
                // `int(a..b)` — no evidence can be minted statically.
                out.push(diag(
                    file,
                    path.range,
                    format!(
                        "{}: these bounds are not statically provable — validate with \
                         `non_empty(a..b)` and draw from its `some` payload",
                        DiagnosticCode::E117.title(),
                    ),
                ));
            }
        }
        return;
    }

    // Leg (b): a non-literal argument — the evidence must already be on
    // its inferred type.
    // Evidence carried (`non_empty: true`) is free; a non-range type is
    // the conversion leg (E078's domain, not ours); Unknown/Conflicted/
    // unclassifiable is left to the escape checks + the runtime fault.
    // Only the evidence-free range errs.
    if let Some(Ty::Range { non_empty: false }) = structs::classify_expr_ty(arg, ctx) {
        out.push(diag(
            file,
            path.range,
            format!(
                "{}: this range is possibly empty — validate with `non_empty(r)` \
                 (the evidence is minted once; every later draw is free)",
                DiagnosticCode::E117.title(),
            ),
        ));
    }
}

/// `TextRange` has no `Ord`; the `(start, end)` pair is the map key — the
/// same private copy every sibling checker keeps (`conversions`, `structs`,
/// `strict`…).
fn range_key(range: TextRange) -> (u32, u32) {
    (range.start().into(), range.end().into())
}

/// One file's resolutions re-keyed by [`range_key`] — the same private copy
/// `conversions::resolution_index` keeps.
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

fn diag(file: FileId, range: TextRange, message: String) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message,
        code: DiagnosticCode::E117,
    }
}

/// Fold a range bound to its compile-time int value: an integer literal, a
/// unary-negated foldable bound, or a resolved CONST whose own initializer
/// folds ("CONST refs fold" — the F7 evidence rule). `None` for anything
/// else.
fn fold_bound(expr: &Expr, fold: &FoldCtx<'_>) -> Option<i64> {
    match expr {
        Expr::Path(p) => {
            let def = fold.resolution_by_range.get(&range_key(p.range))?;
            let info = fold.index.symbols.get(def)?;
            if info.kind != SymbolKind::Constant {
                return None;
            }
            fold.const_ints
                .get(&(info.file, info.name.clone()))
                .copied()
        }
        _ => fold_literal_bound(expr),
    }
}

/// The literal-only fold shared with the CONST-table builder: an integer
/// literal or a unary-negated one. `i64` so `-2147483648` folds cleanly.
fn fold_literal_bound(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Int(n) => Some(i64::from(*n)),
        Expr::Prefix(PrefixOp::Negate, inner) => fold_literal_bound(inner).map(|n| -n),
        _ => None,
    }
}
