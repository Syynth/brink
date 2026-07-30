//! `contains(m, needle)` static key-domain warning (`E152`, issue #582,
//! companion to #580's ruling — `docs/decision-log.md` 2026-07-12
//! "contains(map, non-key-domain needle) returns false").
//!
//! #580 ruled that `contains(m, needle)` is *total*: when `m` is a map and
//! `needle` is outside the runtime key domain (int/string/bool —
//! `brink_format::MapKey::from_value`'s exact permitted set), the call
//! returns `false` rather than raising `InvalidMapKeyType`. That ruling's
//! own text deferred a **static** half: "a static analyzer warning for
//! statically-visible non-key needles is deferred (#582)". This module is
//! that half — flagging the call at compile time when both halves of the
//! always-false shape are already visible in the source, so the author
//! sees the mistake before running the story rather than discovering a
//! silently-empty membership test at runtime.
//!
//! # Why this needs the inference substrate (strict-mode-only)
//!
//! Unlike [`crate::map_keys`]'s `E106` (a map-literal key's own domain,
//! checkable from the literal's syntax alone with zero type resolution),
//! this check needs to answer two static-typing questions no purely
//! syntactic pass can: "is `m` a map at all?" (a `contains` call on an
//! *array* has no key-domain restriction — it's ordinary structural
//! element containment) and "what is `needle`'s type, when it isn't a bare
//! literal?" (a `VAR`, a local `temp`, a call result, an index expression).
//! Both questions are exactly [`crate::structs::classify_expr_ty`]'s job —
//! the same whole-project inference-substrate classifier
//! [`crate::conversions`]'s `E078` and [`crate::range_refinement`]'s `E117`
//! already reuse for their own domain checks. That substrate
//! ([`crate::infer::InferenceResult`]) is only ever computed under `types =
//! strict` (`brink_analyzer::strict_diagnostics`'s own gate), so this check
//! is wired into [`crate::strict::check`] alongside them, strict-mode-only
//! like the rest of that family. Under `types = gradual` this stays silent
//! — the runtime's total, non-faulting `false` return (#580) is already the
//! correct residual, so gradual mode loses nothing but the compile-time
//! convenience.
//!
//! # The exact shape flagged
//!
//! An **unresolved** (not author-shadowed, `docs/t1b-surface-spec.md` §5)
//! call to `contains` with exactly two arguments (a wrong arity is `E031`'s
//! job, not this pass's), where:
//!
//! 1. the first argument (`m`) classifies, via
//!    [`crate::structs::classify_expr_ty`], to a concrete `Ty::Map(_, _)`
//!    — an `Unknown`/`Conflicted` or non-`Map` (e.g. `Ty::Array`) result
//!    means "not statically known to be a map", and the call is left
//!    alone; and
//! 2. the second argument (`needle`) classifies to a concrete type outside
//!    the int/string/bool key domain — see [`non_key_domain_kind`].
//!
//! `classify_expr_ty` already reaches every shape its own family
//! (`conversions`/`range_refinement`) does: a bare literal, a `Path` to a
//! global `VAR`/`CONST` (declaration-derived, issue #1540's full-fidelity
//! `Sig::value_ty`) or to a local param/`temp` (the enclosing def's
//! finalized `BodyTypes::locals` — including one typed by the native `:
//! type` annotation grammar, issues #1487-#1489), a `Call` to a resolved
//! knot/stitch (its finalized `InferredSig::return_ty`), or an `Index`
//! (recursing into its base's array-element/map-value type). This is the
//! "far more cases" the issue's own re-scoping note anticipated for the
//! **container** and for a **global-valued needle** — see the "deliberately
//! excluded shapes" section below for the one reach `classify_expr_ty`
//! promises in general but this specific call shape can't actually use: a
//! local temp/param-valued *needle*.
//!
//! # Precision over recall: deliberately excluded shapes
//!
//! - **Any needle whose classified type is `Unknown`/`Conflicted`** —
//!   "Unknown never disagrees" (the same posture every sibling check in
//!   this family takes): an unannotated, never-narrowed param, a
//!   never-observed local, or an expression shape `classify_expr_ty`
//!   doesn't handle (field access, arithmetic, `?:`/coalesce, …) stays
//!   silently unchecked. A false "always false" claim would be strictly
//!   worse than a miss (the issue's own framing).
//! - **A container not statically known to be a map** — an `Array`
//!   receiver has no key-domain restriction at all (`contains` on an array
//!   is structural element-equality against any type, matching
//!   `collection_ops::map_contains`'s own `Value::Array` branch), and an
//!   `Unknown`/`Conflicted` receiver might resolve to either at runtime, so
//!   guessing "map" would risk a false positive against legitimate array
//!   code. Only a **provably** map-typed first argument is in scope.
//! - **A needle whose type is int/string/bool but disagrees with the
//!   map's own declared key type** (e.g. a `string` needle against a
//!   statically `map<int, _>` receiver) is deliberately **not** flagged.
//!   #580's ruling — and this companion — is about the *general* runtime
//!   key domain (`MapKey`'s three variants), not a specific map's declared
//!   `K`; a mismatch there is a different, narrower claim (it depends on
//!   trusting the map's static `K` never diverging from its runtime
//!   contents) that risks exactly the false-positive-on-legitimate-code
//!   failure mode the issue's own text warns is worse than a miss.
//! - **`contains_value(m, v)`** (map *values*, `MapContainsValue`) is a
//!   different verb with no key-domain restriction at all — out of scope
//!   by construction (this pass only matches the literal name `contains`).
//! - **A local `temp`/param used as the *needle*** (never the container —
//!   see the tests) is structurally unreachable, discovered empirically
//!   while writing this module's own test suite: `infer::body`'s
//!   pre-existing `"contains"` arm (unrelated to this issue — it exists so
//!   `intrinsic_fault_discharged` can prove a call total) calls
//!   `self.observe(needle, key_ty)` at every `contains` call site, which
//!   *unifies* the needle-local's finalized `BodyTypes::locals` entry with
//!   the container's own key type right there. A needle-local with an
//!   independently-established different type doesn't surface as "provably
//!   mismatched" — it unifies to `Ty::Conflicted`, filtered by "Unknown
//!   never disagrees"; a needle-local with no independent type simply
//!   *becomes* the container's key type and so can never disagree with it
//!   either. This is flagged as scope discovered beyond #582, not worked
//!   around here — closing it would mean reading the needle's
//!   pre-observation type from a separate snapshot, a change to the shared
//!   inference substrate outside this issue's fence.
//! - **Gradual-mode projects** get no static signal from this pass at all
//!   (see the module doc's inference-substrate note above) — the runtime
//!   `false` return is the only residual there, unchanged from #580.
//!
//! # Shadowing
//!
//! An unresolved call to `contains` is the builtin; a call that *did*
//! resolve (an author-defined `contains` knot) is an ordinary function
//! call and is never flagged here — mirrors `conversions::check`'s own
//! shadowing rule exactly (`docs/t1b-surface-spec.md` §5's "author-defined
//! function with the same name shadows the builtin" ruling).

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

/// Strict-mode-only `contains(m, needle)` key-domain checks over every call
/// in the project. Callers only reach this once `strict::config_error` has
/// confirmed `types = strict` + `dialect = brink` (mirrors
/// `conversions::check`'s/`range_refinement::check`'s own entry condition —
/// same wiring point, `strict::check`).
///
/// `index`/`inference`: the same whole-project `SymbolIndex`/
/// `InferenceResult` `strict::check` already computes for its own
/// escape/mismatch checks — this is what lets [`structs::classify_expr_ty`]
/// classify a variable/call/index-valued argument instead of only
/// literal-shaped ones (see the module doc).
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
        let mut v = ContainsVisitor {
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
        // block-tree walk — same pattern `conversions::check`/
        // `structs::check` use for VAR/CONST. No enclosing def here, so
        // `locals` is `None` — only a reference to a global VAR/CONST is
        // classifiable at file scope.
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

struct ContainsVisitor<'a> {
    file: FileId,
    index: &'a SymbolIndex,
    globals: &'a BTreeMap<DefinitionId, Ty>,
    signatures: &'a BTreeMap<DefinitionId, InferredSig>,
    bodies: &'a BTreeMap<DefinitionId, crate::infer::BodyTypes>,
    resolution_by_range: &'a BTreeMap<(u32, u32), DefinitionId>,
    /// The currently-open knot's own name — `enter_stitch` needs it to
    /// reconstruct the qualified `knot.stitch` name a stitch is indexed
    /// under. Mirrors `conversions::ConversionVisitor`'s identical field.
    current_knot_name: Option<String>,
    /// The enclosing knot's own finalized locals, set for the duration of
    /// its body (and every stitch nested inside it, until `enter_stitch`
    /// overrides it with the stitch's own).
    knot_locals: Option<&'a BTreeMap<String, Ty>>,
    /// The currently-open stitch's own finalized locals, if any — takes
    /// priority over `knot_locals` while set.
    stitch_locals: Option<&'a BTreeMap<String, Ty>>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl<'a> ContainsVisitor<'a> {
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
    /// `conversions::ConversionVisitor::knot_def_id` exactly.
    fn knot_def_id(&self, knot: &Knot) -> Option<DefinitionId> {
        let kind = knot.symbol_kind();
        annotations::def_id_for(self.index, self.file, kind, &knot.name.text)
    }
}

impl HirVisitor for ContainsVisitor<'_> {
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
        // `conversions::ConversionVisitor::enter_stitch` exactly.
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

/// Recurse into `expr` looking for `contains` calls — used only for the
/// file-level VAR/CONST initializers `visit::visit` doesn't cover; every
/// other position is already reached through the `HirVisitor` walk above.
/// Mirrors `conversions::check_expr`'s own shape exactly.
fn check_expr(expr: &Expr, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    check_call(expr, file, ctx, out);
    for child in expr_children(expr) {
        check_expr(child, file, ctx, out);
    }
}

/// Direct child expressions of `expr` — mirrors `conversions::expr_children`/
/// `structs::expr_children` (needed only because `check_expr` runs outside
/// the `HirVisitor` walk).
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
        Expr::FnLiteral(fl) => fl.args.iter().collect(),
        Expr::RefArg(ra) => vec![&ra.operand],
        // A lambda's whole body (issue #1685, #1764). An always-false
        // `contains` is always false wherever the call sits, so a braced
        // body's *statements* are reached too, not just its trailing value
        // expression — see `LambdaBody::all_exprs`.
        Expr::Lambda(l) => l.body.all_exprs(),
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
/// `contains` with exactly two arguments, whose first argument classifies
/// to a concrete `Ty::Map(_, _)` and whose second argument classifies to a
/// concrete out-of-key-domain type — push `E152`. See the module doc's
/// "the exact shape flagged" / "deliberately excluded shapes" sections for
/// the full rule.
fn check_call(expr: &Expr, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    let Expr::Call(path, args) = expr else {
        return;
    };
    let [seg] = path.segments.as_slice() else {
        return;
    };
    if seg.text != "contains" {
        return;
    }
    if ctx.resolution_by_range.contains_key(&range_key(path.range)) {
        return; // resolved to an author-defined symbol — shadows the builtin
    }
    let [container, needle] = args.as_slice() else {
        return; // wrong arity — E031's job, not this pass's
    };
    let Some(Ty::Map(_, _)) = structs::classify_expr_ty(container, ctx) else {
        return; // not statically known to be a map
    };
    let Some(kind) = non_key_domain_kind(needle, ctx) else {
        return;
    };
    out.push(Diagnostic {
        file,
        range: path.range,
        message: format!(
            "{}: the needle is a statically-known `{kind}` value, which is outside the \
             int/string/bool key domain — `contains` on this map always returns `false` \
             (docs/decision-log.md 2026-07-12 ruling, issue #580)",
            DiagnosticCode::E152.title(),
        ),
        code: DiagnosticCode::E152,
    });
}

/// Classify `expr` as statically outside the map key domain
/// (int/string/bool) — [`literal_non_key_domain_kind`]'s direct-literal
/// classification first (the three shapes [`structs::classify_expr_ty`]
/// doesn't itself resolve: a divert-target literal, a `LIST` literal, an
/// `#fn(...)` literal), falling back to [`structs::classify_expr_ty`]'s
/// inference-substrate classification (which already covers the scalar/
/// array/map/struct literal shapes plus `Path`/`Call`/`Index`) mapped
/// through [`non_key_domain_kind_for_ty`]. `None` — "in-domain or not
/// classifiable" — for int/string/bool, `Unknown`/`Conflicted`, or any
/// expression shape neither classifier reaches: the same "Unknown never
/// disagrees" posture every sibling check in this family takes.
fn non_key_domain_kind(expr: &Expr, ctx: &MistypeCtx<'_>) -> Option<&'static str> {
    if let Some(kind) = literal_non_key_domain_kind(expr) {
        return Some(kind);
    }
    let ty = structs::classify_expr_ty(expr, ctx)?;
    non_key_domain_kind_for_ty(&ty)
}

/// The three literal shapes [`structs::classify_expr_ty`] doesn't classify
/// on its own (it has no `Ty::Divert`/`Ty::List`/`Ty::Fn` literal arm —
/// only `Path`/`Call`/`Index` reach those, through a *resolved* symbol).
/// Mirrors [`crate::map_keys::non_key_domain_kind`]'s own label vocabulary
/// for message consistency between the two "outside the key domain" checks
/// (`E106`'s literal-key sibling and this one).
fn literal_non_key_domain_kind(expr: &Expr) -> Option<&'static str> {
    match expr {
        Expr::DivertTarget(_) => Some("divert target"),
        Expr::ListLiteral(_) => Some("list"),
        Expr::FnLiteral(_) => Some("function"),
        _ => None,
    }
}

/// Map a resolved [`Ty`] to its `E152` "kind" word — every concrete type
/// outside `Int`/`String`/`Bool` (the exact `MapKey::from_value` domain,
/// `brink_format::value::MapKey`), `None` for the permitted domain itself
/// and for `Unknown`/`Conflicted` (not statically classifiable at all).
fn non_key_domain_kind_for_ty(ty: &Ty) -> Option<&'static str> {
    match ty {
        Ty::Float => Some("float"),
        Ty::Divert => Some("divert target"),
        Ty::List(_) => Some("list"),
        Ty::Array(_) => Some("array"),
        Ty::Map(_, _) => Some("map"),
        Ty::Struct(_) => Some("struct"),
        Ty::Fn(..) => Some("function"),
        Ty::Handle(_) => Some("handle"),
        Ty::Option(_) => Some("option"),
        Ty::Range { .. } => Some("range"),
        Ty::Weighted(_) => Some("weighted"),
        Ty::Tower(_) => Some("tower"),
        // The permitted key domain itself (`Int`/`String`/`Bool`) and
        // "not statically classifiable at all" (`Unknown`/`Conflicted`)
        // both resolve to "don't flag" — distinct *reasons*, identical
        // outcome, so clippy's `match_same_arms` wants them merged.
        Ty::Int | Ty::String | Ty::Bool | Ty::Unknown | Ty::Conflicted => None,
    }
}

fn range_key(range: TextRange) -> (u32, u32) {
    (range.start().into(), range.end().into())
}

/// This file's own reference resolutions, projected to a range-keyed lookup
/// — mirrors `strict::resolution_index`/`conversions::resolution_index`
/// (same rationale: a `Path`'s range is only unique within its own file).
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

    /// Computes real resolutions and a whole-project `InferenceResult` —
    /// needed by every test, since `check` always consults that substrate
    /// for its container/needle classification. Mirrors
    /// `conversions::tests::build_with_inference` exactly.
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

    fn check_all(src: &str) -> Vec<Diagnostic> {
        let (hir, index, resolutions, inference) = build_with_inference(src);
        check(&[(FileId(0), &hir)], &index, &inference, &resolutions)
    }

    /// [`check_all`]'s native-surface twin — lambdas exist only on the
    /// native surface, so the #1764 fixtures below need `lower_native`.
    fn check_all_native(src: &str) -> Vec<Diagnostic> {
        let parsed = brink_syntax_native::parse(src);
        assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
        let (hir, manifest, _diag) = brink_ir::hir::lower_native::lower(FileId(0), &parsed.tree());
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
        check(&[(FileId(0), &hir)], &index, &inference, &resolutions)
    }

    // ── issue #1764: a lambda's statements in a VAR/CONST initializer ────

    /// The VAR/CONST-initializer recursion is the one walk that isn't
    /// `visit::visit`'s (which already descends a lambda's statements), so it
    /// has to descend them itself.
    #[test]
    fn an_always_false_contains_in_a_lambda_statement_of_a_var_initializer_is_e152() {
        let diags = check_all_native(
            "var f = ||: int {\n  let hit = contains(Map { 1: \"a\" }, 3.5);\n  0\n};\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E152);
        assert!(diags[0].message.contains("float"), "{:?}", diags[0].message);
    }

    /// The tail position was already covered — pinned so a later refactor
    /// can't trade one half of the body for the other.
    #[test]
    fn an_always_false_contains_in_a_lambda_tail_of_a_var_initializer_is_still_e152() {
        let diags = check_all_native(
            "var f = ||: bool {\n  let a = 1;\n  contains(Map { 1: \"a\" }, 3.5)\n};\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E152);
    }

    // ── literal needle, literal map container ───────────────────────────

    #[test]
    fn float_needle_against_a_map_literal_is_e152() {
        let diags = check_all("=== main ===\n~ x = contains(#{1: \"a\"}, 3.5)\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E152);
        assert!(diags[0].message.contains("float"), "{:?}", diags[0].message);
    }

    #[test]
    fn array_needle_against_a_map_literal_is_e152() {
        let diags = check_all("=== main ===\n~ x = contains(#{1: \"a\"}, #[1, 2])\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("array"), "{:?}", diags[0].message);
    }

    #[test]
    fn map_needle_against_a_map_literal_is_e152() {
        let diags = check_all("=== main ===\n~ x = contains(#{1: \"a\"}, #{2: \"b\"})\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("map"), "{:?}", diags[0].message);
    }

    #[test]
    fn struct_needle_against_a_map_literal_is_e152() {
        let diags = check_all(
            "STRUCT Point = #{x: int}\n\
             === main ===\n~ x = contains(#{1: \"a\"}, Point#{x: 1})\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("struct"),
            "{:?}",
            diags[0].message
        );
    }

    #[test]
    fn list_needle_against_a_map_literal_is_e152() {
        let diags = check_all(
            "LIST Colors = red, blue\n\
             === main ===\n~ x = contains(#{1: \"a\"}, (red))\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("list"), "{:?}", diags[0].message);
    }

    #[test]
    fn divert_target_needle_against_a_map_literal_is_e152() {
        let diags = check_all(
            "=== main ===\n~ x = contains(#{1: \"a\"}, -> other)\n-> DONE\n\
             === other ===\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("divert target"),
            "{:?}",
            diags[0].message
        );
    }

    #[test]
    fn fn_literal_needle_against_a_map_literal_is_e152() {
        let diags = check_all(
            "=== main ===\n~ x = contains(#{1: \"a\"}, #fn(score))\n-> DONE\n\
             === score(x) ===\n~ return x\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("function"),
            "{:?}",
            diags[0].message
        );
    }

    // ── in-domain needles: never flagged ─────────────────────────────────

    #[test]
    fn int_needle_is_clean() {
        let diags = check_all("=== main ===\n~ x = contains(#{1: \"a\"}, 2)\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn string_needle_is_clean() {
        let diags = check_all("=== main ===\n~ x = contains(#{1: \"a\"}, \"k\")\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn bool_needle_is_clean() {
        let diags = check_all("=== main ===\n~ x = contains(#{1: \"a\"}, true)\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// A needle in the key domain (`string`) but disagreeing with the map's
    /// own statically-declared key type (`int`) — deliberately NOT flagged,
    /// see the module doc's "deliberately excluded shapes" section.
    #[test]
    fn key_domain_needle_of_a_mismatched_map_key_type_is_clean() {
        let diags =
            check_all("=== main ===\n~ x = contains(#{1: \"a\"}, \"not-an-int\")\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── container must be provably a map ─────────────────────────────────

    #[test]
    fn float_needle_against_an_array_container_is_clean() {
        // Arrays have no key-domain restriction — element containment is
        // structural equality against any type.
        let diags = check_all("=== main ===\n~ x = contains(#[1, 2], 3.5)\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn float_needle_against_an_unknown_container_is_clean() {
        let diags = check_all("=== main(m) ===\n~ x = contains(m, 3.5)\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── global VAR/CONST container and needle (issue #1540 reachability) ─

    #[test]
    fn global_map_var_container_with_float_needle_is_e152() {
        let diags = check_all(
            "VAR scores = #{1: \"a\"}\n=== main ===\n~ x = contains(scores, 3.5)\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("float"), "{:?}", diags[0].message);
    }

    #[test]
    fn global_const_map_container_with_array_needle_is_e152() {
        let diags = check_all(
            "CONST lookup = #{1: \"a\"}\n\
             === main ===\n~ x = contains(lookup, #[1])\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("array"), "{:?}", diags[0].message);
    }

    #[test]
    fn global_float_var_needle_against_a_map_literal_is_e152() {
        let diags =
            check_all("VAR f = 3.5\n=== main ===\n~ x = contains(#{1: \"a\"}, f)\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("float"), "{:?}", diags[0].message);
    }

    #[test]
    fn global_int_var_needle_is_clean() {
        let diags = check_all("VAR i = 5\n=== main ===\n~ x = contains(#{1: \"a\"}, i)\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── local temp/param NEEDLE: structurally unreachable, by design ─────
    //
    // Unlike `conversions`/`range_refinement`'s own local-temp fixtures, a
    // local `temp`/param used as `contains`'s *needle* (never its
    // container) can never be caught provably-mismatched here — discovered
    // empirically while writing this suite, not a gap introduced by this
    // pass. `infer::body`'s own pre-existing `"contains"` arm (unrelated to
    // this issue — it exists to let `intrinsic_fault_discharged` prove a
    // call total) calls `self.observe(needle, key_ty)` at every `contains`
    // call site, which *unifies* the needle-local's finalized
    // `BodyTypes::locals` entry with the container's own key/element type
    // right there. A local whose independent uses already established a
    // different concrete type doesn't surface as "provably mismatched" —
    // it unifies to `Ty::Conflicted` instead, which this pass (like every
    // sibling in the family) treats as unclassifiable ("Unknown never
    // disagrees"). A local with no independent type simply *becomes* the
    // container's key type, so it can never disagree with it either. Net
    // effect: this pass's local-needle reach is limited to what it can
    // prove empty (`Unknown`) — never a genuine positive. Flagged as scope
    // discovered beyond #582 rather than worked around here (would need a
    // needle-specific pre-observation type snapshot, a change to shared
    // inference substrate well outside this issue's fence).

    #[test]
    fn temp_array_needle_never_fires_self_unifies_or_conflicts() {
        let diags = check_all(
            "=== main ===\n~ temp xs = #[1, 2]\n~ x = contains(#{1: \"a\"}, xs)\n-> DONE\n",
        );
        assert!(
            diags.is_empty(),
            "a temp needle self-unifies/Conflicts against the container's key type, per the \
             `infer::body` `contains` arm — see this test's module-doc-adjacent comment: {diags:?}"
        );
    }

    #[test]
    fn temp_map_container_fires_when_needle_is_out_of_domain() {
        // The container (arg 0) is never touched by `observe` — only the
        // needle is — so a temp-valued *container* is unaffected by the
        // confound above and fires normally.
        let diags =
            check_all("=== main ===\n~ temp m = #{\"a\": 1}\n~ x = contains(m, 2.5)\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("float"), "{:?}", diags[0].message);
    }

    #[test]
    fn stitch_local_temp_needle_never_fires_self_unifies_or_conflicts() {
        let diags = check_all(
            "=== room ===\n= inside\n~ temp xs = #[1, 2]\n\
             ~ x = contains(#{1: \"a\"}, xs)\n-> DONE\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unresolved_param_needle_stays_silent_when_unknown() {
        let diags = check_all("=== main(n) ===\n~ x = contains(#{1: \"a\"}, n)\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── call-valued container/needle ─────────────────────────────────────

    #[test]
    fn call_valued_needle_fires_when_provably_out_of_domain() {
        let diags = check_all(
            "STRUCT Point = #{x: int}\n\
             === function shape() ===\n~ return Point#{x: 1}\n\
             === main ===\n~ x = contains(#{1: \"a\"}, shape())\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("struct"),
            "{:?}",
            diags[0].message
        );
    }

    // ── shadowing, arity, and wrong-verb exclusions ──────────────────────

    #[test]
    fn author_defined_contains_shadowing_the_builtin_is_never_checked() {
        let diags = check_all(
            "=== function contains(a, b) ===\n~ return true\n\
             === main ===\n~ x = contains(#{1: \"a\"}, 3.5)\n-> DONE\n",
        );
        assert!(
            diags.is_empty(),
            "a resolved call to the author's own `contains` must never be flagged: {diags:?}"
        );
    }

    #[test]
    fn wrong_arity_contains_call_is_not_flagged_here() {
        let diags = check_all("=== main ===\n~ x = contains(#{1: \"a\"})\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn contains_value_is_never_checked_by_this_pass() {
        // A different verb (map *values*, no key-domain restriction) —
        // this pass only matches the literal name `contains`.
        let diags = check_all("=== main ===\n~ x = contains_value(#{1: \"a\"}, 3.5)\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── gradual mode: no static signal (module doc's inference-substrate
    // note) ───────────────────────────────────────────────────────────────

    #[test]
    fn gradual_mode_never_reaches_this_pass_at_all() {
        // `check` itself doesn't gate on `types` — `strict::check`'s own
        // wiring point does (this module doc's "why this needs the
        // inference substrate" section). This test pins the module-level
        // contract: called directly (as every test above does) it always
        // classifies from whatever `InferenceResult` it's handed, so the
        // *policy* gate lives one level up. Full pipeline reachability
        // (including the gradual-mode silence) is proven in
        // `crates/brink-compiler/tests/e0xx_diagnostics.rs`.
        let diags = check_all("=== main ===\n~ x = contains(#{1: \"a\"}, 3.5)\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }
}
