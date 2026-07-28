//! TM-3 completion: `int(x)`/`float(x)` strict-mode domain compile error
//! (`docs/typed-mode-spec.md` §4, maintainer ruling 2026-07-13, issue #659,
//! ruling 2: "Divert/list/collection inputs: compile error under `types =
//! strict`, runtime fault under gradual").
//!
//! Strict-mode-only, mirroring `structs::check`'s own gating (wired into
//! `strict::check` alongside E065-E071): under `types = gradual` this module
//! is never invoked at all, deferring entirely to the runtime fault the
//! `int`/`float` VM ops already implement
//! (`RuntimeError::InvalidConversionDomain`).
//!
//! `string(x)` accepts every type (ruling 2: "everything, display form") and
//! is never checked here — only `int`/`float` have a restricted domain.
//!
//! Scoped to **statically classifiable** arguments — a divert-target
//! expression (`-> knot`), a LIST literal, or a `#[...]`/`#{...}`/`Name#{...}`
//! collection/struct literal passed *directly* as the argument, **plus**
//! (issue #983, mirroring `structs::check`'s own `E071` extension for issue
//! #670) a variable-, call-, or index-valued argument whose type is
//! statically known through the project's whole-project inference
//! substrate: a `Path` resolving to a param/temp reads that def's finalized
//! `BodyTypes::locals`; a `Path` resolving to a global `VAR`/`CONST` reads
//! its declaration-derived type; a `Call` reads the resolved callee's
//! `InferredSig::return_ty`; an `Index` recurses into its base's classified
//! type and takes the array-element/map-value type. Reuses
//! `structs::classify_expr_ty`/`structs::MistypeCtx` verbatim — the same
//! firewall `infer::body` itself reads through — rather than re-deriving an
//! independent copy. Whenever that resolution lands on `Unknown` or
//! `Conflicted` (unresolved, unannotated, or genuinely contradictory), the
//! argument stays silently unchecked, preserving gradual-mode conservatism
//! exactly as before. The runtime fault (`InvalidConversionDomain`) is
//! always the backstop that still catches every case at execution time even
//! under `types = strict` — this pass is a compile-time convenience for the
//! statically-provable cases, not the sole enforcement.
//!
//! Shadowing: an unresolved call to `int`/`float` is the builtin (per the
//! stdlib slice-1 ruling, T1b-surface-spec §5); a call that *did* resolve
//! (an author-defined `int`/`float` knot) is an ordinary function call and
//! is never flagged here.

use std::collections::BTreeMap;

use brink_format::DefinitionId;
use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{
    Diagnostic, DiagnosticCode, Expr, FileId, HirFile, Knot, ResolutionMap, Stitch, SymbolIndex,
    SymbolKind,
};
use rowan::TextRange;

use crate::annotations;
use crate::infer::{InferenceResult, InferredSig, Ty};
use crate::structs::{self, MistypeCtx};

/// The two domain-restricted conversion intrinsics — `string()` accepts
/// every type (ruling 2) and is deliberately excluded.
fn domain_checked_name(name: &str) -> bool {
    matches!(name, "int" | "float")
}

/// Strict-mode-only conversion-domain checks over every `int(x)`/`float(x)`
/// call in the project. Callers only reach this once `strict::config_error`
/// has confirmed `types = strict` + `dialect = brink` (mirrors
/// `structs::check`'s own entry condition).
///
/// `index`/`inference` (issue #983): the same whole-project `SymbolIndex`/
/// `InferenceResult` `strict::check` already computes for its own
/// escape/mismatch checks and threads into `structs::check` — this is what
/// lets the domain check classify a variable/call/index-valued argument
/// instead of only literal-shaped ones (see the module doc).
#[must_use]
pub fn check(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
    resolutions: &ResolutionMap,
) -> Vec<Diagnostic> {
    // No manifest access at this call site, mirroring `structs::check`'s own
    // note — a global's `Handle<K>` annotation resolving isn't in this
    // check's scope any more than a struct field's is.
    let globals = crate::infer::collect_globals(files, index, None);
    let mut out = Vec::new();
    for &(file, hir) in files {
        let resolution_by_range = resolution_index(resolutions, file);
        let mut v = ConversionVisitor {
            file,
            index,
            globals: &globals,
            signatures: &inference.signatures,
            bodies: &inference.bodies,
            resolution_by_range: &resolution_by_range,
            current_knot_name: None,
            knot_locals: None,
            stitch_locals: None,
            diagnostics: &mut out,
        };
        visit::visit(hir, &mut v);
        // File-level declaration initializers aren't part of `visit::visit`'s
        // block-tree walk (see its module doc) — same pattern
        // `structs::check`/`dialect_gate`/`annotations` use for VAR/CONST. No
        // enclosing def here, so `locals` is `None` — only a reference to a
        // global VAR/CONST is classifiable at file scope, matching
        // `infer::body`'s own firewall (a body never sees another def's
        // locals either).
        let ctx = MistypeCtx {
            index,
            globals: &globals,
            signatures: &inference.signatures,
            resolution_by_range: &resolution_by_range,
            locals: None,
        };
        for var in &hir.variables {
            check_expr(&var.value, file, &ctx, &mut out);
        }
        for c in &hir.constants {
            check_expr(&c.value, file, &ctx, &mut out);
        }
    }
    out
}

struct ConversionVisitor<'a> {
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
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl<'a> ConversionVisitor<'a> {
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

impl HirVisitor for ConversionVisitor<'_> {
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
        // `visit::visit` only ever calls `enter_stitch` nested inside an
        // `enter_knot`/`exit_knot` pair, so `current_knot_name` is always
        // set here.
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
        check_call(expr, self.file, &ctx, self.diagnostics);
    }
}

/// Recurse into `expr` looking for `int`/`float` calls — used only for the
/// file-level VAR/CONST initializers `visit::visit` doesn't cover; every
/// other position is already reached through the `HirVisitor` walk above.
/// Mirrors `structs::check_expr`'s own shape (a small hand recursion, not
/// worth sharing across the two modules for one call site each).
fn check_expr(expr: &Expr, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    check_call(expr, file, ctx, out);
    for child in expr_children(expr) {
        check_expr(child, file, ctx, out);
    }
}

/// Direct child expressions of `expr` — mirrors `structs::expr_children`
/// (same rationale: needed only because `check_expr` runs outside the
/// `HirVisitor` walk).
fn expr_children(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => vec![inner],
        Expr::FieldAccess(fa) => vec![&fa.base],
        Expr::Infix(ie) => vec![&ie.lhs, &ie.rhs],
        Expr::Call(_, args) => args.iter().collect(),
        Expr::ArrayLiteral(a) => a.elements.iter().collect(),
        Expr::MapLiteral(m) => m.entries.iter().flat_map(|(k, v)| [k, v]).collect(),
        Expr::Index(idx) => vec![&idx.base, &idx.index],
        Expr::StructLiteral(sl) => sl.fields.iter().map(|(_, v)| v).collect(),
        // T1c `#fn(target, args…)`: only the bound arguments are child
        // expressions — the target is a static `Path` field, same as `Call`.
        Expr::FnLiteral(fl) => fl.args.iter().collect(),
        // T1e `ref lvalue-path`: only the operand is a child expression.
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

/// If `expr` is an unresolved (builtin, not author-shadowed) call to
/// `int`/`float` whose single argument is statically out-of-domain — either
/// a literal-shaped argument (see [`literal_out_of_domain_kind`]) or, since
/// issue #983, a variable/call/index-valued argument whose inference-
/// substrate-resolved type (via `ctx`) falls outside the permitted
/// int/float/bool/string domain — push `E078`. Anything else — resolved
/// calls (shadowed), other names, wrong arity (flagged separately as
/// `E031`), or an argument whose type isn't statically obvious (`Unknown`/
/// `Conflicted`) — is silently clean.
fn check_call(expr: &Expr, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    let Expr::Call(path, args) = expr else {
        return;
    };
    let [seg] = path.segments.as_slice() else {
        return;
    };
    if !domain_checked_name(&seg.text) {
        return;
    }
    if ctx.resolution_by_range.contains_key(&range_key(path.range)) {
        return; // resolved to an author-defined symbol — shadows the builtin
    }
    let [arg] = args.as_slice() else {
        return; // wrong arity — E031's job, not this pass's
    };
    let Some(kind) = classify_out_of_domain(arg, ctx) else {
        return;
    };
    out.push(Diagnostic {
        file,
        range: path.range,
        message: format!(
            "{}: `{}(x)` cannot accept a {kind} value under `types = strict` — permitted \
             domain: int, float, string, bool (docs/typed-mode-spec.md §4)",
            DiagnosticCode::E078.title(),
            seg.text,
        ),
        code: DiagnosticCode::E078,
    });
}

/// Classify an argument expression as statically out-of-domain for
/// `int`/`float`: [`literal_out_of_domain_kind`]'s direct-literal
/// classification first, falling back (issue #983) to the non-literal forms
/// #670's `structs::classify_expr_ty` already resolves through the project's
/// inference substrate — a `Path` (variable), a `Call` (function), or an
/// `Index` expression. `None` — "in-domain or not classifiable" — whenever
/// the resolved type is itself int/float/bool/string, `Unknown`, or
/// `Conflicted`, or the expression shape isn't handled at all: the same
/// "Unknown never disagrees" posture `structs::check`'s `E071` takes.
fn classify_out_of_domain(expr: &Expr, ctx: &MistypeCtx<'_>) -> Option<&'static str> {
    if let Some(kind) = literal_out_of_domain_kind(expr) {
        return Some(kind);
    }
    let ty = structs::classify_expr_ty(expr, ctx)?;
    domain_kind_for_ty(&ty)
}

/// Classify an argument expression as statically out-of-domain for
/// `int`/`float` from its own literal shape alone, or `None` if it's
/// in-domain or not classifiable this way (a variable/call/index — handled
/// by [`classify_out_of_domain`]'s inference-substrate fallback instead).
fn literal_out_of_domain_kind(expr: &Expr) -> Option<&'static str> {
    match expr {
        Expr::DivertTarget(_) => Some("divert"),
        Expr::ListLiteral(_) => Some("list"),
        Expr::ArrayLiteral(_) => Some("array"),
        Expr::MapLiteral(_) => Some("map"),
        Expr::StructLiteral(_) => Some("struct"),
        _ => None,
    }
}

/// Map an inference-substrate-resolved [`Ty`] to its `E078` "kind" word,
/// naming exactly the same five categories [`literal_out_of_domain_kind`]
/// does (divert/list/array/map/struct) — the permitted domain
/// (int/float/bool/string) and anything not statically resolved
/// (`Unknown`/`Conflicted`) fall through to `None`. `Ty::Fn`/`Ty::Handle`
/// also fall through here: neither `structs::literal_ty` nor this pass's own
/// direct-literal classification treats those as classifiable field/argument
/// shapes either (no manifest is threaded into this call site — see
/// `check`'s own doc — so `Ty::Handle` can't in practice arise from
/// `collect_globals(..., None)` here anyway), so extending the domain
/// vocabulary to cover them is left out of this issue's scope rather than
/// invented ad hoc.
fn domain_kind_for_ty(ty: &Ty) -> Option<&'static str> {
    match ty {
        Ty::Divert => Some("divert"),
        Ty::List(_) => Some("list"),
        Ty::Array(_) => Some("array"),
        Ty::Map(_, _) => Some("map"),
        Ty::Struct(_) => Some("struct"),
        _ => None,
    }
}

fn range_key(range: TextRange) -> (u32, u32) {
    (range.start().into(), range.end().into())
}

/// This file's own reference resolutions, projected to a range-keyed lookup
/// — mirrors `strict::resolution_index` (same rationale: a `Path`'s range is
/// only unique within its own file).
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
    use brink_ir::hir::lower;
    use brink_ir::{FileId, SymbolIndex};

    /// Computes real resolutions and a whole-project [`InferenceResult`] —
    /// needed by every test, since [`check`] now always consults that
    /// substrate for its variable/call/index-valued argument classification
    /// (issue #983). Mirrors `structs::tests::build_with_inference` exactly.
    fn build_with_inference(src: &str) -> (HirFile, SymbolIndex, ResolutionMap, InferenceResult) {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _diag) = lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) =
            crate::resolve(FileId(0), &manifest, &index, &crate::ImportScope::default());
        let inference = crate::infer_project(
            &[(FileId(0), &hir)],
            &index,
            &resolutions,
            None,
            &BTreeMap::new(),
        );
        (hir, (*index).clone(), (*resolutions).clone(), inference)
    }

    /// [`check`] driven by [`build_with_inference`]'s output — the harness
    /// every test below shares.
    fn check_all(src: &str) -> Vec<Diagnostic> {
        let (hir, index, resolutions, inference) = build_with_inference(src);
        check(&[(FileId(0), &hir)], &index, &inference, &resolutions)
    }

    #[test]
    fn int_of_a_divert_target_literal_is_e078() {
        let diags =
            check_all("=== knot ===\nHello.\n-> DONE\n=== main ===\n~ x = int(-> knot)\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E078);
        assert!(diags[0].message.contains("divert"));
    }

    #[test]
    fn float_of_an_array_literal_is_e078() {
        let diags = check_all("=== main ===\n~ x = float(#[1, 2])\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E078);
        assert!(diags[0].message.contains("array"));
    }

    #[test]
    fn int_of_a_map_literal_is_e078() {
        let diags = check_all("=== main ===\n~ x = int(#{\"a\": 1})\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("map"));
    }

    #[test]
    fn int_of_a_struct_literal_is_e078() {
        let diags = check_all(
            "STRUCT Point = #{x: float}\n=== main ===\n~ y = int(Point#{x: 1.0})\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("struct"));
    }

    #[test]
    fn int_of_a_list_literal_is_e078() {
        let diags = check_all("LIST Colors = red, blue\n=== main ===\n~ x = int((red))\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("list"));
    }

    #[test]
    fn int_of_a_numeric_literal_is_clean() {
        let diags = check_all("=== main ===\n~ x = int(2.9)\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn int_of_a_string_literal_is_clean() {
        let diags = check_all("=== main ===\n~ x = int(\"42\")\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn int_of_an_in_domain_variable_is_clean() {
        // `gold`'s declaration-derived type is a concrete `int` — in-domain,
        // so this stays clean (not because it's unclassifiable, but because
        // its resolved type is permitted).
        let diags = check_all("VAR gold = 5\n=== main ===\n~ x = int(gold)\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn string_of_a_divert_target_is_never_checked() {
        // Ruling 2: `string()` accepts every type.
        let diags = check_all(
            "=== knot ===\nHello.\n-> DONE\n=== main ===\n~ x = string(-> knot)\n-> DONE\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn author_defined_int_shadowing_the_builtin_is_never_checked() {
        let diags = check_all(
            "=== function int(x) ===\n~ return 0\n=== main ===\n~ y = int(-> main)\n-> DONE\n",
        );
        assert!(
            diags.is_empty(),
            "a resolved call to the author's own `int` must never be flagged: {diags:?}"
        );
    }

    #[test]
    fn wrong_arity_int_call_is_not_flagged_here() {
        // E031's job, not this pass's — this pass only checks the arg when
        // arity is exactly 1.
        let diags = check_all("=== main ===\n~ x = int(1, 2)\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── issue #983: variable/call/index-valued arguments ────────────────

    #[test]
    fn global_variable_valued_argument_fires_when_provably_mistyped() {
        // `v`'s declaration-derived type is a concrete `divert` (its own
        // divert-target initializer) — out of `int`/`float`'s permitted
        // domain, so this now fires exactly like a literal `-> knot`
        // argument would. (Issue #1540 widened `collect_globals` to full
        // `Ty` fidelity, so a global's `array`/`map`/`struct`-literal
        // initializer now drives this same dispatch too — see
        // `global_array_valued_argument_fires_since_the_value_ty_widening`
        // just below. `divert` stays the fixture here because it is the
        // shape this issue's own scope introduced.)
        let diags = check_all(
            "=== knot ===\nHello.\n-> DONE\nVAR v = -> knot\n=== main ===\n~ x = int(v)\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E078);
        assert!(
            diags[0].message.contains("divert"),
            "{:?}",
            diags[0].message
        );
    }

    /// Issue #1540: the collection shapes that used to be dropped on the
    /// way into `collect_globals` now reach this dispatch. Before the
    /// `Sig::value_ty` widening this compiled clean — a latent miss, not a
    /// design choice — while the `temp` twin
    /// (`temp_variable_valued_argument_fires_when_provably_mistyped`)
    /// reported for the identical value.
    #[test]
    fn global_array_valued_argument_fires_since_the_value_ty_widening() {
        let diags = check_all("VAR xs = #[1, 2]\n=== main ===\n~ x = int(xs)\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E078);
        assert!(diags[0].message.contains("array"), "{:?}", diags[0].message);
    }

    #[test]
    fn global_variable_valued_argument_of_in_domain_type_is_clean() {
        let diags = check_all("VAR v = 1.0\n=== main ===\n~ x = int(v)\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unused_param_variable_valued_argument_stays_silent_when_unknown() {
        // `n` is never used anywhere else in the body, so it stays
        // `Unknown` — "Unknown never disagrees" holds here exactly as it
        // does for `structs::check`'s `E071`.
        let diags = check_all("=== main(n) ===\n~ x = int(n)\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn temp_variable_valued_argument_fires_when_provably_mistyped() {
        // `xs`'s finalized `BodyTypes::locals` type is `Map<string, int>`
        // (its own literal initializer) — out of domain.
        let diags = check_all("=== main ===\n~ temp xs = #{\"a\": 1}\n~ x = int(xs)\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E078);
        assert!(diags[0].message.contains("map"), "{:?}", diags[0].message);
    }

    #[test]
    fn call_valued_argument_fires_when_provably_mistyped() {
        // `shape()`'s only `~ return` is a `STRUCT` literal, so its
        // finalized `InferredSig::return_ty` is a concrete struct type —
        // out of domain.
        let diags = check_all(
            "STRUCT Point = #{x: float}\n\
             === function shape() ===\n~ return Point#{x: 1.0}\n\
             === main ===\n~ y = int(shape())\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E078);
        assert!(
            diags[0].message.contains("struct"),
            "{:?}",
            diags[0].message
        );
    }

    #[test]
    fn call_valued_argument_of_in_domain_return_type_is_clean() {
        let diags = check_all(
            "=== function label() ===\n~ return \"a\"\n\
             === main ===\n~ x = int(label())\n-> DONE\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn index_valued_argument_fires_when_provably_mistyped() {
        // `xs` is a local `~ temp` bound to a `#[#[..], #[..]]`
        // array-of-arrays literal, so its finalized locals type is
        // `Array<Array<int>>` — indexing it yields `Array<int>`, out of
        // domain.
        let diags = check_all(
            "=== main ===\n\
             ~ temp xs = #[#[1, 2], #[3, 4]]\n~ x = int(xs[0])\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E078);
        assert!(diags[0].message.contains("array"), "{:?}", diags[0].message);
    }

    #[test]
    fn index_valued_argument_of_in_domain_result_is_clean() {
        let diags = check_all(
            "=== main ===\n\
             ~ temp xs = #[1.0, 2.0]\n~ x = int(xs[0])\n-> DONE\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn index_valued_argument_stays_silent_when_unknown() {
        // `xs` is only ever indexed, never assigned/observed to a concrete
        // type elsewhere, so it stays `Unknown` — no false-flagging.
        let diags = check_all("=== main(xs) ===\n~ x = int(xs[0])\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn stitch_local_variable_valued_argument_fires_when_provably_mistyped() {
        // Every other non-literal-classification test above only ever
        // exercises knot scope (`main`) or file scope — never a stitch
        // body. This drives the `enter_stitch`/`stitch_locals` dispatch
        // path specifically (the exact gap PR #975's own review caught for
        // `structs::check`): `t`'s finalized `BodyTypes::locals` type is a
        // concrete `Array<int>` — out of domain.
        let diags =
            check_all("=== room ===\n= inside\n~ temp t = #[1, 2]\n~ x = int(t)\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E078);
        assert!(diags[0].message.contains("array"), "{:?}", diags[0].message);
    }

    #[test]
    fn variable_valued_argument_inside_var_initializer_uses_global_scope_only() {
        // A call in a file-level VAR/CONST initializer has no enclosing
        // knot/stitch body — only a reference to *another* global is
        // classifiable there (mirrors `structs::check`'s identical file-scope
        // note). `other`'s declared type (`List<Colors>`, one of the two
        // non-scalar shapes `InferredType` represents — see the previous
        // test's comment) is out of domain.
        let diags = check_all("LIST Colors = red, blue\nVAR other = (red)\nVAR x = int(other)\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E078);
        assert!(diags[0].message.contains("list"), "{:?}", diags[0].message);
    }

    #[test]
    fn mistyped_variable_argument_diagnostic_is_order_independent() {
        // #627 discipline (mirrored from `structs::tests::
        // mistyped_variable_field_diagnostic_is_order_independent`): `v`'s
        // out-of-domain classification (and the resulting E078) must not
        // depend on whether its `int(v)` call comes before or after another,
        // unrelated `int()` call in the same body.
        let forward = "=== knot ===\nHello.\n-> DONE\nVAR v = -> knot\n\
             === main ===\n~ x = int(v)\n~ y = int(2.9)\n-> DONE\n";
        let reversed = "=== knot ===\nHello.\n-> DONE\nVAR v = -> knot\n\
             === main ===\n~ y = int(2.9)\n~ x = int(v)\n-> DONE\n";

        let diags_f = check_all(forward);
        let diags_r = check_all(reversed);

        assert_eq!(diags_f.len(), 1, "{diags_f:?}");
        assert_eq!(diags_f[0].code, DiagnosticCode::E078);
        assert!(
            diags_f[0].message.contains("divert"),
            "{:?}",
            diags_f[0].message
        );

        assert_eq!(diags_r.len(), 1, "{diags_r:?}");
        assert_eq!(diags_r[0].code, DiagnosticCode::E078);
        assert!(
            diags_r[0].message.contains("divert"),
            "{:?}",
            diags_r[0].message
        );
    }
}
