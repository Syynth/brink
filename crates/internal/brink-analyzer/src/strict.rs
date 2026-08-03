//! TM-3 strict typed-mode policy (docs/typed-mode-spec.md §1/§4/§5/§9-step-3).
//!
//! `types = strict` is a project-level config option, orthogonal to (but
//! gated by) the T1b dialect (`docs/t1b-surface-spec.md` §1): strict typing
//! requires the brink dialect, since its annotation syntax (TM-2, spec §3)
//! is brink-extension syntax. Three jobs live here:
//!
//! - [`config_error`]: `types = strict` + `dialect = strict-ink` is a
//!   project-level config error (`E064`), reported once and skipping every
//!   other strict-mode check (there is nothing more useful to say about a
//!   project whose dialect already rejects the annotation syntax strict mode
//!   needs).
//! - [`check`]: the inference-driven strict diagnostics — Unknown-escape
//!   (`E065`) and Conflicted-escape (`E066`) over every inferable def's
//!   signature and body-local slots (spec §1: "Unknown escaping inference is
//!   a compile error"; the #627-landed `Ty::Conflicted` absorbing point is
//!   strict mode's payoff, spec's own words: "TM-3 (#619) is the slice that
//!   turns a Conflicted slot into a real strict-mode error"), the void-
//!   assignment check (`E067`, spec §3: "assigning a `void` call is an error
//!   in strict mode" — a `~ x = f()` / `~ temp x = f()` whose RHS *root* is a
//!   call resolving to a `void`-returning function; statement-position calls
//!   and calls nested in interpolation are never flagged), plus wiring the
//!   already-landed advisory `annotations::mismatches` (`E063`) into
//!   production under strict (the inherited #640-round ruling: "TM-3's
//!   strict-policy wiring, which must run inference anyway, is where E063
//!   starts firing in production").
//! - [`effective_severity`]: the policy-conditional severity lookup both of
//!   `brink-db`'s diagnostic-partitioning sites (`partition_diagnostics`'s
//!   two call sites, plus `lir_query`'s own LIR-diagnostic partition) must
//!   call instead of the raw [`brink_ir::DiagnosticCode::severity`] default —
//!   `E063` is `Warning` under `types = gradual` but `Error`-eligible under
//!   `types = strict` (the #640-round ruling this module's `check` doc above
//!   already cites); every other code's severity is policy-independent.
//!
//! A slot is exempted from Unknown-escape when an explicit, resolvable type
//! annotation is present (TM-2's "annotation = firewall" — the entire point
//! of annotating a boundary is to supply the concrete type inference alone
//! couldn't pin down, spec §5's own worked example: `#[]` is an `Unknown`
//! escape *unless* the binding is annotated). A `Conflicted` slot is never
//! exempted by an annotation — the body's own uses genuinely disagree with
//! each other, which no annotation can resolve (`annotations::mismatches`
//! already declines to compare against a `Conflicted`/`Unknown` body type
//! for the same reason, via [`Ty::is_unresolved`]).
//!
//! Coercion lattice (spec §4) and collection-literal joins (spec §5) need no
//! separate enforcement pass here: `infer::ty::unify` already implements the
//! lattice (`int -> float` directional, everything else structurally
//! mismatched joins to `Conflicted`), condition positions are already
//! inferred without forcing `bool` (`infer::body`'s module doc — the
//! int-truthiness idiom `{visited_knot: ...}` types as a clean concrete
//! `int`, never escapes), and a heterogeneous collection literal
//! (`#[1, "a"]`) already comes out `Array(Conflicted)` — this module's
//! recursive [`classify`] walk catches it precisely because it *is* the same
//! lattice, not a parallel implementation of it.
//!
//! ## Scope (see PR description for the full list)
//!
//! This slice does **not** implement: the boundary-annotation-*required*
//! diagnostic (spec's "host-callable functions... and entry points require
//! explicit annotations" has no ratified, mechanically-checkable definition
//! of either term in the codebase today — inventing one here would be
//! unilateral architecture, not wiring). The `int()`/`float()`/`string()`
//! pure conversion intrinsics (TM-3 completion, issue #659) now exist —
//! VM-native ops plus the `conversions` module's strict-mode domain check,
//! wired in below alongside `structs::check`.
//!
//! Issue #1877 closed a gap this doc used to describe as out of scope:
//! `VAR`/`CONST` cross-type-reassignment detection. `infer::body`'s
//! `observe` still only accumulates for `Param`/`Temp` locals into the
//! `Ty::Conflicted` lattice — that much is unchanged — but a global
//! assignment target's already-known declaration-derived type
//! (`BodyCtx::globals`) is now checked directly against the RHS's inferred
//! type ([`check_typed_assign_mismatches`], `E063`), independently of that
//! lattice. The same PR added [`check_global_initializers`] for the sibling
//! declaration-initializer gap (a VAR/CONST's own explicit annotation
//! disagreeing with its initializer literal) and a `~ temp` initializer's
//! ascription check (`infer::body::InferPass::check_declared_temp_init`).

use std::collections::{BTreeMap, BTreeSet};

use brink_format::{DefinitionId, DefinitionTag};
use brink_ir::{
    Block, BlockStmt, Content, ContentPart, ElseBranch, Expr, FileId, HirFile, IfStmt, Path,
    ResolutionMap, Stmt, SymbolIndex, SymbolKind, TypeExpr,
};
use rowan::TextRange;

use crate::annotations;
use crate::infer::{InferenceResult, InferredSig, Ty};

// `TypePolicy` is defined in `brink-project-config` alongside `Dialect` —
// both are project-policy types the analyzer consumes rather than owns, and
// keeping them there is what lets that crate publish standalone (#1234).
// Re-exported so every existing `brink_analyzer::TypePolicy` path is
// unchanged. The *default* remains dialect-keyed via `resolve_type_policy`
// below (issue #1127); the derived `Default` (`Gradual`) exists only so
// pre-resolution containers can derive theirs and must never be read as the
// policy default.
pub use brink_project_config::TypePolicy;

// `LintLevel` is defined in `brink-project-config` for the same reason as
// `TypePolicy` above (#1234). Re-exported so `brink_analyzer::LintLevel` is
// the canonical path every consumer of [`LintPolicy::overrides`] uses.
pub use brink_project_config::LintLevel;

/// The resolved `[lints]` policy (issue #1160): per-code severity overrides
/// plus the blanket `deny-warnings` flag. Bundled as its own small,
/// cheaply-`PartialEq`-comparable value — rather than as two loose scalars —
/// so `brink-db`'s severity-partitioning call sites can share one narrow
/// salsa projection the same way [`TypePolicy`] already does (see
/// `brink-db`'s `type_policy_query`/`lint_policy_query` doc comments for the
/// cutoff argument).
///
/// This is the `AnalysisOptions::lints` field's type — resolved once, at
/// `Project::load` (via `AnalysisOptions::apply_project_config`), never
/// re-derived at a call site (#1160's "apply it at the ONE point" mandate).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LintPolicy {
    /// Per-code overrides, keyed by the diagnostic code's string form
    /// (`DiagnosticCode::as_str`, e.g. `"E063"`). Only ever consulted for
    /// codes whose *default* severity ([`brink_ir::DiagnosticCode::severity`])
    /// is `Warning` — see [`effective_severity`]'s doc comment for why a
    /// hard-error-by-default code is never even looked up here.
    pub overrides: BTreeMap<String, LintLevel>,
    /// `[lints] deny-warnings = true`: promote every diagnostic that would
    /// otherwise resolve to `Warning` up to `Error` (the `-D warnings`
    /// equivalent). A code with an explicit [`Self::overrides`] entry is
    /// unaffected by this flag — `Deny` is `Error` either way, and `Allow`
    /// is specifically the "stay `Warning` even under `deny-warnings`" knob.
    pub deny_warnings: bool,
}

/// THE `types`-default resolution function (issue #1127, decision-log
/// 2026-07-19 "Typing posture ruled"). An explicit `types = …` — a CLI
/// `--types` flag, a `brink.toml` `[project] types` key, an editor/LSP API
/// call — always wins. When the project never says, the default is keyed on
/// the dialect:
///
/// - `Dialect::Brink` → **`Strict`** (the flip: the new surfaces are
///   designed under the strict doctrine; gradual remains an opt-out knob);
/// - `Dialect::StrictInk` → **`Gradual`**, forever (the oracle corpus is
///   anchored to it — byte-identity preserved by construction).
///
/// Every mount resolves through this one function (usually via
/// `AnalysisOptions::type_policy`); no other code may invent a `types`
/// default.
#[must_use]
pub fn resolve_type_policy(dialect: crate::Dialect, explicit: Option<TypePolicy>) -> TypePolicy {
    explicit.unwrap_or(match dialect {
        crate::Dialect::Brink => TypePolicy::Strict,
        crate::Dialect::StrictInk => TypePolicy::Gradual,
    })
}

/// The severity a diagnostic code should actually be reported at, given the
/// project's `types` policy and resolved `[lints]` policy — the single seam
/// every diagnostic-partitioning site must call instead of the raw
/// [`brink_ir::DiagnosticCode::severity`] default.
///
/// Resolution order:
///
/// 1. **Type-policy carve-out** (the #640-round ruling: "TM-3's
///    strict-policy wiring, which must run inference anyway, is where E063
///    starts firing in production") — `E063` (annotation-vs-inference
///    mismatch) is `Warning` under `types = gradual` but `Error` under
///    `types = strict`. Every other code's *base* severity is
///    policy-independent and comes straight from
///    [`brink_ir::DiagnosticCode::severity`].
/// 2. **Hard-error exemption** (issue #1160): if the base severity is
///    already `Error`, `lints` is never consulted — a code that is a hard
///    error by default can never be downgraded by `[lints]`. This is the
///    "conservative overridable set" #1160 asks for: rather than inventing
///    a policy for which `Error`-default codes are "safe" to relax, none of
///    them are reachable through this table at all. Everything else (a
///    `Warning`-base code, or — since issue #1674 — an `Info`/`Hint`-base
///    one) *is* reachable; the exemption is specifically about `Error`, not
///    about `Warning` being the only overridable base.
/// 3. **`[lints]` per-code override**: `Deny` → `Error`; `Allow` → the base
///    severity unchanged, immune to step 4; `Info`/`Hint` (issue #1162) →
///    `Severity::Info`/`Severity::Hint`, also immune to step 4 — an author
///    who deliberately down-leveled a code to an advisory tier does not want
///    `deny-warnings` escalating it back past `Warning`, the same reasoning
///    `Allow`'s immunity already rests on; `Warn` → `Severity::Warning`
///    unconditionally (an explicit ask to promote an `Info`/`Hint`-base code
///    up a tier, or to restate a `Warning`-base code's own default).
/// 4. **`deny-warnings`**: *no* override, resolving to the base severity —
///    becomes `Error` if that base is `Warning` and `lints.deny_warnings` is
///    set (the `-D warnings` equivalent); an `Info`/`Hint`-base code with no
///    override is never touched by `deny-warnings` (issue #1674: the
///    default-`Info` `E157`'s whole point is staying quiet until an author
///    opts it up through `[lints]` — `deny-warnings` alone must not do that
///    for them).
#[must_use]
pub fn effective_severity(
    code: brink_ir::DiagnosticCode,
    types: TypePolicy,
    lints: &LintPolicy,
) -> brink_ir::Severity {
    let base = if code == brink_ir::DiagnosticCode::E063 && types == TypePolicy::Strict {
        brink_ir::Severity::Error
    } else {
        code.severity()
    };

    if base == brink_ir::Severity::Error {
        return base;
    }

    // The "candidate" severity before `deny-warnings` gets a look: an
    // explicit `Deny`/`Allow`/`Info`/`Hint` override resolves (and returns)
    // immediately, same as before #1674 — none of those four are ever
    // touched by `deny-warnings` (`Allow`/`Info`/`Hint` are deliberate
    // downgrades immune to it by design; `Deny` is already `Error`).
    // `Warn`/unset both fall through to the shared `deny-warnings` check
    // below — byte-identical to the pre-#1674 `Warning`-base-only version
    // of this function when `base == Warning` (see this module's
    // `info_base_code_*` tests for the new `Info`/`Hint`-base behavior this
    // generalization adds).
    let candidate = match lints.overrides.get(code.as_str()) {
        Some(LintLevel::Deny) => return brink_ir::Severity::Error,
        Some(LintLevel::Allow) => return base,
        Some(LintLevel::Info) => return brink_ir::Severity::Info,
        Some(LintLevel::Hint) => return brink_ir::Severity::Hint,
        // An explicit `warn` always means "Warning", regardless of the
        // code's own base — the one case where the override outranks a
        // non-`Warning` base.
        Some(LintLevel::Warn) => brink_ir::Severity::Warning,
        None => base,
    };

    if candidate == brink_ir::Severity::Warning && lints.deny_warnings {
        brink_ir::Severity::Error
    } else {
        candidate
    }
}

/// `types = strict` + `dialect != brink` is a project-level config error —
/// there is no single offending span, so this reports once, attached to the
/// first file in the project (mirroring how a whole-project condition with
/// no natural per-construct site has to pick *some* file to carry it).
/// `None` when the project has no files at all (nothing to attach to) or the
/// dialect is already `brink` (no error).
#[must_use]
pub fn config_error(
    dialect: crate::Dialect,
    first_file: Option<FileId>,
) -> Option<brink_ir::Diagnostic> {
    if dialect == crate::Dialect::Brink {
        return None;
    }
    let file = first_file?;
    Some(brink_ir::Diagnostic {
        file,
        range: TextRange::new(0.into(), 0.into()),
        message: "types = strict requires dialect = brink — strict typing's annotation syntax \
                   is a brink-dialect extension (docs/typed-mode-spec.md §1); set \
                   `dialect = brink` or drop back to `types = gradual`"
            .to_owned(),
        code: brink_ir::DiagnosticCode::E064,
    })
}

/// The B0.9 native strict-only enforcement point (`docs/b0-sequencing.md`
/// §B0.9's "the strict-only ruling's enforcement point", issue #1342;
/// decision-log 2026-07-19 "Typing posture ruled": "the native surface is
/// strict-only — `types = strict` is a property of the dialect, not a
/// project knob; gradual typing does not exist on the native surface").
///
/// The inverse of [`config_error`] above in spirit — both are project-level
/// `types` config errors with no single offending span — but a different
/// axis: [`config_error`] rejects `types = strict` under the wrong
/// **dialect** (an ink-only concept); this rejects an explicit `types =
/// gradual` **knob** reaching a native (`.brink`) file, which has no
/// dialect at all (`Language::Native` is a separate, path-derived
/// classification — see `brink-db`'s `file_language` doc). Deliberately
/// keyed on the *explicit* `AnalysisOptions::types` field, never the
/// dialect-defaulted [`AnalysisOptions::type_policy`] result: a native
/// project that never touches the `types` knob resolves through the
/// ink-shaped `resolve_type_policy` default (which native's B0.10 dialect
/// wiring has not yet overridden) and must not be penalized for a default
/// it never chose — only a caller (CLI flag, `brink.toml`, editor/API call)
/// that explicitly dials `types = gradual` for a native file hits this.
///
/// `None` when `explicit_types` isn't `Some(TypePolicy::Gradual)` (unset, or
/// explicitly `Strict`) — the only two cases a native compile passes this
/// gate.
#[must_use]
pub fn native_strict_only_error(
    file: FileId,
    explicit_types: Option<TypePolicy>,
) -> Option<brink_ir::Diagnostic> {
    if explicit_types != Some(TypePolicy::Gradual) {
        return None;
    }
    Some(brink_ir::Diagnostic {
        file,
        range: TextRange::new(0.into(), 0.into()),
        message: "native `.brink` compiles are strict-only — `types = gradual` is not a valid \
                   policy for native source (docs/decision-log.md \"Typing posture ruled\", \
                   2026-07-19); drop the `types` setting (native strict is the only policy) or \
                   set `types = strict` explicitly"
            .to_owned(),
        code: brink_ir::DiagnosticCode::E137,
    })
}

/// The strict-mode diagnostics that need a full `InferenceResult`:
/// Unknown-escape (`E065`), Conflicted-escape (`E066`), void-assignment
/// (`E067`), and — the inherited #640-round ruling — auto-wiring
/// `annotations::mismatches` (`E063`) into production. Callers only reach
/// this once [`config_error`] has confirmed `dialect = brink`.
///
/// `resolutions`: the project's full resolution map — the void-assignment
/// pass needs it to resolve a call-site's `Path` back to the def it targets
/// (the same range→`DefinitionId` lookup `infer::body` builds its own
/// per-file projection of).
///
/// `manifest`: the registered host manifest (T1d-2, docs/t1d-spec.md §3) —
/// the `Handle<K>` annotation-firewall vocabulary source, threaded through
/// to [`check_escapes`] and `annotations::mismatches`. `None` degrades to an
/// empty handle-kind set, same posture as every other manifest-driven check.
#[must_use]
pub fn check(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
    resolutions: &ResolutionMap,
    manifest: Option<&brink_ir::HostManifest>,
) -> Vec<brink_ir::Diagnostic> {
    let mut out = check_escapes(files, index, inference, manifest);
    out.extend(annotations::mismatches(files, index, inference, manifest));
    out.extend(check_void_assignments(files, index, resolutions, inference));
    // T1c (docs/t1c-spec.md §4/§8): calls through function values are
    // statically checked under strict — the facts inference already
    // recorded map onto the existing TM-3 codes (E065/E066 escapes, E063
    // typed mismatches), never parallel ones.
    out.extend(check_value_calls(files, index, inference));
    // Issue #1864: a direct call's arguments, checked against the resolved
    // callee's already-known declared parameter types — the gap T1c's own
    // `check_value_calls` above deliberately never covered (that pass is
    // calls *through a value* specifically).
    out.extend(check_direct_call_args(files, index, inference));
    // Issue #1877 (the remainder of #1864 that PR #1875 left): a `~ temp`
    // initializer against its own ascription, and a plain assignment
    // against its target's already-known declared type — direct-call
    // arguments' sibling gap, same E063 machinery.
    out.extend(check_typed_assign_mismatches(files, index, inference));
    // Issue #1994 (RULED 2026-08-01, closing #1932): a lambda's own written
    // param/return annotation disagreeing with its body-derived type — an
    // eager `Error`-severity `E174`, deliberately not folded into the
    // gradual `E063` machinery above.
    out.extend(check_lambda_annotation_mismatches(files, index, inference));
    out.extend(check_global_initializers(files, index, manifest));
    // Issue #1532 (#1501 review, migration-tail finding 1): `remove`'s
    // pre-#1484 array leg has no compatibility shim — a statically-known
    // array receiver is caught here instead of only at the `MapRemove`
    // runtime fault.
    out.extend(check_array_remove_calls(files, index, inference));
    // Issue #1540 (second symptom): the UFCS spelling of that same check.
    // `infer::body::infer_call` types a multi-segment callee `Unknown`
    // before `infer_intrinsic` runs, so `arr.remove(0)` records no fact for
    // `check_array_remove_calls` to read — the B3a verdict table is where
    // the `(receiver type, verb)` pair survives. See `ufcs::check_strict`.
    out.extend(crate::ufcs::check_strict(
        files,
        index,
        resolutions,
        inference,
    ));
    // TM-4b (docs/typed-mode-spec.md §6): missing/extra/mistyped struct
    // construction-literal fields — strict-mode-only, per the crate doc.
    out.extend(crate::structs::check(files, index, inference, resolutions));
    // Issue #1900 (split from #1864/#1877): a *plain* dotted struct-field
    // assignment target (`~ p.x = expr`) checked against the field's
    // declared type — the E063 sibling of `check_typed_assign_mismatches`
    // above for a multi-segment assignment target, which that check's own
    // `check_declared_assign_target` explicitly declines.
    out.extend(crate::structs::check_assignments(files, index, inference));
    // T1e-1 (docs/t1e-spec.md §6, issue #831): a `ref lvalue-path`
    // projection's segments (dotted fields, `[…]` indices) checked against
    // the root's statically-known declared shape — strict-mode-only, same
    // rule `structs::check`'s own missing/extra/mistyped trio follows,
    // reusing the same shape table.
    out.extend(crate::ref_projection::check_strict(
        files,
        index,
        resolutions,
    ));
    // TM-3 completion (docs/typed-mode-spec.md §4, issue #659; extended to
    // variable/call/index-valued arguments by issue #983): `int(x)`/
    // `float(x)` statically out-of-domain arguments — strict-mode-only, per
    // `conversions`'s own module doc.
    out.extend(crate::conversions::check(
        files,
        index,
        inference,
        resolutions,
    ));
    // F27 (docs/stdlib-spec.md §1.6, ruled 2026-07-19, issue #1120):
    // condition-position `Option[T]` has no truthiness — strict-mode-only,
    // the compile-time half of the ruling (E116); the gradual-mode half is
    // the runtime `OptionTruthiness` fault, which also backstops every
    // statically-unclassifiable condition under strict.
    out.extend(crate::option_conditions::check(
        files,
        index,
        inference,
        resolutions,
    ));
    // NS-A5 (docs/stdlib-spec.md §7, F7/F8, issue #1111): the inhabited-
    // range refinement — `int(r)` demands `NonEmptyRange` evidence under
    // strict (E117); gradual is inert with the runtime-fault residual.
    // The template for every future value refinement.
    out.extend(crate::range_refinement::check(
        files,
        index,
        inference,
        resolutions,
    ));
    // B1 `or`-coalescing (docs/stdlib-spec.md §1.6a, issue #1460; review
    // finding on PR #1469): `infer::ty::coalesce`'s `LeftNotOption`/
    // `Mismatch` failures, surfaced at the coalescing expression's own site
    // — strict-mode-only, the compile-time half; gradual is inert with the
    // runtime `TypeError` fault as the (narrower) residual backstop.
    out.extend(crate::coalesce::check(files, index, inference, resolutions));
    // `contains(m, needle)` static key-domain warning (E152, issue #582,
    // companion to #580's ruling): a needle statically visible as outside
    // the int/string/bool key domain, against a receiver statically
    // visible as a map, always returns `false` at runtime — flagged at
    // compile time rather than left as a silent always-false membership
    // test. Strict-mode-only, same inference-substrate-backed domain-check
    // family as `conversions`/`range_refinement` above (see
    // `contains_domain`'s own module doc for why).
    out.extend(crate::contains_domain::check(
        files,
        index,
        inference,
        resolutions,
    ));
    out
}

/// Unknown-escape (`E065`) + Conflicted-escape (`E066`) over every inferable
/// def's params, return type, and temps. Return-value semantics — and so
/// the return-type escape check plus the fall-through check ([`E150`],
/// issue #1551) — apply to any def that is `is_function` (a `fn`) *or*
/// carries a declared, non-`void` return-type annotation (a value-returning
/// flow/stitch, the coroutine side of the ruled toggle,
/// `docs/decision-log.md` 2026-07-22 implicit-end ruling item 3); an
/// ordinary knot/stitch with neither has no return-value concept at all and
/// stays entirely unchecked.
///
/// [`E150`]: brink_ir::DiagnosticCode::E150
#[must_use]
fn check_escapes(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
    manifest: Option<&brink_ir::HostManifest>,
) -> Vec<brink_ir::Diagnostic> {
    let names = annotations::TypeNames::new(index, manifest);
    let mut out = Vec::new();
    for &(file, hir) in files {
        for knot in &hir.knots {
            let kind = knot.symbol_kind();
            if let Some(id) = annotations::def_id_for(index, file, kind, &knot.name.text) {
                // Issue #1591: "the body" for the return-value checks below
                // is the knot's own block *plus* every one of its stitches
                // — a stitch is reachable purely by fall-through when the
                // knot's own body is empty, so a value-returning `return`
                // living there counts the same as one in the knot's own
                // block. See [`has_value_return_over_stitches`].
                let body_has_value_return =
                    has_value_return_over_stitches(knot, id, file, index, inference);
                check_def(
                    id,
                    file,
                    &knot.name.text,
                    knot.name.range,
                    knot.is_function,
                    knot.return_type.as_ref(),
                    &knot.params,
                    &knot.body,
                    &names,
                    inference,
                    body_has_value_return,
                    &mut out,
                );
            }
            for stitch in &knot.stitches {
                let qualified = format!("{}.{}", knot.name.text, stitch.name.text);
                if let Some(id) =
                    annotations::def_id_for(index, file, SymbolKind::Stitch, &qualified)
                {
                    // `is_function` stays `false` (stitches never carry it,
                    // per `lower_native::container`'s module doc) — the
                    // real `return_type` (#1509) is forwarded regardless,
                    // since #1551 made `check_def`'s return-value checks
                    // (escape + fall-through) fire off a declared
                    // `return_type` too, not just `is_function`. A stitch
                    // has no nested stitches of its own (#1591's merge is
                    // one level, owned by the knot above), so its own
                    // `has_value_return` fact needs no merge here.
                    let body_has_value_return = inference
                        .bodies
                        .get(&id)
                        .is_some_and(|b| b.has_value_return);
                    check_def(
                        id,
                        file,
                        &qualified,
                        stitch.name.range,
                        false,
                        stitch.return_type.as_ref(),
                        &stitch.params,
                        &stitch.body,
                        &names,
                        inference,
                        body_has_value_return,
                        &mut out,
                    );
                }
            }
        }
    }
    out
}

/// Unknown-escape (`E065`) + Conflicted-escape (`E066`) over every
/// **registered** `EXTERNAL` declaration's own parameter types (issue #1004).
///
/// An `EXTERNAL name(params)` carries no ink-side type-annotation grammar
/// (its parameters are bare identifiers — `external_declaration` parses no
/// `(x: T)`), so a binding's declared parameter types can only come from the
/// host manifest (or an inline `///` `@param` doc). [`check`] above walks
/// `hir.knots` and therefore never sees these declarations; without this
/// pass a manifest whose `ManifestParam.ty` fails to resolve (an empty `ty`,
/// or one naming a semantic type absent from the `types` vocabulary) is
/// silently treated as an untyped call rather than the strict escape it is.
///
/// The signatures come from [`crate::collect_external_sigs`] — the *same*
/// resolution that seeds call-site argument checking into
/// `infer_project`/`solve_scc` — so a param typed by the manifest resolves to
/// its own [`Ty`] (a scalar semantic type as its `base`, a handle kind as
/// `Ty::Handle`) and stays clean, while one that resolves to [`Ty::Unknown`]
/// escapes. An `EXTERNAL` with *no* declared signature at all (neither a
/// manifest entry nor an inline doc) is absent from `external_sigs` and stays
/// entirely unchecked — the deliberate "unregistered external's call sites
/// stay unchecked" posture (see `collect_external_sigs`'s own doc), so this
/// never turns a bare, host-only `EXTERNAL` into a strict error.
///
/// Each diagnostic anchors at the external's *own* declaration span
/// (`SymbolInfo::range`), fixing the #1004 secondary defect where every
/// external escape collapsed onto one arbitrary line. Externals are visited
/// in `(file, declaration offset)` order — the same deterministic ordering
/// [`crate::external_check::analyze_externals`] uses — so diagnostic order is
/// source order, not `DefinitionId`-hash order.
#[must_use]
pub(crate) fn check_external_escapes(
    index: &SymbolIndex,
    external_sigs: &BTreeMap<DefinitionId, InferredSig>,
) -> Vec<brink_ir::Diagnostic> {
    // Resolve each signed external to its `SymbolInfo`, then order by
    // (file, declaration offset) for deterministic, source-ordered output.
    let mut externals: Vec<(&brink_ir::SymbolInfo, &InferredSig)> = external_sigs
        .iter()
        .filter_map(|(id, sig)| index.symbols.get(id).map(|info| (info, sig)))
        .filter(|(info, _)| info.kind == SymbolKind::External)
        .collect();
    externals.sort_by_key(|(info, _)| (info.file.0, info.range.start()));

    let mut out = Vec::new();
    for (info, sig) in externals {
        for (i, param) in info.params.iter().enumerate() {
            let ty = sig.params.get(i).unwrap_or(&Ty::Unknown);
            emit_escape(
                info.file,
                &info.name,
                &format!("parameter `{}`", param.name),
                info.range,
                ty,
                // No inline-annotation exemption exists for an `EXTERNAL`
                // (bare-identifier params): the resolved manifest/doc type
                // *is* the declared type, so an `Unknown` here is a genuine
                // "no resolvable type" escape, not a merely-uninferred slot.
                false,
                &mut out,
            );
        }
    }
    out
}

/// Whether **"the body"** of `knot` — for the `E150` fall-through check and
/// `E067` inferred-void classification — ever carries a value-returning
/// `return <expr>` anywhere: its own block **plus every one of its
/// stitches** (`docs/typed-mode-spec.md` §3 ruling, issue #1591). A stitch
/// is reachable from its owning knot purely by fall-through when the
/// knot's own body is empty — no explicit divert required — or by an
/// explicit divert; either way it is a continuation of the *same
/// definition's* execution, not a separate callable, so a value-returning
/// `return` anywhere in a stitch counts exactly like one in the knot's own
/// block.
///
/// This reading does **not** extend to the `E065`/`E066` return-type
/// escape check in [`check_def`]: that check reads `sig.return_ty`, which
/// is inferred per-def and is never merged over stitches (only the
/// has-value-return *fact* is merged here), so the escape branch keeps
/// reading the def's own body (`body_types.has_value_return`) rather than
/// this merged value — merging it there would make an inferred `Unknown`
/// on the knot's own signature look "proven" by a sibling stitch's return,
/// which is a different def with its own signature.
///
/// `Stitch` has no nested stitches in the HIR (`hir::types::Stitch` carries
/// no `stitches` field), so this is exactly one level of merge, never
/// recursive.
///
/// This is the **one** has-value-return-over-stitches reading shared by
/// [`check_def`]'s `E150` fall-through check (called once per knot below)
/// and [`collect_void_defs`]'s (`E067`) inferred-void classification.
/// Issue #1551 fixed the E065/E066 + E150 checks' `is_function`-only
/// gating; #1054/PR #1585 fixed `collect_void_defs`'s own copy of this
/// exact stitch-merge read; #1591 is the E150 path's turn, done here by
/// sharing instead of adding a fourth copy.
fn has_value_return_over_stitches(
    knot: &brink_ir::Knot,
    own_id: DefinitionId,
    file: FileId,
    index: &SymbolIndex,
    inference: &InferenceResult,
) -> bool {
    let own = inference
        .bodies
        .get(&own_id)
        .is_some_and(|b| b.has_value_return);
    own || knot.stitches.iter().any(|st| {
        annotations::def_id_for(
            index,
            file,
            SymbolKind::Stitch,
            &format!("{}.{}", knot.name.text, st.name.text),
        )
        .and_then(|sid| inference.bodies.get(&sid))
        .is_some_and(|b| b.has_value_return)
    })
}

#[expect(clippy::too_many_arguments, reason = "internal helper, not public API")]
fn check_def(
    id: DefinitionId,
    file: FileId,
    def_label: &str,
    name_range: TextRange,
    is_function: bool,
    return_type: Option<&TypeExpr>,
    params: &[brink_ir::Param],
    body: &Block,
    names: &annotations::TypeNames,
    inference: &InferenceResult,
    // The has-value-return fact for the `E150` fall-through check below,
    // read over "the body" as issue #1591 defines it (the caller's job —
    // see [`has_value_return_over_stitches`] — since only the caller knows
    // whether `id` is a knot with stitches to merge in or a stitch, which
    // is always a leaf). The `E065`/`E066` return-type escape check does
    // *not* use this merged fact — it reads `body_types.has_value_return`
    // directly, since the signature it's checking is per-def and is never
    // merged over stitches.
    body_has_value_return: bool,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    let Some(sig) = inference.signatures.get(&id) else {
        return;
    };
    let Some(body_types) = inference.bodies.get(&id) else {
        return;
    };

    // Params: an explicit, resolvable annotation supplies the concrete type
    // — TM-2's firewall — and exempts the slot from Unknown-escape *only*.
    // It never exempts Conflicted-escape: the body's own uses genuinely
    // disagree with each other, which no annotation can resolve (mirrors
    // `annotations::mismatches`' `is_unresolved()` treatment of Conflicted).
    for (i, p) in params.iter().enumerate() {
        let annotated = p
            .annotation
            .as_ref()
            .is_some_and(|ann| annotations::resolve(ann, names).is_some());
        let ty = sig.params.get(i).unwrap_or(&Ty::Unknown);
        emit_escape(
            file,
            def_label,
            &format!("parameter `{}`", p.name.text),
            p.name.range,
            ty,
            annotated,
            out,
        );
    }

    // Return type: return-value semantics apply to a `fn` (`is_function`)
    // *or* to any def carrying a declared, non-`void` return-type
    // annotation (issue #1551 — a value-returning flow/stitch is the
    // coroutine side of the ruled toggle, `docs/decision-log.md`
    // 2026-07-22 implicit-end ruling item 3: "no return type ⇒ ends
    // implicitly as DONE; has one ⇒ must return"). A `void`-annotated def
    // never needs a concrete return value either way — `void` reads as "no
    // return type" for this purpose on both a `fn` and a flow/stitch.
    let has_void_annotation =
        return_type.is_some_and(|rt| matches!(rt, TypeExpr::Named { name, .. } if name == "void"));
    let declares_return_value = return_type.is_some() && !has_void_annotation;
    if is_function || declares_return_value {
        // Issue #1028 (originally `is_function`-only) / #1551 (generalized
        // to any declared-return-value def): a body that never carries a
        // value-returning `return <expr>` — it either falls off the end or
        // only ever bare-`return`s — proves nothing ever flows out of it.
        // `sig.return_ty.is_unknown()` alone can't distinguish "never
        // returns a value" from "returns a value inference couldn't pin
        // down" (a genuine Unknown-escape, handled in the `else` below).
        //
        // The two branches below read *different* has-value-return facts
        // on purpose. The `else if` (E150 fall-through) reads
        // `body_has_value_return`, the merged fact passed in by the caller
        // (issue #1591: over the def's own block *plus* its stitches — see
        // [`has_value_return_over_stitches`]) — a value-returning `return`
        // reached purely by fall-through into a stitch still proves the
        // *flow* returns a value. The Unknown-escape branch immediately
        // below reads `body_types.has_value_return` — this def's own body
        // only — because it's checking `sig.return_ty`, which inference
        // computes per-def and never merges over stitches; merging the
        // fact there without merging the signature would let a sibling
        // stitch's return "prove" this knot's own Unknown return type,
        // which is a different def with a different (still-Unknown)
        // signature.
        //
        // What "never returns a value" *means* differs by whether a return
        // value was promised:
        //   - No declared return type (bare `fn`, typed-mode-spec §3 is
        //     silent on this shape): inference shouldn't demand an
        //     annotation to say what the body already proves — reads as
        //     `void`, same as an explicit `: void` annotation. Nothing to
        //     report.
        //   - A declared, non-`void` return type: the author promised a
        //     value every path must supply. Falling through is the ruled
        //     **checker error** (`E150`, decision-log 2026-07-22 item 3),
        //     never a silent implicit `void` — and never satisfied by an
        //     implicit `-> DONE` synthesized at HIR lowering (`DONE` ends
        //     the turn, not the value contract). This also fixes a latent
        //     gap in the *pre-existing* `is_function` case: an annotated
        //     `fn f(): int { … }` with no `return` anywhere previously
        //     inferred `is_void = true` via the old blanket
        //     `!has_value_return` short-circuit and skipped checking
        //     entirely — silent despite the declared `int`.
        // `!has_void_annotation` here matters even though `has_value_return`
        // alone looks sufficient: a `: void`-annotated def whose body does
        // carry a value-returning `return <expr>` (a body/annotation
        // mismatch, not an escape) must not run the Unknown-escape check —
        // `void` reads as "no return type" for escape purposes on both
        // branches, so it also can't trip `E150` in the `else` below.
        if body_types.has_value_return && !has_void_annotation {
            let annotated = return_type.is_some_and(|rt| annotations::resolve(rt, names).is_some());
            emit_escape(
                file,
                def_label,
                "return type",
                name_range,
                &sig.return_ty,
                annotated,
                out,
            );
        } else if declares_return_value && !body_has_value_return {
            out.push(brink_ir::Diagnostic {
                file,
                range: name_range,
                message: format!(
                    "`{def_label}` declares a return type but its body never returns a value"
                ),
                code: brink_ir::DiagnosticCode::E150,
            });
        }
    }

    // Temps: an explicit ascription (`~ temp x: T = ...`) exempts the slot
    // the same way a param annotation does (Unknown-escape only, per above).
    let param_names: std::collections::BTreeSet<&str> =
        params.iter().map(|p| p.name.text.as_str()).collect();
    let temp_decls = collect_temps(body, names);
    for (name, ty) in &body_types.locals {
        if param_names.contains(name.as_str()) {
            continue; // already checked above, positionally + annotation-aware
        }
        let decl = temp_decls.get(name);
        let annotated = decl.is_some_and(|d| d.annotation_ty.is_some());
        let range = decl.map_or(name_range, |d| d.range);
        emit_escape(
            file,
            def_label,
            &format!("temp `{name}`"),
            range,
            ty,
            annotated,
            out,
        );
    }

    // Issue #1770: give every lambda literal anywhere in this body the same
    // Unknown-escape (`E065`) / Conflicted-escape (`E066`) treatment the
    // params/temps loops above just gave `def_label` itself. Each
    // `body_types.lambda_escapes` entry is already a fully-built
    // `emit_escape` input (final type, declaration range,
    // annotation-exemption bit, ready-made slot label) — recorded
    // unconditionally by `infer::body::InferPass::infer_lambda` for every
    // lambda anywhere in this body, including one nested inside another
    // lambda's own body (see that field's own doc) — so this is a flat
    // re-emit under the enclosing def's own label, no per-lambda grouping
    // or lookup needed.
    for slot in &body_types.lambda_escapes {
        emit_escape(
            file,
            def_label,
            &slot.slot_label,
            slot.range,
            &slot.ty,
            slot.annotated,
            out,
        );
    }
}

/// `annotated`: whether an explicit, resolvable annotation/ascription is
/// present for this slot — exempts an `Unknown` classification (the
/// annotation supplies the type TM-1 alone couldn't pin down) but never a
/// `Conflicted` one (a genuine body-internal contradiction, which no
/// annotation heals).
fn emit_escape(
    file: FileId,
    def_label: &str,
    slot_label: &str,
    range: TextRange,
    ty: &Ty,
    annotated: bool,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    match classify(ty) {
        Escape::Clean => {}
        Escape::Unknown if annotated => {}
        Escape::Unknown => out.push(brink_ir::Diagnostic {
            file,
            range,
            message: format!(
                "`{def_label}`'s {slot_label} escapes strict inference as Unknown — \
                 annotate or restructure"
            ),
            code: brink_ir::DiagnosticCode::E065,
        }),
        Escape::Conflicted => out.push(brink_ir::Diagnostic {
            file,
            range,
            message: format!(
                "`{def_label}`'s {slot_label} is Conflicted under strict types — its uses \
                 disagree on its type (observed as `{}`)",
                ty.display()
            ),
            code: brink_ir::DiagnosticCode::E066,
        }),
    }
}

enum Escape {
    Clean,
    Unknown,
    Conflicted,
}

/// Recursively classify a type as clean, an Unknown-escape, or a
/// Conflicted-escape — `Conflicted` wins whenever both appear inside the
/// same `Array`/`Map` nesting (it is the stronger diagnosis: a genuine
/// contradiction, not merely an unconstrained slot).
fn classify(ty: &Ty) -> Escape {
    match ty {
        Ty::Conflicted => Escape::Conflicted,
        Ty::Unknown => Escape::Unknown,
        // Array, (NS-A1) `Option[T]`, and (NS-A7) `Weighted[T]` recurse on
        // their single element — a parameterized builtin whose element is
        // Unknown/Conflicted escapes like any other nesting.
        Ty::Array(elem) | Ty::Option(elem) | Ty::Weighted(elem) => classify(elem),
        Ty::Map(k, v) => match (classify(k), classify(v)) {
            (Escape::Conflicted, _) | (_, Escape::Conflicted) => Escape::Conflicted,
            (Escape::Unknown, _) | (_, Escape::Unknown) => Escape::Unknown,
            (Escape::Clean, Escape::Clean) => Escape::Clean,
        },
        // T1c `fn(T…): R` (docs/t1c-spec.md §4): the same recursive lattice
        // walk as Array/Map — a fn value whose row carries Unknown or
        // Conflicted slots can't be call-checked, so it escapes like any
        // other nesting. (In practice the row comes from the target's own
        // inferred signature, so the target def carries the root-cause
        // E065/E066 too.)
        Ty::Fn(params, ret, _) => {
            params
                .iter()
                .chain(std::iter::once(ret.as_ref()))
                .fold(Escape::Clean, |acc, t| match (acc, classify(t)) {
                    (Escape::Conflicted, _) | (_, Escape::Conflicted) => Escape::Conflicted,
                    (Escape::Unknown, _) | (_, Escape::Unknown) => Escape::Unknown,
                    (Escape::Clean, Escape::Clean) => Escape::Clean,
                })
        }
        // TM-4b (docs/typed-mode-spec.md §6): "struct-typed slots are
        // concrete for E065/E066 purposes" — a resolved `Ty::Struct` is as
        // clean as any other nominal (`Ty::List`'s existing precedent).
        // T1d-2 (docs/t1d-spec.md §3): a resolved `Ty::Handle` is equally
        // concrete — reusing TM-3's existing E065/E066 vocabulary is exactly
        // the spec's "strict kind-checking via existing TM-3 machinery, no
        // new codes" ruling. A *cross-kind* mismatch never reaches this
        // function as `Ty::Handle` at all — `unify` already folds it to
        // `Ty::Conflicted` at the point the two kinds meet, so it's caught
        // by the `Ty::Conflicted` arm above, not here.
        // NS-A5: a resolved `Ty::Range` is concrete either way — the
        // `non_empty` refinement bit is evidence, not openness; a missing
        // refinement is E117's business (range_refinement), never an
        // Unknown-escape.
        // (NS-A8 tower kinds are concrete leaves — clean, like scalars.)
        // A resolved `Ty::Content` (issue #1846) is equally concrete —
        // fragment-backed, not an openness axis; strict escape-checking
        // treats it exactly like any other nominal leaf.
        Ty::Int
        | Ty::Float
        | Ty::Bool
        | Ty::String
        | Ty::Content
        | Ty::Divert
        | Ty::List(_)
        | Ty::Struct(_)
        | Ty::Handle(_)
        | Ty::Range { .. }
        | Ty::Tower(_) => Escape::Clean,
    }
}

// ── T1c: calls through function values (docs/t1c-spec.md §4/§8) ───────

/// Report every [`crate::infer::ValueCallFact`] inference recorded, per
/// def, using the existing TM-3 vocabulary:
///
/// - `Unknown` callee → `E065` (the escape rule applied to call position:
///   "a strict-mode author can never reach the §3 runtime fault");
/// - `Conflicted` callee → `E066`;
/// - known-type mismatches (non-callable type, arity, argument type) →
///   `E063` (typed-mismatch reporting extended to call-through-value
///   sites — `Error` under strict via [`effective_severity`]).
fn check_value_calls(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
) -> Vec<brink_ir::Diagnostic> {
    use crate::infer::ValueCallKind;

    let mut out = Vec::new();
    for &(file, hir) in files {
        let mut def_ids: Vec<DefinitionId> = Vec::new();
        for knot in &hir.knots {
            let kind = knot.symbol_kind();
            if let Some(id) = annotations::def_id_for(index, file, kind, &knot.name.text) {
                def_ids.push(id);
            }
            for stitch in &knot.stitches {
                let qualified = format!("{}.{}", knot.name.text, stitch.name.text);
                if let Some(id) =
                    annotations::def_id_for(index, file, SymbolKind::Stitch, &qualified)
                {
                    def_ids.push(id);
                }
            }
        }
        for id in def_ids {
            let Some(body) = inference.bodies.get(&id) else {
                continue;
            };
            for fact in &body.value_calls {
                let callee = &fact.callee;
                let (message, code) = match &fact.kind {
                    ValueCallKind::UnknownCallee => (
                        format!(
                            "`{callee}` is called as a function value but its type escapes \
                             strict inference as Unknown — annotate (`fn(T…): R`) or \
                             restructure"
                        ),
                        brink_ir::DiagnosticCode::E065,
                    ),
                    ValueCallKind::ConflictedCallee => (
                        format!(
                            "`{callee}` is called as a function value but its type is \
                             Conflicted under strict types — its uses disagree"
                        ),
                        brink_ir::DiagnosticCode::E066,
                    ),
                    ValueCallKind::NotCallable(ty) => (
                        format!(
                            "`{callee}` has type `{}` — not callable (a `fn(T…): R` \
                             function value is required in call position)",
                            ty.display()
                        ),
                        brink_ir::DiagnosticCode::E063,
                    ),
                    ValueCallKind::ArityMismatch { expected, got } => (
                        format!(
                            "call through `{callee}` supplies {got} argument(s) but its \
                             known type expects {expected}"
                        ),
                        brink_ir::DiagnosticCode::E063,
                    ),
                    ValueCallKind::ArgMismatch {
                        index,
                        expected,
                        found,
                    } => (
                        format!(
                            "argument {} of call through `{callee}` has type `{}` but its \
                             known type expects `{}`",
                            index + 1,
                            found.display(),
                            expected.display()
                        ),
                        brink_ir::DiagnosticCode::E063,
                    ),
                    ValueCallKind::OverBind { available, got } => (
                        format!(
                            "`bind` through `{callee}` supplies {got} argument(s) but only \
                             {available} parameter(s) remain in its known type"
                        ),
                        brink_ir::DiagnosticCode::E063,
                    ),
                };
                out.push(brink_ir::Diagnostic {
                    file,
                    range: fact.range,
                    message,
                    code,
                });
            }
        }
    }
    out
}

// ── Direct-call + `#fn` creation-site argument types (issues #1864, #2001) ──

/// Report every [`crate::infer::DirectCallArgMismatch`] inference recorded,
/// per def, as `E063` — the same typed-mismatch code
/// [`check_value_calls`]'s own `ArgMismatch` arm reports for a call
/// *through a value*; a direct call resolving straight to a known def via
/// `known_sigs` is the same "declared type disagrees with what a caller
/// passed" fact, just without a value in between (docs/t1c-spec.md §8's
/// "existing TM-3 machinery, no new codes" posture, applied to the direct-
/// call case #1864 identified as unchecked). Same shape as
/// [`check_value_calls`]: walk every inferable def, read its recorded
/// facts, map each straight onto one diagnostic.
///
/// As of #2001, [`crate::infer::DirectCallArgMismatch`] also carries facts
/// from a second producer that is not a call at all: a `#fn(target, args…)`
/// literal's bound-argument list, which is the by-ref *creation* site for a
/// partial application (see that struct's own doc). As of #2127, a third
/// producer joins them: a divert-with-arguments (`-> knot(a, b)`) `ref`
/// position. All three map onto the same `E063` message ("argument N of
/// call to `name`") — accepted as close enough for the creation-site and
/// divert-target cases too rather than adding a call-vs-creation-vs-divert
/// discriminant; see [`crate::infer::DirectCallArgMismatch`] for that call.
///
/// `infer::body::InferPass::arg_is_observed_local` already excludes an
/// argument `InferPass::observe` itself would join `param_ty` into, so
/// every fact reaching here is disjoint from `check_escapes`'s own
/// Conflicted-escape (`E066`) reporting for the same call/creation site —
/// no dedup needed on this side.
fn check_direct_call_args(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
) -> Vec<brink_ir::Diagnostic> {
    let mut out = Vec::new();
    for &(file, hir) in files {
        let mut def_ids: Vec<DefinitionId> = Vec::new();
        // Issue #1903: add root_content's synthetic ID to the list of defs
        // to check, just as collect_defs synthesizes it for inference.
        // Mirrors check_typed_assign_mismatches below — without this, a
        // direct call (or #fn literal) at the top level of an ink file's
        // root_content silently drops its recorded facts (2026-08 review,
        // issue #2001).
        if !hir.root_content.stmts.is_empty() {
            let synthetic_id = DefinitionId::new(DefinitionTag::LocalVar, u64::from(file.0));
            def_ids.push(synthetic_id);
        }
        for knot in &hir.knots {
            let kind = knot.symbol_kind();
            if let Some(id) = annotations::def_id_for(index, file, kind, &knot.name.text) {
                def_ids.push(id);
            }
            for stitch in &knot.stitches {
                let qualified = format!("{}.{}", knot.name.text, stitch.name.text);
                if let Some(id) =
                    annotations::def_id_for(index, file, SymbolKind::Stitch, &qualified)
                {
                    def_ids.push(id);
                }
            }
        }
        for id in def_ids {
            let Some(body) = inference.bodies.get(&id) else {
                continue;
            };
            for fact in &body.direct_call_arg_mismatches {
                out.push(brink_ir::Diagnostic {
                    file,
                    range: fact.range,
                    message: format!(
                        "argument {} of call to `{}` has type `{}` but its known \
                         type expects `{}`",
                        fact.index + 1,
                        fact.callee,
                        fact.found.display(),
                        fact.expected.display()
                    ),
                    code: brink_ir::DiagnosticCode::E063,
                });
            }
        }
    }
    out
}

// ── `~ temp` initializers + plain assignments (issue #1877) ───────────

/// Report every [`crate::infer::TypedAssignMismatch`] inference recorded,
/// per def, as `E063` — the same typed-mismatch code
/// [`check_direct_call_args`] reports for a direct call's arguments. Issue
/// #1877 is the remainder of #1864 that PR #1875 explicitly left: that PR
/// checked direct-call arguments against a callee's declared param types;
/// this checks a `~ temp name: T = expr` initializer against its own
/// ascription, and a plain `~ name = expr` assignment against the target's
/// already-known declared type (a VAR/CONST's declaration-derived type, or
/// an annotated Param/Temp's ascription). Same shape as
/// [`check_direct_call_args`]: walk every inferable def, read its recorded
/// facts, map each straight onto one diagnostic.
///
/// `infer::body::InferPass::check_declared_assign_target` and
/// `check_declared_temp_init` each exclude a Temp write whose own `observe`/
/// `bind_local` join is about to drive it to `Ty::Conflicted` *on that exact
/// write*; `infer::body::InferPass::
/// drop_typed_assign_mismatches_conflicted_by_a_later_read` (run post-walk,
/// from `infer_def_body` via `InferPass::finish_walk`) additionally drops
/// any fact whose target's *final* whole-body type ends up `Conflicted` —
/// the guard is per-write and order-sensitive, so a later read of the same
/// local (not just the write that produced the fact) can also conflict it,
/// and only the post-walk pass sees that. Between the two, every fact
/// reaching here is disjoint from `check_escapes`'s own Conflicted-escape
/// (`E066`) reporting for the same local, no dedup needed
/// on this side (mirrors [`check_direct_call_args`]'s own doc on the
/// identical point).
/// Every top-level def id a body-level check (typed-assign mismatches,
/// lambda annotation mismatches) needs to walk for one file: each
/// knot/stitch, plus (issue #1903) `root_content`'s own synthetic id when
/// the file has top-level content of its own — `collect_defs` synthesizes
/// that same id for inference, so a body-level check must look it up under
/// the identical scheme or it silently never sees a lambda/assignment
/// written directly in a file's top-level content. Factored out of
/// [`check_typed_assign_mismatches`] and [`check_lambda_annotation_mismatches`]
/// (previously a character-for-character copy in each, house rule on
/// keeping a single walk shared once it needs a second issue-specific fix
/// threaded through it) so the next such fix only has to land once.
fn body_def_ids(file: FileId, hir: &HirFile, index: &SymbolIndex) -> Vec<DefinitionId> {
    let mut def_ids: Vec<DefinitionId> = Vec::new();
    if !hir.root_content.stmts.is_empty() {
        let synthetic_id = DefinitionId::new(DefinitionTag::LocalVar, u64::from(file.0));
        def_ids.push(synthetic_id);
    }
    for knot in &hir.knots {
        let kind = knot.symbol_kind();
        if let Some(id) = annotations::def_id_for(index, file, kind, &knot.name.text) {
            def_ids.push(id);
        }
        for stitch in &knot.stitches {
            let qualified = format!("{}.{}", knot.name.text, stitch.name.text);
            if let Some(id) = annotations::def_id_for(index, file, SymbolKind::Stitch, &qualified) {
                def_ids.push(id);
            }
        }
    }
    def_ids
}

fn check_typed_assign_mismatches(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
) -> Vec<brink_ir::Diagnostic> {
    let mut out = Vec::new();
    for &(file, hir) in files {
        for id in body_def_ids(file, hir, index) {
            let Some(body) = inference.bodies.get(&id) else {
                continue;
            };
            for fact in &body.typed_assign_mismatches {
                out.push(brink_ir::Diagnostic {
                    file,
                    range: fact.range,
                    message: format!(
                        "`{}` has type `{}` but its declared type is `{}`",
                        fact.target,
                        fact.found.display(),
                        fact.expected.display()
                    ),
                    code: brink_ir::DiagnosticCode::E063,
                });
            }
        }
    }
    out
}

/// Report every [`crate::infer::LambdaAnnotationMismatch`] inference
/// recorded, per def, as `E174` (issue #1994, RULED 2026-08-01, closing
/// #1932). Same walk shape as [`check_typed_assign_mismatches`] above (every
/// fact was harvested onto whichever top-level def's own `BodyResult` the
/// lambda that produced it was nested inside — `infer::body::InferPass`
/// never snapshots this accumulator around a lambda frame, see that
/// struct's own field doc), but a materially different severity posture:
/// unlike `E063`'s gradual/advisory "two independent derivations, compared
/// but never merged", a lambda's own written annotation now *replaces* its
/// body-derived type at this slot (`infer::body::InferPass::infer_lambda`'s
/// own doc), so a disagreement recorded here is never merely a warning —
/// `E174`'s default severity is `Error`, not downgradable the way `E063` is.
fn check_lambda_annotation_mismatches(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
) -> Vec<brink_ir::Diagnostic> {
    let mut out = Vec::new();
    for &(file, hir) in files {
        for id in body_def_ids(file, hir, index) {
            let Some(body) = inference.bodies.get(&id) else {
                continue;
            };
            for fact in &body.lambda_annotation_mismatches {
                let message = match &fact.param_name {
                    Some(name) => format!(
                        "lambda parameter `{name}` is annotated `{}` but its body infers `{}`",
                        fact.expected.display(),
                        fact.found.display()
                    ),
                    None => format!(
                        "lambda return type is annotated `{}` but its body infers `{}`",
                        fact.expected.display(),
                        fact.found.display()
                    ),
                };
                out.push(brink_ir::Diagnostic {
                    file,
                    range: fact.range,
                    message,
                    code: brink_ir::DiagnosticCode::E174,
                });
            }
        }
    }
    out
}

// ── VAR/CONST declaration initializers (issue #1877) ──────────────────

/// Report a VAR/CONST declaration whose explicit `: type` annotation
/// disagrees with its own initializer literal's independently-inferred
/// type, as `E063` — the declaration-initializer sibling of
/// [`check_typed_assign_mismatches`] above, for the one declaration shape
/// that has no enclosing body to walk (`hir.variables`/`hir.constants` are
/// file-level, not per-def facts inference records).
///
/// TM-2's firewall (`signature::declared_value_ty`'s own doc: "annotation
/// *replaces* [the initializer-inferred type]") means `Sig::value_ty` for an
/// annotated VAR/CONST is the annotation alone — the initializer's own
/// independently-inferred type is computed and then silently discarded,
/// never compared against it. This is that comparison, reusing
/// `signature::literal_ty` (the same collection-aware literal-typing
/// `Sig::value_ty`'s own fallback branch already calls) rather than
/// re-deriving it.
///
/// Declaration-derived only, like the rest of `signature.rs`: a non-literal
/// initializer (a call, an index, a reference to another global, `#fn(…)`)
/// has no `literal_ty` here and is silently unchecked — the runtime
/// type-mismatch fault (gradual) or a body-inference-driven check (were one
/// to exist for globals — TM-3's module doc already notes cross-
/// reassignment detection for globals is out of scope) is the backstop, not
/// this stub.
fn check_global_initializers(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    manifest: Option<&brink_ir::HostManifest>,
) -> Vec<brink_ir::Diagnostic> {
    let names = annotations::TypeNames::new(index, manifest);
    let mut out = Vec::new();
    for &(file, hir) in files {
        for v in &hir.variables {
            check_one_global_initializer(
                &v.name.text,
                &v.value,
                v.annotation.as_ref(),
                file,
                index,
                &names,
                &mut out,
            );
        }
        for c in &hir.constants {
            check_one_global_initializer(
                &c.name.text,
                &c.value,
                c.annotation.as_ref(),
                file,
                index,
                &names,
                &mut out,
            );
        }
    }
    out
}

fn check_one_global_initializer(
    name: &str,
    value: &Expr,
    annotation: Option<&TypeExpr>,
    file: FileId,
    index: &SymbolIndex,
    names: &annotations::TypeNames,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    let Some(te) = annotation else { return };
    let Some(ann_ty) = annotations::resolve(te, names) else {
        return;
    };
    let Some(lit_ty) = crate::signature::literal_ty(value, index) else {
        return;
    };
    if lit_ty.is_unresolved() || crate::infer::assignable(&ann_ty, &lit_ty) {
        return;
    }
    out.push(brink_ir::Diagnostic {
        file,
        range: te.range(),
        message: format!(
            "`{name}`'s declared type `{}` disagrees with its initializer's type (`{}`)",
            ann_ty.display(),
            lit_ty.display()
        ),
        code: brink_ir::DiagnosticCode::E063,
    });
}

// ── `remove`/`remove_at` migration tail (issue #1532, `E149`) ─────────

/// Report every [`crate::infer::body`]-recorded array-typed `remove(a, i)`
/// call site (`BodyResult::array_remove_calls`, threaded through
/// [`crate::infer::BodyTypes`]) as `E149`. Same shape as
/// [`check_value_calls`] — walk every inferable def, read its recorded
/// facts, map each straight onto one diagnostic — but the fact carries no
/// per-call detail to interpolate (the message is fixed), so there is no
/// `match` here.
fn check_array_remove_calls(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
) -> Vec<brink_ir::Diagnostic> {
    let mut out = Vec::new();
    for &(file, hir) in files {
        let mut def_ids: Vec<DefinitionId> = Vec::new();
        for knot in &hir.knots {
            let kind = knot.symbol_kind();
            if let Some(id) = annotations::def_id_for(index, file, kind, &knot.name.text) {
                def_ids.push(id);
            }
            for stitch in &knot.stitches {
                let qualified = format!("{}.{}", knot.name.text, stitch.name.text);
                if let Some(id) =
                    annotations::def_id_for(index, file, SymbolKind::Stitch, &qualified)
                {
                    def_ids.push(id);
                }
            }
        }
        for id in def_ids {
            let Some(body) = inference.bodies.get(&id) else {
                continue;
            };
            for &range in &body.array_remove_calls {
                out.push(brink_ir::Diagnostic {
                    file,
                    range,
                    message: brink_ir::DiagnosticCode::E149.title().to_owned(),
                    code: brink_ir::DiagnosticCode::E149,
                });
            }
        }
    }
    out
}

// ── Void-assignment (E067, docs/typed-mode-spec.md §3) ────────────────

/// `(start, end)` `u32` pair — `TextRange` has no `Ord` impl, so every
/// `BTreeMap` keyed by a source range in this module uses this instead
/// (mirrors `infer::mod`'s own `range_key`).
fn range_key(range: TextRange) -> (u32, u32) {
    (range.start().into(), range.end().into())
}

/// `~ x = f()` / `~ temp x = f()` where `f`'s resolved def is a `void`-
/// returning function — whether by an explicit `): void ===` annotation or
/// by inference (issue #1054: a `fn` with no declared return type whose
/// body never carries a value-returning `return`, the same inferred-void
/// shape #1046 taught the return-type escape check to recognize) — is a
/// compile error under strict (spec §3: "assigning a `void` call is an
/// error in strict mode"). Only the assignment/temp-decl's RHS *root*
/// expression is checked — a statement-position call (`~ f()`) or a call
/// nested inside interpolation is never flagged, since neither assigns the
/// (nonexistent) result anywhere.
#[must_use]
fn check_void_assignments(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    inference: &InferenceResult,
) -> Vec<brink_ir::Diagnostic> {
    let void_defs = collect_void_defs(files, index, inference);
    if void_defs.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for &(file, hir) in files {
        let resolution_by_range = resolution_index(resolutions, file);
        for knot in &hir.knots {
            check_void_block(file, &knot.body, &void_defs, &resolution_by_range, &mut out);
            for stitch in &knot.stitches {
                check_void_block(
                    file,
                    &stitch.body,
                    &void_defs,
                    &resolution_by_range,
                    &mut out,
                );
            }
        }
    }
    out
}

/// Every function knot that is `void`, by `DefinitionId` — either by an
/// explicit `): void ===` return annotation, or by inference (issue #1054):
/// a `fn` with *no* declared return type whose body never carries a
/// value-returning `return <expr>` reads as void the same way `check_def`'s
/// return-type escape check already treats it (issue #1046's "an
/// unannotated, never-returning function infers void" ruling — this is that
/// same fact, read here for the void-*assignment* check rather than the
/// escape check). A declared, *non*-`void` return type whose body never
/// returns a value is a different shape entirely — the checker error
/// `E150` (issue #1551), not void — so it is deliberately excluded here:
/// `knot.return_type.is_none()` gates the inferred branch, meaning only a
/// bare `fn` with no annotation at all can infer void.
///
/// Only `is_function` knots are function calls in the sense this check
/// cares about (a value-returning *non-function* flow/stitch is the
/// coroutine side of the NG-C/#1509 toggle, not a callable void-or-not
/// function) — so only `hir.knots` entries with `is_function` set are
/// candidates, mirroring `check_escapes`' own def-id lookup (`kind` tracks
/// `knot.ptr`, since a top-level stitch promoted to knot status is indexed
/// under `SymbolKind::Stitch`, #626). A *nested* `Stitch` never carries
/// `is_function` (no HIR container below `Knot` does), so it is never a
/// candidate here regardless of its own `return_type` (#1509).
///
/// The inferred branch's "never carries a value-returning `return`" check
/// must also account for the function's own stitches: a fall-through
/// `-> f.sub` (or a conditional divert into one) reaches a stitch's body,
/// which is a *separate* `Def` (`infer::collect_defs`, qualified name
/// `f.sub`, `SymbolKind::Stitch`) with its own `BodyTypes` in
/// `inference.bodies` — the knot's own `BodyTypes` only covers content
/// before the first stitch. A knot is only inferred-void when neither its
/// own body nor any of its stitches carries a value-returning `return` —
/// [`has_value_return_over_stitches`] is that shared reading (issue #1591).
fn collect_void_defs(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
) -> BTreeSet<DefinitionId> {
    let mut out = BTreeSet::new();
    for &(file, hir) in files {
        for knot in &hir.knots {
            if !knot.is_function {
                continue;
            }
            let kind = knot.symbol_kind();
            let Some(id) = annotations::def_id_for(index, file, kind, &knot.name.text) else {
                continue;
            };
            let has_void_annotation = knot
                .return_type
                .as_ref()
                .is_some_and(|rt| matches!(rt, TypeExpr::Named { name, .. } if name == "void"));
            // `has_value_return_over_stitches` reads `false` for a def with
            // no `inference.bodies` entry at all, same as it reads `false`
            // for a body that has one but never returns a value — the two
            // are different facts ("never inferred" vs. "inferred, no
            // value return") and only the latter should count as
            // inferred-void (mirrors the pre-dedupe
            // `inference.bodies.get(&id).is_some_and(|bt|
            // !bt.has_value_return)` guard this replaced).
            let inferred_void = knot.return_type.is_none()
                && inference.bodies.contains_key(&id)
                && !has_value_return_over_stitches(knot, id, file, index, inference);
            if has_void_annotation || inferred_void {
                out.insert(id);
            }
        }
    }
    out
}

/// This file's own reference resolutions, projected to a range-keyed lookup
/// (mirrors `infer::mod`'s `index_resolutions_by_file`, narrowed to one file
/// at a time — a `Path`'s range is only unique within its own file).
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

fn check_void_block(
    file: FileId,
    block: &Block,
    void_defs: &BTreeSet<DefinitionId>,
    resolution_by_range: &BTreeMap<(u32, u32), DefinitionId>,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    for stmt in &block.stmts {
        check_void_stmt(file, stmt, void_defs, resolution_by_range, out);
    }
}

fn check_void_stmt(
    file: FileId,
    stmt: &Stmt,
    void_defs: &BTreeSet<DefinitionId>,
    resolution_by_range: &BTreeMap<(u32, u32), DefinitionId>,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    match stmt {
        Stmt::TempDecl(t) => {
            if let Some(value) = &t.value {
                check_void_root(file, value, void_defs, resolution_by_range, out);
            }
        }
        Stmt::Assignment(a) => {
            check_void_root(file, &a.value, void_defs, resolution_by_range, out);
        }
        Stmt::ChoiceSet(cs) => {
            for choice in &cs.choices {
                check_void_block(file, &choice.body, void_defs, resolution_by_range, out);
                if let Some(c) = &choice.start_content {
                    check_void_content(file, c, void_defs, resolution_by_range, out);
                }
                if let Some(c) = &choice.bracket_content {
                    check_void_content(file, c, void_defs, resolution_by_range, out);
                }
                if let Some(c) = &choice.inner_content {
                    check_void_content(file, c, void_defs, resolution_by_range, out);
                }
            }
            check_void_block(file, &cs.continuation, void_defs, resolution_by_range, out);
        }
        Stmt::LabeledBlock(b) => check_void_block(file, b, void_defs, resolution_by_range, out),
        Stmt::Conditional(c) => {
            for branch in &c.branches {
                check_void_block(file, &branch.body, void_defs, resolution_by_range, out);
            }
        }
        Stmt::Sequence(s) => {
            for branch in &s.branches {
                check_void_block(file, &branch.body, void_defs, resolution_by_range, out);
            }
        }
        Stmt::Content(c) => check_void_content(file, c, void_defs, resolution_by_range, out),
        Stmt::LogicBlock(lb) => {
            for bs in &lb.stmts {
                check_void_block_stmt(file, bs, void_defs, resolution_by_range, out);
            }
        }
        // `~ await <cond>` (docs/flow-suspension-spec.md §3): the condition is
        // a value position, so a void-returning call used there is the same
        // strict-mode error it is anywhere else a value is expected.
        Stmt::Await(a) => {
            if let Some(cond) = &a.condition {
                check_void_root(file, cond, void_defs, resolution_by_range, out);
            }
        }
        Stmt::Divert(_)
        | Stmt::TunnelCall(_)
        | Stmt::ThreadStart(_)
        | Stmt::Return(_)
        | Stmt::ExprStmt(_)
        | Stmt::EndOfLine => {}
    }
}

fn check_void_content(
    file: FileId,
    content: &Content,
    void_defs: &BTreeSet<DefinitionId>,
    resolution_by_range: &BTreeMap<(u32, u32), DefinitionId>,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    for part in &content.parts {
        check_void_content_part(file, part, void_defs, resolution_by_range, out);
    }
}

fn check_void_content_part(
    file: FileId,
    part: &ContentPart,
    void_defs: &BTreeSet<DefinitionId>,
    resolution_by_range: &BTreeMap<(u32, u32), DefinitionId>,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    match part {
        ContentPart::InlineConditional(c) => {
            for branch in &c.branches {
                check_void_block(file, &branch.body, void_defs, resolution_by_range, out);
            }
        }
        ContentPart::InlineSequence(s) => {
            for branch in &s.branches {
                check_void_block(file, &branch.body, void_defs, resolution_by_range, out);
            }
        }
        // A span can nest a conditional/sequence (§4.3).
        ContentPart::Span(span) => {
            for child in &span.children {
                check_void_content_part(file, child, void_defs, resolution_by_range, out);
            }
        }
        ContentPart::Interpolation(_)
        | ContentPart::Text(_)
        | ContentPart::Glue
        | ContentPart::Spring => {}
    }
}

fn check_void_block_stmt(
    file: FileId,
    bs: &BlockStmt,
    void_defs: &BTreeSet<DefinitionId>,
    resolution_by_range: &BTreeMap<(u32, u32), DefinitionId>,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    match bs {
        BlockStmt::TempDecl(t) => {
            if let Some(value) = &t.value {
                check_void_root(file, value, void_defs, resolution_by_range, out);
            }
        }
        BlockStmt::Assignment(a) => {
            check_void_root(file, &a.value, void_defs, resolution_by_range, out);
        }
        BlockStmt::If(i) => check_void_if(file, i, void_defs, resolution_by_range, out),
        BlockStmt::While(w) => {
            for s in &w.body {
                check_void_block_stmt(file, s, void_defs, resolution_by_range, out);
            }
        }
        BlockStmt::For(f) => {
            for s in &f.body {
                check_void_block_stmt(file, s, void_defs, resolution_by_range, out);
            }
        }
        BlockStmt::Await(a) => {
            if let Some(cond) = &a.condition {
                check_void_root(file, cond, void_defs, resolution_by_range, out);
            }
        }
        BlockStmt::Return(_)
        | BlockStmt::ExprStmt(_)
        | BlockStmt::Break(_)
        | BlockStmt::Continue(_) => {}
    }
}

fn check_void_if(
    file: FileId,
    i: &IfStmt,
    void_defs: &BTreeSet<DefinitionId>,
    resolution_by_range: &BTreeMap<(u32, u32), DefinitionId>,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    for s in &i.body {
        check_void_block_stmt(file, s, void_defs, resolution_by_range, out);
    }
    match &i.else_branch {
        Some(ElseBranch::ElseIf(inner)) => {
            check_void_if(file, inner, void_defs, resolution_by_range, out);
        }
        Some(ElseBranch::Else(stmts)) => {
            for s in stmts {
                check_void_block_stmt(file, s, void_defs, resolution_by_range, out);
            }
        }
        None => {}
    }
}

/// If `expr`'s root is `Expr::Call(path, _)` resolving to a def in
/// `void_defs`, push `E067`. Anything else (a non-call root, or a call that
/// doesn't resolve to a known void def) is silently clean — this is a root-
/// position-only check, never a recursive expression walk (a void call
/// buried inside e.g. `1 + f()` is a type error `infer::body` would already
/// have caught as a non-numeric operand, not this diagnostic's job).
fn check_void_root(
    file: FileId,
    expr: &Expr,
    void_defs: &BTreeSet<DefinitionId>,
    resolution_by_range: &BTreeMap<(u32, u32), DefinitionId>,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    let Expr::Call(path, _) = expr else {
        return;
    };
    // `path.range` — the callee `Path`'s own whole span — is the exact key
    // the analyzer's `ResolvedRef::range` produced for this call site
    // (issue #1561; see that field's doc for the other three consumers
    // keying on the same contract). A narrowed range here would silently
    // stop finding a resolution and E067 would never fire.
    let Some(&def_id) = resolution_by_range.get(&range_key(path.range)) else {
        return;
    };
    if !void_defs.contains(&def_id) {
        return;
    }
    out.push(brink_ir::Diagnostic {
        file,
        range: path.range,
        message: format!(
            "`{}` returns void — its result cannot be assigned (docs/typed-mode-spec.md §3)",
            path_display(path)
        ),
        code: brink_ir::DiagnosticCode::E067,
    });
}

/// Dotted display name for a call target's `Path` (e.g. `knot.stitch`).
fn path_display(path: &Path) -> String {
    path.segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

/// A temp declaration's own name span plus its resolved ascription type
/// (`None` if unascribed or the ascription doesn't resolve — same "silent,
/// not binding" contract [`annotations::resolve`] documents).
struct TempDecl {
    range: TextRange,
    annotation_ty: Option<Ty>,
}

/// Walk one def's body collecting every `~ temp name[: type] = expr`
/// declaration by bare name (last declaration wins on a shadowed name — this
/// is diagnostic-only positioning, not a binding scope resolution). Mirrors
/// `infer::body`'s and `dialect_gate`'s own recursive shapes: `Stmt`-level
/// nesting (`ChoiceSet`/`Conditional`/`Sequence`/`LabeledBlock`/inline
/// content) plus the closed T1b `~ { … }` `BlockStmt` tree, which needs its
/// own hand-recursion (see `dialect_gate`'s module doc on why).
///
/// Deliberately does **not** descend into an `Expr::Lambda` reachable from a
/// `TempDecl`/`Assignment`/`Return`/`ExprStmt`'s own expression, looking for
/// the lambda's own `~ temp`/`let` declarations (issue #1763, filed from the
/// #1749/#1750 wave retro — that pair is the *effect-row* instance of a
/// block-bodied lambda's `stmts` going unwalked; this is the *strict-mode
/// temp-ascription* instance, in a different function, with a different
/// consumer). This walker has exactly one call site: `check_def`'s loop
/// over `body_types.locals`, `InferPass`'s accumulated map of the
/// **enclosing** def's own locals. After #1750's frame-boundary fix,
/// `InferPass::infer_lambda` snapshots `locals` (plus `annotated` /
/// `return_ty` / `has_value_return` / `local_fn_origins`) before walking a
/// block-bodied lambda's own body — its `stmts` through `infer_block_stmt`
/// and, since #1789, its tail expression too — and restores the snapshot
/// wholesale afterward, so a name declared only
/// inside a lambda body can **never** end up as a key in the enclosing
/// def's `body_types.locals`. On an *unshadowed* name that just means a
/// naive `Expr::Lambda` arm here would populate `TempDecl` entries this
/// module's one consumer structurally cannot look up — a dead entry,
/// nothing more. But on a *shadowed* name (the enclosing def declares the
/// same bare name the lambda does), `collect_temps`'s own last-write-wins
/// insert (see this fn's doc above) means a naive `Expr::Lambda` arm would
/// not stay dead: it would overwrite the enclosing declaration's
/// `TempDecl` — both `range` and `annotation_ty` — with the lambda's,
/// silently exempting the outer temp from `E065` and mis-spanning any
/// `E066` that does fire. That shadowing collision, not the harmlessness
/// of a dead entry on the unshadowed case, is the load-bearing reason not
/// to descend. Pinned by
/// `native_shadowed_lambda_local_temp_does_not_exempt_enclosing_temp`
/// (the shadowed case, proving the enclosing temp still `E065`-escapes
/// despite the lambda-local ascription), below.
///
/// Issue #1770 has since given a lambda its own strict-checked frame —
/// but not by descending here. `InferPass::infer_lambda` records a
/// lambda's own params/body-declared temps into a wholly separate,
/// cumulative vector (`BodyTypes::lambda_escapes`,
/// [`crate::infer::LambdaEscapeSlot`]), re-emitted by `check_def` under
/// the enclosing def's own label — never merged into, and never read
/// back out of, `body_types.locals`. So the shadowing collision this doc
/// describes is still exactly as live a hazard for *this* function as it
/// ever was: a naive `Expr::Lambda` arm added directly to `collect_temps`
/// would still overwrite an enclosing shadowed name's `TempDecl` with the
/// lambda-local one. #1770 sidesteps the whole question rather than
/// answering it — don't read the existence of `lambda_escapes` as license
/// to add that arm here; the two mechanisms solve different problems
/// (this fn's one consumer keys off *bare name*, which is exactly what
/// breaks under shadowing, while `lambda_escapes` never keys off name at
/// all). See `native_lambda_local_temp_ascription_now_reaches_its_own_
/// escape_check`, below, for the now-visible unshadowed case this doc
/// used to pin here before #1770 gave it a home of its own.
///
/// See `docs/effects-spec.md` §4.1 (issue #1762) for the general
/// frame-scoped-vs-cumulative `InferPass` field rule this is the mirror
/// case of: `locals` is frame-scoped and must not leak a lambda-local
/// name out into the enclosing def, from either direction.
fn collect_temps(body: &Block, names: &annotations::TypeNames) -> BTreeMap<String, TempDecl> {
    let mut out = BTreeMap::new();
    collect_temps_block(body, names, &mut out);
    out
}

fn collect_temps_block(
    block: &Block,
    names: &annotations::TypeNames,
    out: &mut BTreeMap<String, TempDecl>,
) {
    for stmt in &block.stmts {
        collect_temps_stmt(stmt, names, out);
    }
}

fn collect_temps_stmt(
    stmt: &Stmt,
    names: &annotations::TypeNames,
    out: &mut BTreeMap<String, TempDecl>,
) {
    match stmt {
        // `t.value` (the initializer expression) is never inspected here —
        // only the declaration's own name/ascription. See `collect_temps`'s
        // doc comment for why a nested `Expr::Lambda` inside it is not
        // walked either.
        Stmt::TempDecl(t) => {
            let annotation_ty = t
                .annotation
                .as_ref()
                .and_then(|te| annotations::resolve(te, names));
            out.insert(
                t.name.text.clone(),
                TempDecl {
                    range: t.name.range,
                    annotation_ty,
                },
            );
        }
        Stmt::ChoiceSet(cs) => {
            for choice in &cs.choices {
                collect_temps_block(&choice.body, names, out);
                if let Some(c) = &choice.start_content {
                    collect_temps_content(c, names, out);
                }
                if let Some(c) = &choice.bracket_content {
                    collect_temps_content(c, names, out);
                }
                if let Some(c) = &choice.inner_content {
                    collect_temps_content(c, names, out);
                }
            }
            collect_temps_block(&cs.continuation, names, out);
        }
        Stmt::LabeledBlock(b) => collect_temps_block(b, names, out),
        Stmt::Conditional(c) => {
            for branch in &c.branches {
                collect_temps_block(&branch.body, names, out);
            }
        }
        Stmt::Sequence(s) => {
            for branch in &s.branches {
                collect_temps_block(&branch.body, names, out);
            }
        }
        Stmt::Content(c) => collect_temps_content(c, names, out),
        Stmt::LogicBlock(lb) => {
            for bs in &lb.stmts {
                collect_temps_block_stmt(bs, names, out);
            }
        }
        // An `await` condition is an expression — it declares no temps
        // (docs/flow-suspension-spec.md §3). `Assignment`/`Return`/
        // `ExprStmt`/`Await` each carry an expression too (any of which
        // could itself be, or embed, an `Expr::Lambda`) that is likewise
        // never inspected — see `collect_temps`'s doc comment.
        Stmt::Divert(_)
        | Stmt::TunnelCall(_)
        | Stmt::ThreadStart(_)
        | Stmt::Assignment(_)
        | Stmt::Return(_)
        | Stmt::ExprStmt(_)
        | Stmt::Await(_)
        | Stmt::EndOfLine => {}
    }
}

fn collect_temps_content(
    content: &Content,
    names: &annotations::TypeNames,
    out: &mut BTreeMap<String, TempDecl>,
) {
    for part in &content.parts {
        collect_temps_content_part(part, names, out);
    }
}

fn collect_temps_content_part(
    part: &ContentPart,
    names: &annotations::TypeNames,
    out: &mut BTreeMap<String, TempDecl>,
) {
    match part {
        ContentPart::InlineConditional(c) => {
            for branch in &c.branches {
                collect_temps_block(&branch.body, names, out);
            }
        }
        ContentPart::InlineSequence(s) => {
            for branch in &s.branches {
                collect_temps_block(&branch.body, names, out);
            }
        }
        // A span can nest a conditional/sequence (§4.3).
        ContentPart::Span(span) => {
            for child in &span.children {
                collect_temps_content_part(child, names, out);
            }
        }
        ContentPart::Interpolation(_)
        | ContentPart::Text(_)
        | ContentPart::Glue
        | ContentPart::Spring => {}
    }
}

fn collect_temps_block_stmt(
    bs: &BlockStmt,
    names: &annotations::TypeNames,
    out: &mut BTreeMap<String, TempDecl>,
) {
    match bs {
        // `t.value` is never inspected — this is also the arm a `let g =
        // |x| …;` lambda-literal binding takes; see `collect_temps`'s doc
        // comment for why its body is not walked looking for the lambda's
        // own temps.
        BlockStmt::TempDecl(t) => {
            let annotation_ty = t
                .annotation
                .as_ref()
                .and_then(|te| annotations::resolve(te, names));
            out.insert(
                t.name.text.clone(),
                TempDecl {
                    range: t.name.range,
                    annotation_ty,
                },
            );
        }
        BlockStmt::If(i) => collect_temps_if(i, names, out),
        BlockStmt::While(w) => {
            for s in &w.body {
                collect_temps_block_stmt(s, names, out);
            }
        }
        BlockStmt::For(f) => {
            for s in &f.body {
                collect_temps_block_stmt(s, names, out);
            }
        }
        // Each of these carries an expression too (any of which could
        // itself be, or embed, an `Expr::Lambda`) that is likewise never
        // inspected — see `collect_temps`'s doc comment.
        BlockStmt::Assignment(_)
        | BlockStmt::Return(_)
        | BlockStmt::ExprStmt(_)
        | BlockStmt::Await(_)
        | BlockStmt::Break(_)
        | BlockStmt::Continue(_) => {}
    }
}

fn collect_temps_if(
    i: &IfStmt,
    names: &annotations::TypeNames,
    out: &mut BTreeMap<String, TempDecl>,
) {
    for s in &i.body {
        collect_temps_block_stmt(s, names, out);
    }
    match &i.else_branch {
        Some(ElseBranch::ElseIf(inner)) => collect_temps_if(inner, names, out),
        Some(ElseBranch::Else(stmts)) => {
            for s in stmts {
                collect_temps_block_stmt(s, names, out);
            }
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::{Diagnostic, DiagnosticCode, ResolutionMap, hir::lower};

    // ── resolve_type_policy: one test per (dialect × explicit/implicit)
    //    cell (issue #1127, ruled 2026-07-19) ──────────────────────────────

    #[test]
    fn resolve_brink_implicit_defaults_strict() {
        assert_eq!(
            resolve_type_policy(crate::Dialect::Brink, None),
            TypePolicy::Strict
        );
    }

    #[test]
    fn resolve_strict_ink_implicit_defaults_gradual() {
        assert_eq!(
            resolve_type_policy(crate::Dialect::StrictInk, None),
            TypePolicy::Gradual
        );
    }

    #[test]
    fn resolve_brink_explicit_gradual_wins() {
        assert_eq!(
            resolve_type_policy(crate::Dialect::Brink, Some(TypePolicy::Gradual)),
            TypePolicy::Gradual
        );
    }

    #[test]
    fn resolve_brink_explicit_strict_stays_strict() {
        assert_eq!(
            resolve_type_policy(crate::Dialect::Brink, Some(TypePolicy::Strict)),
            TypePolicy::Strict
        );
    }

    #[test]
    fn resolve_strict_ink_explicit_gradual_stays_gradual() {
        assert_eq!(
            resolve_type_policy(crate::Dialect::StrictInk, Some(TypePolicy::Gradual)),
            TypePolicy::Gradual
        );
    }

    #[test]
    fn resolve_strict_ink_explicit_strict_wins() {
        // The E064 config error is downstream (config_error); resolution
        // itself honors the explicit request.
        assert_eq!(
            resolve_type_policy(crate::Dialect::StrictInk, Some(TypePolicy::Strict)),
            TypePolicy::Strict
        );
    }

    fn build(src: &str) -> (HirFile, SymbolIndex, ResolutionMap) {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _diag) = lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) =
            crate::resolve(FileId(0), &manifest, &index, &crate::ImportScope::default());
        (hir, (*index).clone(), (*resolutions).clone())
    }

    fn codes(diags: &[Diagnostic]) -> Vec<DiagnosticCode> {
        let mut v: Vec<DiagnosticCode> = diags.iter().map(|d| d.code).collect();
        v.sort_by_key(|c| c.as_str());
        v
    }

    // ── config_error ────────────────────────────────────────────────

    #[test]
    fn config_error_fires_for_strict_ink_dialect() {
        let diag = config_error(crate::Dialect::StrictInk, Some(FileId(0)));
        assert!(diag.is_some());
        assert_eq!(diag.expect("checked above").code, DiagnosticCode::E064);
    }

    #[test]
    fn config_error_is_none_for_brink_dialect() {
        assert!(config_error(crate::Dialect::Brink, Some(FileId(0))).is_none());
    }

    #[test]
    fn config_error_is_none_with_no_files() {
        assert!(config_error(crate::Dialect::StrictInk, None).is_none());
    }

    // ── strict_diagnostics: is_native decouples E064 (issue #1348) ────
    //
    // `dialect` is an ink-only axis (docs/t1b-surface-spec.md §1) — a native
    // `.brink` project has no dialect to be wrong about, so `config_error`
    // must never fire for one, regardless of `opts.dialect`'s value (a
    // native compile never sets it, and the default `StrictInk` is what used
    // to trip `E064` the instant `types = strict` was requested).

    #[test]
    fn strict_diagnostics_is_native_true_never_fires_config_error() {
        // `dialect` left at its `StrictInk` default — the exact combination
        // that fires `E064` for an ink project — plus `types = strict`, the
        // B0.9 native strict-only posture (issue #1342).
        let (hir, index, res) = build("=== main ===\nHello.\n-> DONE\n");
        let opts = crate::AnalysisOptions {
            types: Some(TypePolicy::Strict),
            ..Default::default()
        };
        let diags = crate::strict_diagnostics(
            &[(FileId(0), &hir)],
            &index,
            &res,
            &opts,
            true,
            None,
            &BTreeMap::new(),
        );
        assert!(
            !diags.iter().any(|d| d.code == DiagnosticCode::E064),
            "native must never see the ink-only dialect config error: {diags:?}"
        );
    }

    #[test]
    fn strict_diagnostics_is_native_true_still_runs_inference_checks() {
        // Skipping `config_error` must not skip the rest of strict mode —
        // an otherwise-escaping param must still `E065` for a native project.
        let (hir, index, res) = build("=== noop(x) ===\nHello.\n-> DONE\n");
        let opts = crate::AnalysisOptions {
            types: Some(TypePolicy::Strict),
            ..Default::default()
        };
        let diags = crate::strict_diagnostics(
            &[(FileId(0), &hir)],
            &index,
            &res,
            &opts,
            true,
            None,
            &BTreeMap::new(),
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E065);
    }

    #[test]
    fn strict_diagnostics_is_native_false_unaffected_still_fires_config_error() {
        // The `is_native = false` (ink) path is byte-identical to before
        // this parameter existed — same `StrictInk` + `types = strict`
        // combination as the test above, still an `E064` config error.
        let (hir, index, res) = build("=== main ===\nHello.\n-> DONE\n");
        let opts = crate::AnalysisOptions {
            types: Some(TypePolicy::Strict),
            ..Default::default()
        };
        let diags = crate::strict_diagnostics(
            &[(FileId(0), &hir)],
            &index,
            &res,
            &opts,
            false,
            None,
            &BTreeMap::new(),
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E064);
    }

    // ── native_strict_only_error (B0.9, issue #1342) ────────────────

    #[test]
    fn native_strict_only_fires_for_explicit_gradual() {
        let diag = native_strict_only_error(FileId(0), Some(TypePolicy::Gradual));
        assert!(diag.is_some());
        assert_eq!(diag.expect("checked above").code, DiagnosticCode::E137);
    }

    #[test]
    fn native_strict_only_is_none_for_explicit_strict() {
        assert!(native_strict_only_error(FileId(0), Some(TypePolicy::Strict)).is_none());
    }

    #[test]
    fn native_strict_only_is_none_for_unset_types() {
        // No explicit knob turned — the dialect-defaulted resolution (not
        // this gate's concern, see the function doc) governs instead.
        assert!(native_strict_only_error(FileId(0), None).is_none());
    }

    // ── check(): Unknown-escape ────────────────────────────────────

    #[test]
    fn unused_param_escapes_as_unknown() {
        let (hir, index, res) = build("=== noop(x) ===\nHello.\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E065);
        assert!(diags[0].message.contains('x'));
    }

    #[test]
    fn annotated_unused_param_is_exempt_from_unknown_escape() {
        let (hir, index, res) = build("=== noop(x: int) ===\nHello.\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(diags.is_empty(), "annotation supplies the type: {diags:?}");
    }

    /// T1d-2 (docs/t1d-spec.md §3): a `Handle<K>`-annotated, otherwise-unused
    /// param is exempt from `E065` the same way any other resolvable
    /// annotation is — "strict kind-checking via existing TM-3 machinery",
    /// reusing the annotation-firewall exemption path, no new code needed.
    /// Reachable only when the manifest declaring `K` is registered — with
    /// none registered, the annotation doesn't resolve and the slot escapes
    /// as `Unknown` exactly like an unrecognized type name would.
    #[test]
    fn annotated_handle_param_is_exempt_from_unknown_escape_when_kind_is_registered() {
        let (hir, index, res) = build("=== noop(x: Handle<AudioInstance>) ===\nHello.\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let manifest = brink_ir::HostManifest {
            markup: Vec::new(),
            types: vec![brink_ir::SemanticTypeDef {
                name: "AudioInstance".to_string(),
                base: brink_ir::BaseType::Handle,
                constraint: None,
                values: None,
                widget: None,
            }],
            ..Default::default()
        };
        let diags = check(
            &[(FileId(0), &hir)],
            &index,
            &inference,
            &res,
            Some(&manifest),
        );
        assert!(diags.is_empty(), "annotation supplies the type: {diags:?}");
    }

    // ── NG-A/NG-B: the annotation firewall reaches the NATIVE frontend ──
    //
    // Issues #1487/#1488. Native is strict-only (E137, #1342), so before
    // the `: type` grammar existed every escaping native param/binding was
    // structurally condemned to `E065` — this module's annotation firewall
    // (`check_def`'s `p.annotation` / `collect_temps`' `annotation_ty`
    // exemption, which exempts an `Unknown` escape but never a
    // `Conflicted` one) was unreachable from a `.brink` file. These prove
    // it now fires, through the same `check` entry point the ink fixtures
    // above use — the annotations land in the same `hir::TypeExpr` slots.

    /// Native-lowered `(HirFile, SymbolIndex, ResolutionMap)`, the native
    /// counterpart of [`build`] (which parses through `brink_syntax`, the
    /// ink/brink-extension frontend). Mirrors `coalesce`'s own
    /// `build_native`.
    fn build_native(src: &str) -> (HirFile, SymbolIndex, ResolutionMap) {
        let parsed = brink_syntax_native::parse(src);
        assert!(
            parsed.errors().is_empty(),
            "fixture must parse cleanly: {:?}",
            parsed.errors()
        );
        let tree = parsed.tree();
        let (hir, manifest, _diag) = brink_ir::hir::lower_native::lower(FileId(0), &tree);
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) =
            crate::resolve(FileId(0), &manifest, &index, &crate::ImportScope::default());
        (hir, (*index).clone(), (*resolutions).clone())
    }

    fn native_strict_diags(src: &str) -> Vec<Diagnostic> {
        let (hir, index, res) = build_native(src);
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        check(&[(FileId(0), &hir)], &index, &inference, &res, None)
    }

    #[test]
    fn native_unannotated_param_escapes_as_unknown() {
        // The baseline the exemption is measured against — without it the
        // next test would pass with the firewall deleted.
        let diags = native_strict_diags("flow noop(x) {\n  Hello.\n}\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E065);
    }

    #[test]
    fn native_annotated_param_is_exempt_from_unknown_escape() {
        let diags = native_strict_diags("flow noop(x: int) {\n  Hello.\n}\n");
        assert!(
            diags.is_empty(),
            "the `: int` annotation supplies the type: {diags:?}"
        );
    }

    // ── Issue #1912(a): handing an annotated param straight back out ──
    //
    // `infer::body::InferPass::infer_return` applies `or_own_annotation` to
    // the returned value, so `return <annotated param>` exports the
    // parameter's declared type instead of `Unknown`. Filed against
    // `content` (#1846 gave it a resolvable `Ty`; #1882's native strict
    // sweep caught the corpus row) but never `content`-specific — the
    // second test below is the proof of that.

    #[test]
    fn native_returning_a_content_param_takes_its_annotated_type() {
        // Issue #1912's own reduction, both halves: the annotated-return
        // twin was already clean, the bare one reported `E065` on a return
        // type that is *exactly* the annotated parameter type.
        let bare = native_strict_diags("fn passthru(t: content) {\n  return t;\n}\n");
        assert!(
            bare.is_empty(),
            "`t: content` supplies the return type: {bare:?}"
        );
        let annotated = native_strict_diags("fn passthru(t: content): content {\n  return t;\n}\n");
        assert!(
            annotated.is_empty(),
            "the annotated twin stays clean: {annotated:?}"
        );
    }

    #[test]
    fn native_returning_an_annotated_param_is_not_content_specific() {
        // Issue #1912 framed the gap as a `content` one; it was general to
        // every resolvable annotation. All four leaf spellings, so a fix
        // that only special-cased `Ty::Content` would fail here.
        for ty in ["int", "float", "bool", "string"] {
            let src = format!("fn passthru(t: {ty}) {{\n  return t;\n}}\n");
            let diags = native_strict_diags(&src);
            assert!(
                diags.is_empty(),
                "`t: {ty}` supplies the return type: {diags:?}"
            );
        }
    }

    #[test]
    fn native_a_body_use_contradicting_the_annotation_still_reports_e063() {
        // The TM-2 firewall #1912's fix must not dissolve: `or_own_annotation`
        // overlays an `Unknown` only, so a param the body *does* constrain
        // keeps exporting its own independent derivation and
        // `annotations::mismatches` still has two things to compare.
        let diags = native_strict_diags("fn f(a: int) {\n  return a + \"x\";\n}\n");
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E063),
            "a body use disagreeing with the annotation still reports E063: {diags:?}"
        );
    }

    #[test]
    fn native_returning_a_param_that_disagrees_with_the_return_annotation_reports_e063() {
        // The other side of the same coin, and a diagnostic that could not
        // fire before #1912: the body's return type used to escape as
        // `Unknown` and get overlaid by the *return* annotation, so a
        // handing-through that contradicts the declared return was silent.
        let diags = native_strict_diags("fn f(t: content): string {\n  return t;\n}\n");
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E063),
            "returning a `content` param from a `: string` fn disagrees: {diags:?}"
        );
    }

    #[test]
    fn native_unresolvable_param_annotation_still_escapes() {
        // The firewall exempts a *resolvable* annotation only
        // (`annotations::resolve`) — an unrecognized name supplies nothing.
        let diags = native_strict_diags("flow noop(x: Nonesuch) {\n  Hello.\n}\n");
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E065),
            "an unresolvable annotation must not exempt the slot: {diags:?}"
        );
    }

    #[test]
    fn native_annotated_let_is_exempt_from_unknown_escape() {
        // NG-B's binding half: an ascribed `let` inside a code-ground `fn`
        // body reaches `collect_temps`' `annotation_ty` exemption.
        let bare = native_strict_diags("fn f(n: int): int {\n  let t;\n  return n;\n}\n");
        assert!(
            bare.iter().any(|d| d.code == DiagnosticCode::E065),
            "an unannotated, uninferable `let` escapes: {bare:?}"
        );
        let annotated =
            native_strict_diags("fn f(n: int): int {\n  let t: string;\n  return n;\n}\n");
        assert!(
            annotated.is_empty(),
            "the `: string` ascription supplies the type: {annotated:?}"
        );
    }

    /// Issue #1770 (closing the gap #1763 pinned as the then-deliberate
    /// interim posture): a temp declared *inside* a lambda's own block body
    /// now gets its own per-lambda escape-check frame
    /// ([`crate::infer::LambdaEscapeSlot`]), populated by
    /// `InferPass::infer_lambda` and re-emitted by `check_def` under the
    /// enclosing def's own label — so it is no longer invisible just
    /// because it never reaches the *enclosing* def's own
    /// `body_types.locals` (`InferPass::infer_lambda`'s #1750 snapshot/
    /// restore of `locals` still keeps it out of *that* map; this is a
    /// wholly separate, cumulative map fed straight from the same walk).
    ///
    /// Before this fix (see the git history of this test, formerly
    /// `native_lambda_local_temp_ascription_is_invisible_to_enclosing_
    /// escape_check`): the unannotated and the ascribed lambda-local `let
    /// t` below produced the *identical* (empty) diagnostic set — the
    /// ascription changed nothing observable. Now they genuinely differ,
    /// proving the ascription firewall reaches a lambda's own temps exactly
    /// like it already does a top-level one
    /// (`native_annotated_let_is_exempt_from_unknown_escape`, the same `let
    /// t;` shape one scope out).
    #[test]
    fn native_lambda_local_temp_ascription_now_reaches_its_own_escape_check() {
        let unannotated = native_strict_diags(
            "fn f(n: int): int {\n  let g = |x: int|: int {\n    let t;\n    x\n  };\n  return n;\n}\n",
        );
        assert_eq!(
            unannotated.len(),
            1,
            "the lambda's own unannotated `let t` (never used, so genuinely \
             `Unknown`) now escapes in its own right: {unannotated:?}"
        );
        assert_eq!(unannotated[0].code, DiagnosticCode::E065);
        assert!(
            unannotated[0].message.contains("lambda temp `t`"),
            "{unannotated:?}"
        );

        let ascribed = native_strict_diags(
            "fn f(n: int): int {\n  let g = |x: int|: int {\n    let t: string;\n    x\n  };\n  return n;\n}\n",
        );
        assert!(
            ascribed.is_empty(),
            "the `: string` ascription now supplies the type, exempting the \
             lambda's own temp exactly like a top-level one: {ascribed:?}"
        );
    }

    /// Guards the shadowing hazard the `collect_temps` doc comment calls
    /// out: on a shadowed name, `collect_temps`'s last-write-wins insert
    /// means a naive `Expr::Lambda` arm added directly to it would
    /// overwrite the *enclosing* `let t;`'s `TempDecl` with the
    /// lambda-local `let t: string;`'s ascribed one, silently exempting the
    /// outer temp from `E065`. Issue #1770 gives the lambda's own `t` a
    /// genuine escape-check frame now (a separate, `LambdaEscapeSlot`-based
    /// map fed straight from `InferPass::infer_lambda`'s own walk — see
    /// that fact's own doc for why this sidesteps the collision entirely,
    /// never touching `collect_temps`), so this fixture's own hazard
    /// (`collect_temps_stmt`/`collect_temps_block_stmt` growing a naive
    /// `Expr::Lambda` arm) remains exactly as un-triggered as before. Still
    /// pins the enclosing temp's own escape, now expected to be the *only*
    /// diagnostic (the lambda's own `t: string` is separately, correctly
    /// exempt by its own ascription). (Shadowing itself is legal here:
    /// `check_capture_writes`
    /// (`crates/internal/brink-ir/src/hir/lower_native/lambda.rs`) fires
    /// `E156` only on writes to *captured* outers, never on a lambda-local
    /// re-declaration of the same name.)
    #[test]
    fn native_shadowed_lambda_local_temp_does_not_exempt_enclosing_temp() {
        let diags = native_strict_diags(
            "fn f(n: int): int {\n  let t;\n  let g = |x: int|: int {\n    let t: string;\n    x\n  };\n  return n;\n}\n",
        );
        assert_eq!(
            diags.len(),
            1,
            "the enclosing, unannotated `let t;` must still escape as E065 \
             even though a lambda-local `let t: string;` shadows the same \
             bare name with its own ascription — a naive `Expr::Lambda` arm \
             in `collect_temps` would overwrite the enclosing `TempDecl` \
             and silently swallow this; the lambda's own `t` is separately \
             exempt by its own ascription, so nothing else should appear: \
             {diags:?}"
        );
        assert_eq!(diags[0].code, DiagnosticCode::E065);
    }

    /// Issue #1770's `E066` (Conflicted-escape) half — the sibling of
    /// [`native_lambda_local_temp_ascription_now_reaches_its_own_escape_check`],
    /// which only pins the `E065` (Unknown-escape) half. `t` is written as
    /// an `int` then reassigned a `string` inside the lambda's own block
    /// body, a genuine same-type disagreement (`unify(int, string) ==
    /// Conflicted`, the #627 lattice) local to the lambda's own frame —
    /// never observable at the top level at all, since `t` is declared and
    /// used entirely inside `g`'s body.
    #[test]
    fn native_lambda_local_temp_with_conflicting_uses_reports_e066() {
        let diags = native_strict_diags(
            "fn f(n: int): int {\n  let g = |x: int|: int {\n    let t = 1;\n    t = \"oops\";\n    x\n  };\n  return n;\n}\n",
        );
        assert_eq!(
            diags.len(),
            1,
            "the lambda's own `t` genuinely disagrees with itself (`int` \
             then `string`) and must escape as E066, not merely go \
             unreported the way a lambda-local temp did before #1770: \
             {diags:?}"
        );
        assert_eq!(diags[0].code, DiagnosticCode::E066);
        assert!(diags[0].message.contains("lambda temp `t`"), "{diags:?}");
    }

    /// Review finding on #1770: a param name the lambda's own body
    /// re-binds (`|t: int| { let t = 1; t = "oops"; t }`) must be reported
    /// as the *rebound local's* own escape, never misattributed to the
    /// annotated parameter of the same spelling. Before this fix, the
    /// params governance loop read `self.locals["t"]` — by then the
    /// rebound local's accumulated type, `Conflicted` here, not the
    /// param's — straight into a `LambdaEscapeSlot` labeled
    /// `"lambda parameter `t`"`, blaming the annotated param for a
    /// contradiction entirely internal to the fresh local that shadows it,
    /// while the body-declared-temps loop silently skipped the name
    /// entirely (see `LambdaEscapeSlot::annotated`'s doc and
    /// `InferPass::infer_lambda`'s two governance-loop comments). The
    /// temps loop now owns this name instead, so the only lambda-frame
    /// row is a `"lambda temp `t`"` one and no `"lambda parameter `t`"`
    /// row appears. The enclosing `let g = …` temp also escapes as
    /// Conflicted in its own right — `g`'s inferred `fn(Conflicted):
    /// Conflicted` type recursively classifies as Conflicted too
    /// (`classify`'s own `Ty::Fn` arm, the same shape
    /// `native_nested_lambda_inside_lambda_gets_its_own_escape_frame_too`
    /// exercises for Unknown) — a real, independent escape, not a
    /// duplicate.
    #[test]
    fn native_lambda_rebound_param_escape_is_attributed_to_the_temp_not_the_param() {
        let diags = native_strict_diags(
            "fn f(n: int): int {\n  let g = |t: int| {\n    let t = 1;\n    t = \"oops\";\n    t\n  };\n  return n;\n}\n",
        );
        assert_eq!(
            diags.len(),
            2,
            "the rebound local `t`'s own int/string contradiction escapes \
             at its own lambda-frame slot, and `g`'s own inferred \
             fn(Conflicted): Conflicted type recursively escapes too: \
             {diags:?}"
        );
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E066));
        assert!(
            diags.iter().any(|d| d.message.contains("lambda temp `t`")),
            "must be attributed to the rebound local, not the annotated \
             parameter of the same name: {diags:?}"
        );
        assert!(
            diags.iter().any(|d| d.message.contains("temp `g`")),
            "{diags:?}"
        );
        assert!(
            diags
                .iter()
                .all(|d| !d.message.contains("lambda parameter `t`")),
            "the annotated parameter `t` must never be blamed for a \
             contradiction entirely internal to the local that shadows it: \
             {diags:?}"
        );
    }

    /// Issue #1770's own doc on [`crate::infer::LambdaEscapeSlot`]: a lambda
    /// nested inside another lambda's own body gets its **own**, separate
    /// frame — its escape slots are folded into the same flat, cumulative
    /// vector as the outer lambda's, not merged into (or lost inside) the
    /// outer lambda's own frame. Two diagnostics prove two independent
    /// things fired: `h`'s own unannotated, unused param `y` is only
    /// reachable by recursing into `g`'s own nested lambda `h` (proving the
    /// per-lambda walk genuinely recurses rather than stopping at the
    /// first lambda it finds); `g`'s own temp `h` *also* escapes, because
    /// `h`'s inferred `fn(Unknown): Unknown` type recursively classifies as
    /// `Unknown` too (`classify`'s own `Ty::Fn` arm) — a real, independent
    /// escape at `g`'s own frame, not a duplicate of `h`'s.
    #[test]
    fn native_nested_lambda_inside_lambda_gets_its_own_escape_frame_too() {
        let diags = native_strict_diags(
            "fn f(n: int): int {\n  let g = |x: int|: int {\n    let h = |y| y;\n    x\n  };\n  return n;\n}\n",
        );
        assert_eq!(
            diags.len(),
            2,
            "`h`'s own param `y` (only reachable by recursing into `g`'s \
             nested lambda) and `g`'s own temp `h` (whose `fn(Unknown): \
             Unknown` type itself classifies as Unknown) are two \
             independent escapes: {diags:?}"
        );
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E065));
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("lambda parameter `y`")),
            "{diags:?}"
        );
        assert!(
            diags.iter().any(|d| d.message.contains("lambda temp `h`")),
            "{diags:?}"
        );
    }

    /// Issue #1789, the **read** direction of the tail-ordering bug: a
    /// block-bodied lambda's tail expression must be inferred while the
    /// lambda's own `locals` frame is still live, so a temp declared by the
    /// lambda's own `stmts` is visible to it.
    ///
    /// `h` is a lambda-local `fn(int): int` referenced *only* in tail
    /// position. Before the fix, `infer_lambda` walked the tail after
    /// restoring the enclosing def's `locals`, so `ty_of_def` (which keys
    /// `locals` by bare name) found nothing and typed the callee `Unknown`
    /// — `infer_value_call`'s `E063` arity check is skipped entirely on an
    /// `Unknown` callee, so the over-applied `h(1, 2)` was never checked
    /// for arity at all. A spurious `E065` Unknown-escape fired in its
    /// place instead — the wrong diagnostic, not silence.
    ///
    /// The `stmt_position` half is the discriminator: the identical
    /// over-application written as a `;`-terminated statement inside the
    /// same block has always been caught (it is walked inside the frame,
    /// per #1750), so this pins the *tail* as the thing that was broken
    /// rather than the arity check generally.
    #[test]
    fn native_lambda_tail_sees_its_own_block_locals() {
        let stmt_position = native_strict_diags(
            "fn f(n: int): int {\n  let g = |x: int|: int {\n    let h = |y: int|: int { y };\n    h(1, 2);\n    x\n  };\n  return n;\n}\n",
        );
        assert!(
            stmt_position.iter().any(|d| d.code == DiagnosticCode::E063),
            "baseline: an over-applied call to a lambda-local fn temp in \
             *statement* position is inside the #1750 frame window and has \
             always been checked: {stmt_position:?}"
        );

        let tail_position = native_strict_diags(
            "fn f(n: int): int {\n  let g = |x: int|: int {\n    let h = |y: int|: int { y };\n    h(1, 2)\n  };\n  return n;\n}\n",
        );
        assert!(
            tail_position.iter().any(|d| d.code == DiagnosticCode::E063),
            "the very same over-application in *tail* position must be \
             checked too — the tail is the block's value position and reads \
             the locals its own statements bound (#1789): {tail_position:?}"
        );
    }

    /// Issue #1789, the **write** direction — the leak #1750 closed for a
    /// lambda's `stmts` but left open for its tail.
    ///
    /// `observe` (see `infer::body`) keys `locals` by bare name, so a use in
    /// argument position unifies the parameter's type into whatever local
    /// carries that name *right now*. With the tail walked after the
    /// restore, the lambda's own `let t = "hi"` was gone and `takes_string(t)`
    /// unified `string` into the **enclosing** `f`'s `let t = 1` —
    /// `unify(int, string) == Conflicted` — reporting a spurious `E066` on a
    /// temp `f`'s own body never misuses. A false positive on user code.
    ///
    /// The `stmt_position` half is again the discriminator: the same
    /// argument-position use written as a statement never leaked, because
    /// #1750's snapshot/restore already wrapped it.
    #[test]
    fn native_lambda_tail_does_not_corrupt_a_shadowed_enclosing_local() {
        const TAKES_STRING: &str = "fn takes_string(s: string): string {\n  return s;\n}\n";

        let stmt_position = native_strict_diags(&format!(
            "{TAKES_STRING}fn f(n: int): int {{\n  let t = 1;\n  let g = |x: int|: int {{\n    let t = \"hi\";\n    takes_string(t);\n    x\n  }};\n  return n;\n}}\n"
        ));
        assert!(
            stmt_position.is_empty(),
            "baseline: a lambda-local `t` used in argument position from a \
             *statement* is confined by #1750's snapshot/restore, so the \
             enclosing `let t = 1` stays `int`: {stmt_position:?}"
        );

        let tail_position = native_strict_diags(&format!(
            "{TAKES_STRING}fn f(n: int): int {{\n  let t = 1;\n  let g = |x: int|: string {{\n    let t = \"hi\";\n    takes_string(t)\n  }};\n  return n;\n}}\n"
        ));
        assert!(
            tail_position.is_empty(),
            "the same use in *tail* position must be confined the same way — \
             before #1789 it unified `string` into the enclosing `f`'s own \
             `t: int` and reported a spurious E066 Conflicted-escape on it: \
             {tail_position:?}"
        );
    }

    /// Issue #1789, a third direction discovered during review: opening the
    /// frame *around* the tail (not just restoring after it) also changes
    /// where `observe` lands for a *captured* (not shadowed) enclosing
    /// temp used from tail position — `f`'s own `let c;` no longer narrows
    /// from a use inside `g`'s tail, because that use's `observe` now runs
    /// and is undone inside the lambda's frame before `f`'s frame sees it.
    ///
    /// This is not a regression: the statement-position twin already
    /// reported the same `E065` on both sides of this PR (`c` is
    /// unannotated and untouched at every one of `f`'s own use sites
    /// either way, per #1750), so the tail case is now merely consistent
    /// with it rather than an outlier. It reaches
    /// `types = strict` diagnostics same as the other two directions, so
    /// it is pinned here rather than left as an incidental side effect.
    #[test]
    fn native_lambda_tail_capture_use_no_longer_narrows_enclosing_capture() {
        const TAKES_STRING: &str = "fn takes_string(s: string): string {\n  return s;\n}\n";

        let stmt_position = native_strict_diags(&format!(
            "{TAKES_STRING}fn f(n: int): int {{\n  let c;\n  let g = ||: int {{ takes_string(c); 1 }};\n  return n;\n}}\n"
        ));
        assert!(
            stmt_position.iter().any(|d| d.code == DiagnosticCode::E065),
            "baseline: `f`'s own unannotated `let c;` still E065-escapes \
             when the capturing use is in *statement* position, on both \
             sides of #1789 — the use is never enough to narrow it: \
             {stmt_position:?}"
        );

        let tail_position = native_strict_diags(&format!(
            "{TAKES_STRING}fn f(n: int): int {{\n  let c;\n  let g = ||: string {{ takes_string(c) }};\n  return n;\n}}\n"
        ));
        assert!(
            tail_position.iter().any(|d| d.code == DiagnosticCode::E065),
            "the same capturing use from *tail* position must escape the \
             same way — before #1789 the tail's `observe` ran against \
             whatever frame was live *after* the restore and could narrow \
             `f`'s own `c`; opening the frame around the tail keys that \
             `observe` to the lambda's own (discarded) frame instead, so \
             `c` is left exactly as unannotated as the statement-position \
             twin: {tail_position:?}"
        );
    }

    // ── issue #1910: pure verb results and lambda-bound locals ────────
    //
    // `infer::body::InferPass::infer_lambda` used to walk a lambda's body
    // purely for its side effects and then throw away everything it
    // learned, rebuilding the lambda's own `Ty::Fn(params, ret, _)` from
    // *written* annotations alone — `Unknown` for every unannotated param,
    // `Unknown` for an unannotated return regardless of what the body
    // actually computed. That made a pure verb's inline callback
    // (`map`/`filter`/`fold`/`filter_map`/`map_each`) and a lambda literal
    // bound straight to a local escape strict inference as `Unknown` even
    // when the body unambiguously pinned the type.

    #[test]
    fn native_map_result_infers_from_unannotated_lambda_body() {
        let diags = native_strict_diags(
            "fn doubled() {\n  let items = [1, 2, 3];\n  return map(items, |x| x * 2);\n}\n",
        );
        assert!(
            diags.is_empty(),
            "`x * 2` pins `x` (and so `map`'s result) to `int` from the \
             callback's own body alone, with no surrounding annotation: \
             {diags:?}"
        );
    }

    #[test]
    fn native_fold_result_falls_back_to_the_seed_when_the_callback_body_is_unconstrained() {
        let diags = native_strict_diags(
            "fn total() {\n  let items = [1, 2, 3, 4];\n  return fold(items, 0, |acc, x| acc + x);\n}\n",
        );
        // Issue #1770: `acc`/`x` now get their own per-lambda escape-check
        // frame, and neither is pinned by the callback's own body (`acc + x`
        // joins two `Unknown`s) — so both correctly escape as `E065` in
        // their own right. What this test still pins is `fold`'s *own*
        // result: it must fall back to the seed `0`'s `int` rather than
        // itself escaping as a third, redundant diagnostic on `total`'s own
        // return type — the absence of any such row below is that proof.
        assert_eq!(
            diags.len(),
            2,
            "only the lambda's own two unconstrained params should escape — \
             `total`'s own return type must still fall back cleanly to the \
             seed's `int`: {diags:?}"
        );
        assert!(
            diags.iter().all(|d| d.code == DiagnosticCode::E065
                && (d.message.contains("lambda parameter `acc`")
                    || d.message.contains("lambda parameter `x`"))),
            "{diags:?}"
        );
    }

    #[test]
    fn native_fold_still_reports_conflicted_when_the_callback_body_genuinely_conflicts() {
        // The seed fallback above must not paper over a genuine conflict —
        // `fold`'s arm only falls back to the seed when the callback's own
        // return is `Unknown`, never when it is `Conflicted` (real
        // information: the body really did observe two disagreeing types).
        // `-`, not `+`: issue #1911 (landed on `main` after this fixture was
        // first written) rules `string + int`/`string + float` legal display
        // concatenation, typing to `string` rather than `Conflicted` — `-`
        // has no such carve-out (`is_string_numeric_concat` is scoped to
        // `Add` only), so it still exercises a genuine same-type mismatch.
        let diags = native_strict_diags(
            "fn fold_conflicted() {\n  let items = [1, 2, 3];\n  return fold(items, 0, |a, b| {\n    let t = a + 1;\n    let t2 = a - \"oops\";\n    t2\n  });\n}\n",
        );
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E066),
            "`a` is joined against `int` (`a + 1`) and then `string` \
             (`a - \"oops\"`) inside the callback's own body — a genuine \
             conflict that must surface as E066, not be silently replaced \
             by the seed's `int`: {diags:?}"
        );
    }

    #[test]
    fn native_verb_result_bound_to_a_let_is_not_unknown() {
        let diags = native_strict_diags(
            "fn let_map_then_len() {\n  let items = [1, 2, 3];\n  let out = map(items, |x| x * 2);\n  return len(out);\n}\n",
        );
        assert!(
            diags.is_empty(),
            "`out`'s type comes from `map`'s own now-concrete result, not \
             just the return position — a genuinely intermediate binding \
             must be just as clean: {diags:?}"
        );
    }

    #[test]
    fn native_block_bodied_lambda_return_feeds_the_verb_result_too() {
        // The block-bodied twin of `native_map_result_infers_from_
        // unannotated_lambda_body`: the value comes from an internal
        // `return`, not the block's tail — `LambdaBody::Block`'s own doc:
        // "return leaves the lambda". Before #1910's `return_ty` reset this
        // read `self.return_ty` contaminated by whatever the *enclosing*
        // def's own return_ty already held, so `positives` (an `if`/`return`
        // shaped `filter_map` callback, `tests/tier1-native/lambda-verbs/
        // story.brink`) needed both fixes to resolve.
        let diags = native_strict_diags(
            "fn ret_from_block_lambda() {\n  let items = [1, 2, 3];\n  return map(items, |x| {\n    return x * 3;\n  });\n}\n",
        );
        assert!(
            diags.is_empty(),
            "the callback's `return x * 3;` pins its own return type to \
             `int` exactly like a trailing tail expression would: {diags:?}"
        );
    }

    #[test]
    fn native_lambda_bound_local_takes_its_own_fn_type() {
        let diags = native_strict_diags(
            "fn lambda_let(): int {\n  let f = |x| x + 1;\n  return f(1);\n}\n",
        );
        assert!(
            diags.is_empty(),
            "`f`'s own inferred type is `fn(int): int` (from `x + 1`'s body \
             alone), not `Unknown` — `docs/typed-mode-spec.md` §3: a \
             lambda-bound local takes the lambda's own `fn(T…): R` type: \
             {diags:?}"
        );
    }

    #[test]
    fn native_lambda_temp_shadowing_an_enclosing_local_does_not_poison_the_lambda_result() {
        // A regression this fix's own `self.locals` shadow had to grow to
        // cover: `g`'s `let a = "str";` reuses the enclosing `a`'s bare
        // name. Before extending the shadow past just params (issue #1910
        // review), the lambda's *first* `TempDecl` write of "a" `unify`d
        // with the enclosing `a: int`'s already-accumulated type —
        // `unify(int, string) == Conflicted` — and that `Conflicted` value,
        // now read back as this lambda's own tail type, made `map`'s whole
        // result (and so `scaled`'s return) `Conflicted` under strict.
        let diags = native_strict_diags(
            "fn scaled() {\n  let a = 1;\n  let items = [1, 2, 3];\n  let scaled = map(items, |x| {\n    let a = \"str\";\n    a\n  });\n  return len(scaled);\n}\n",
        );
        // Issue #1770: the lambda's own param `x` is never referenced
        // anywhere in its body (`{ let a = "str"; a }` only ever reads its
        // own fresh `a`), so it now correctly escapes as `E065` in its own
        // right — a genuinely new, unrelated finding. What this test still
        // pins is that `a` itself stays clean: `classify(String)` is
        // `Escape::Clean`, so the lambda's own shadowing `let a = "str";`
        // contributes no diagnostic of its own, and — the actual
        // regression this test guards — no `E066` appears anywhere (the
        // corruption this fixture was written to catch).
        assert_eq!(
            diags.len(),
            1,
            "only the lambda's own unused param `x` should escape — the \
             lambda's own `let a = \"str\";` is a fresh binding, wholly \
             unrelated to the enclosing `let a = 1;` of the same name, and \
             must not corrupt the lambda's own inferred `string` return \
             into `Conflicted`: {diags:?}"
        );
        assert_eq!(diags[0].code, DiagnosticCode::E065);
        assert!(
            diags[0].message.contains("lambda parameter `x`"),
            "{diags:?}"
        );
    }

    #[test]
    fn native_lambda_param_does_not_inherit_an_enclosing_annotated_local_of_the_same_name() {
        // Regression (review follow-up on #1910): the `self.locals` shadow
        // `infer_lambda` grew is not the only bare-name-keyed map read
        // during the body walk — `self.annotated` is a second one, read
        // through `own_annotation`'s bare-single-segment fallback (used by
        // `or_own_annotation` for every intrinsic argument, and by
        // `annotated_callee_ty` for a direct-call callee). The enclosing
        // `let x: string = "s";` ascribes `x` in `self.annotated`; without
        // shadowing that map too, the lambda's own unannotated param `x`
        // (bound to an `int` from `items`) read the enclosing ascription
        // back through `own_annotation` and disagreed with the annotated
        // return type — `E063 annotated type Array<Option<int>> disagrees
        // with the type inferred from usage (Array<Option<string>>)`.
        let diags = native_strict_diags(
            "fn f(): Array<Option<int>> {\n  let x: string = \"s\";\n  let items = [1, 2, 3];\n  return map(items, |x| some(x));\n}\n",
        );
        // Issue #1770: the lambda's own param `x` is now visible to strict
        // inference in its own right — `some(x)` places no constraint on
        // `x`'s own type (mono-HM narrowing from a verb's own call site is
        // not modeled, `infer_lambda`'s own doc), so it correctly escapes
        // as `E065` on its own. What this test still pins is the *absence*
        // of the regression it was written for: no `E063` disagreement
        // between the annotated return type and a wrongly-inherited
        // `string` (the enclosing, unrelated `let x: string`'s type).
        assert_eq!(
            diags.len(),
            1,
            "only the lambda's own unconstrained param `x` should escape — \
             it must not inherit the enclosing `let x: string`'s annotated \
             type merely because they share a bare name: {diags:?}"
        );
        assert_eq!(diags[0].code, DiagnosticCode::E065);
        assert!(
            diags[0].message.contains("lambda parameter `x`"),
            "{diags:?}"
        );
    }

    #[test]
    fn native_fold_accumulator_is_not_poisoned_by_an_unrelated_dotted_field_read() {
        // Regression (second review follow-up on #1910, issue #1924's gap):
        // `InferPass::infer_path` mistypes a captured struct's dotted field
        // read (`p.x`) as the struct itself, for lack of a static
        // field-type table (#1924). Before this guard, that wrong `Struct`
        // value reached `fold`'s own accumulator through an ordinary
        // `unify`/`observe` — `p.x + a` joins `Struct(Point)` with `a`
        // (`Unknown`, the identity), so `a`'s own narrowed type became
        // `Struct(Point)` too, and `fold`'s arm trusted it as the
        // accumulator's real type (never `Unknown`, so no seed fallback).
        // `g`'s own `: int` return annotation then disagreed with it —
        // `E063 annotated type int disagrees with the type inferred from
        // usage (Point)` — regressing code this exact shape compiled clean
        // under `main` before #1910, with no source-level workaround.
        let diags = native_strict_diags(
            "struct Point {\n  x: int,\n  y: int\n}\n\nfn g(): int {\n  let p = Point { x: 3, y: 4 };\n  let items = [1, 2];\n  return fold(items, 0, |a, b| p.x + a + b);\n}\n",
        );
        // Issue #1770: `a`/`b` now get their own per-lambda escape-check
        // frame. The dotted-field-read taint guard above (#1924's own
        // follow-up fix) makes `infer_lambda` fall back to an honest
        // `Unknown` for this callback's own signature rather than trust the
        // mistyped `Struct(Point)` value — so `a`/`b` correctly escape as
        // `E065` in their own right. What this test still pins is the
        // *absence* of the regression it was written for: no `E063`
        // disagreement between `g`'s `: int` return annotation and a
        // wrongly-poisoned `Point`.
        assert_eq!(
            diags.len(),
            2,
            "only the lambda's own two params should escape — `p.x`'s own \
             mistyped read must not poison `g`'s own `: int` return \
             annotation, which has nothing to do with `p`'s own struct \
             type: {diags:?}"
        );
        assert!(
            diags.iter().all(|d| d.code == DiagnosticCode::E065
                && (d.message.contains("lambda parameter `a`")
                    || d.message.contains("lambda parameter `b`"))),
            "{diags:?}"
        );
    }

    #[test]
    fn native_verb_callback_param_still_escapes_when_the_body_places_no_constraint_on_it() {
        // The boundary this fix does NOT cross: `infer_lambda`'s own doc
        // ("mono-HM narrowing of a lambda's own params from its concrete
        // call sites is not modeled in this slice") — `scaled`'s callback
        // multiplies `x` by a captured, itself-unconstrained `factor`, so
        // neither ever resolves. This is `tests/tier1-native/lambda-verbs/
        // story.brink`'s `scaled` reduced to its essential shape, still an
        // expected (not #1910-fixed) baseline row.
        let diags = native_strict_diags(
            "fn scaled(factor) {\n  let items = [1, 2, 3];\n  return map(items, |x| x * factor);\n}\n",
        );
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E065),
            "`factor` is never pinned by any use anywhere in `scaled`'s own \
             body (call-site-driven inference is forbidden by \
             `docs/typed-mode-spec.md` §2), so both `factor` and the return \
             type must still escape: {diags:?}"
        );
    }

    // ── issue #1941: a lambda's value-position read of an annotated ──
    // param still typed Unknown — the structurally parallel gap #1938 left
    // for `infer_return`'s fn-return position. `infer_lambda`'s tail
    // (`LambdaBody::Block`) and sole expression (`LambdaBody::Expr`) are
    // both a lambda's own value position, exactly like a `return`, and now
    // run through the same `or_own_annotation` read-site fallback.

    #[test]
    fn native_lambda_tail_reading_a_content_param_takes_its_annotated_type() {
        // Issue #1941's own reduction, both halves: the lambda-return-
        // annotated twin was already clean (`infer_lambda`'s own
        // `l.return_type` overlay — the same firewall shape the fn case had
        // before #1938), the bare one let the lambda's own `Unknown` return
        // type escape through the enclosing temp `g`.
        let bare = native_strict_diags("fn f() {\n  let g = |t: content| {\n    t\n  };\n}\n");
        assert!(
            bare.is_empty(),
            "`t: content`'s tail read supplies the lambda's own return type: {bare:?}"
        );
        let annotated =
            native_strict_diags("fn f() {\n  let g = |t: content|: content {\n    t\n  };\n}\n");
        assert!(
            annotated.is_empty(),
            "the lambda-return-annotated twin stays clean: {annotated:?}"
        );
    }

    #[test]
    fn native_lambda_tail_reading_an_annotated_param_is_not_content_specific() {
        // All four leaf spellings, mirroring #1938's own
        // `native_returning_an_annotated_param_is_not_content_specific` — a
        // fix that only special-cased `Ty::Content` would fail here.
        for ty in ["int", "float", "bool", "string"] {
            let src = format!("fn f() {{\n  let g = |t: {ty}| {{\n    t\n  }};\n}}\n");
            let diags = native_strict_diags(&src);
            assert!(
                diags.is_empty(),
                "`t: {ty}`'s tail read supplies the lambda's own return type: {diags:?}"
            );
        }
    }

    #[test]
    fn native_lambda_expr_body_reading_an_annotated_param_exports_its_declared_type() {
        // The expression-bodied twin (`|t: content| t`, no braces) — the
        // *other* value-position read site #1941 fixed
        // (`LambdaBody::Expr`), a structurally distinct code path from the
        // block-tail arm above (`infer_lambda`'s own `match` on `l.body`).
        let diags = native_strict_diags("fn f() {\n  let g = |t: content| t;\n}\n");
        assert!(
            diags.is_empty(),
            "an expression-bodied lambda's sole expression is its value \
             position, exactly like a block's tail: {diags:?}"
        );
    }

    #[test]
    fn native_lambda_param_annotation_seed_does_not_leak_into_a_rebound_temp_of_the_same_name() {
        // #1954 review finding (BLOCKING): `check_declared_assign_target`'s
        // own `SymbolKind::Temp` arm reads the same bare-name-keyed
        // `self.annotated` map the #1941 seed populates — it is a mismatch
        // *reporter*, not a pure read site like `own_annotation`'s other
        // consumers, and it cannot distinguish "the param's own annotation"
        // from "a fresh same-named local's own (absent) annotation". Without
        // excluding a body-rebound name from the seed, `t`'s param
        // annotation (`int`) leaked into the *lambda-local* `t` this body
        // re-declares, so assigning it a `string` falsely reported
        // `` `t` has type `string` but its declared type is `int` ``
        // (E063) even though the local `t` was never declared `: int` at
        // all. Verified empirically before this fix: reverting the
        // `body_bound_names` exclusion in `infer_lambda` reproduces this
        // exact diagnostic on this exact snippet.
        let diags = native_strict_diags(
            "fn f() {\n  let g = |t: int| {\n    let t = \"a\";\n    t = \"b\";\n    t\n  };\n}\n",
        );
        assert!(
            diags.is_empty(),
            "the lambda body's own `t` re-declaration shadows the param \
             entirely; it has no `int` annotation of its own to conflict \
             with a `string` assignment: {diags:?}"
        );
    }

    #[test]
    fn native_lambda_param_annotation_seed_reaches_every_own_annotation_read_site_in_the_body() {
        // #1954 review finding: the #1941 seed's blast radius is wider than
        // the PR's own description states — it is read by
        // `own_annotation`'s bare-name fallback at *every*
        // `or_own_annotation`/`annotated_callee_ty` consumer reachable
        // during the body walk, not only the tail/expr value position. This
        // mirrors a `fn`/`flow`'s own `new_pass`-time seed, which already
        // covers its whole body, not only its `return`s — see
        // `docs/typed-mode-spec.md` §2's #1941 paragraph for the recorded
        // scope.
        //
        // `some(t)`: `t`'s annotated `int` reaches the intrinsic-argument
        // overlay (`infer_intrinsic_call`'s `or_own_annotation` pass over
        // each argument), which is what lets the tail's `Ty::Option(Int)`
        // resolve at all instead of escaping as `Unknown`.
        let via_intrinsic_arg =
            native_strict_diags("fn f() {\n  let g = |t: int| {\n    some(t)\n  };\n}\n");
        assert!(
            via_intrinsic_arg.is_empty(),
            "the seed reaches `some`'s argument-position read of `t`, not \
             just the lambda's own tail: {via_intrinsic_arg:?}"
        );

        // `cb(1)`: `cb`'s annotated `fn(int): int` reaches
        // `annotated_callee_ty`'s direct-call-callee read, which is what
        // lets `cb` be called as a function value here rather than
        // escaping strict inference.
        let via_callee_ty =
            native_strict_diags("fn f() {\n  let g = |cb: fn(int): int| {\n    cb(1)\n  };\n}\n");
        assert!(
            via_callee_ty.is_empty(),
            "the seed reaches `annotated_callee_ty`'s direct-call read of \
             `cb`, not just the lambda's own tail: {via_callee_ty:?}"
        );
    }

    // ── issue #1994: a lambda's own written annotation governs, with an ──
    // eager E174 on disagreement — exercised through `native_strict_diags`,
    // the same end-to-end harness the #1941/#1954 tests immediately above
    // use, not just at the `infer_lambda` unit level (review finding on
    // #1994: the hand-built HIR unit tests in `infer::body::tests` never
    // reach `strict::check_lambda_annotation_mismatches` at all).

    #[test]
    fn native_lambda_return_annotation_disagreement_is_e174() {
        let diags =
            native_strict_diags("fn f() {\n  let g = |k: int|: int {\n    \"wrong\"\n  };\n}\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E174);
        assert!(
            diags[0]
                .message
                .contains("lambda return type is annotated `int` but its body infers `string`"),
            "{:?}",
            diags[0].message
        );
    }

    #[test]
    fn native_lambda_param_annotation_disagreement_is_e174() {
        // The param-arm twin of the return test above: `k`'s only body
        // evidence (`k == true`, an expression-bodied tail so `g` itself
        // resolves to a concrete `Ty::Fn` rather than separately escaping)
        // pins it to `bool`, disagreeing with its own written `k: int`.
        // `int` vs `bool` is irreconcilable in either direction, so this
        // must still fire even with the widening-only guard in place.
        let diags = native_strict_diags("fn f() {\n  let g = |k: int| k == true;\n}\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E174);
        assert!(
            diags[0]
                .message
                .contains("lambda parameter `k` is annotated `int` but its body infers `bool`"),
            "{:?}",
            diags[0].message
        );
    }

    #[test]
    fn native_lambda_param_widening_use_is_not_a_mismatch() {
        // Review finding (BLOCKING) on #1994: the param arm's original
        // `!assignable(&declared_ty, &inferred)` check compared in the
        // wrong direction for a parameter, turning legal int→float widening
        // into a hard, non-downgradable E174. An `int`-annotated param used
        // as a `float` in the body (ordinary numeric widening, exactly like
        // the structurally identical `fn f(x: int): float { return x +
        // 1.0; }` — which reports nothing under the pre-existing top-level
        // `fn`/`flow` posture) must stay clean.
        let diags = native_strict_diags("fn f() {\n  let g = |x: int| {\n    x + 1.0\n  };\n}\n");
        assert!(
            diags.is_empty(),
            "an int-annotated param used as a float is legal widening, not \
             a mismatch: {diags:?}"
        );
    }

    // ── issue #1551: return-escape check extended past `is_function` ──
    //
    // `docs/decision-log.md` 2026-07-22 implicit-end ruling item 3: "a flow
    // that declares a return type must produce a value... falling through
    // without a value is a checker error" — these prove the checker
    // diagnostic that ruling promised (deferred at the time, per
    // `hir::lower_native::container`'s "not built by this slice" comment)
    // now fires, for both a top-level flow (knot) and a nested flow
    // (stitch), and that it is a distinct code (`E150`) from Unknown-escape
    // (`E065`) — the annotation-fallback in `infer::body::infer_def_body`
    // backfills a no-return body's inferred type from the declared
    // annotation, so E065's `Ty`-based classification structurally cannot
    // see a missing return (it comes out `Clean`).

    #[test]
    fn native_value_returning_knot_falling_through_is_e150() {
        // Mirrors `a_return_typed_flow_does_not_get_the_implicit_done` in
        // `brink-ir`'s own lowering tests — same fixture, now checked.
        let diags = native_strict_diags("flow quest(): int {\n  Onward.\n}\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E150);
        assert!(
            diags[0].message.contains("never returns a value"),
            "{:?}",
            diags[0].message
        );
    }

    #[test]
    fn native_value_returning_nested_stitch_falling_through_is_e150() {
        // Mirrors `a_return_typed_stitch_does_not_get_the_implicit_done`.
        let diags =
            native_strict_diags("flow garden() {\n  flow gate(): int {\n    Creak.\n  }\n}\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E150);
    }

    #[test]
    fn native_value_returning_knot_that_always_returns_is_clean() {
        // The flip side of the falling-through cases above: a value-typed
        // flow whose body actually returns a concrete, resolvable value
        // gets no E150 (has_value_return is true) and is checked as an
        // ordinary Unknown/Conflicted escape instead — clean here since
        // `int` is concrete.
        let diags = native_strict_diags("flow quest(): int ~{\n  return 5;\n}\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn native_value_returning_nested_stitch_that_always_returns_is_clean() {
        let diags =
            native_strict_diags("flow garden() {\n  flow gate(): int ~{\n    return 5;\n  }\n}\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn native_value_returning_knot_with_unresolvable_return_still_escapes_as_unknown() {
        // A value is returned (has_value_return = true), so this goes
        // through the ordinary escape check, not E150 — an unconstrained
        // param's value flowing straight out is a genuine Unknown-escape.
        let diags = native_strict_diags("flow quest(x): int ~{\n  return x;\n}\n");
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E065),
            "{diags:?}"
        );
    }

    #[test]
    fn native_void_annotated_knot_falling_through_is_exempt_from_e150() {
        // `: void` reads as "no return type" for the fall-through check on
        // a flow, same as it does for a `fn` — never E150.
        let diags = native_strict_diags("flow quest(): void {\n  Onward.\n}\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn native_plain_knot_and_stitch_with_no_return_type_stay_unchecked() {
        // Baseline: no declared return type at all (and not `is_function`)
        // — no return-value concept, exactly as before #1551.
        let diags = native_strict_diags("flow quest() {\n  Onward.\n}\n");
        assert!(diags.is_empty(), "{diags:?}");
        let nested_diags =
            native_strict_diags("flow garden() {\n  flow gate() {\n    Creak.\n  }\n}\n");
        assert!(nested_diags.is_empty(), "{nested_diags:?}");
    }

    #[test]
    fn native_annotated_function_falling_through_is_e150_latent_bug_fix() {
        // A pre-existing gap in the `is_function` case itself (found while
        // fixing #1551): before this fix, `fn f(): int { … no return … }`
        // inferred `is_void = true` via the old blanket
        // `!has_value_return` short-circuit and skipped checking
        // entirely — silently accepting a declared `int` that the body
        // never produces. Now a declared, non-void return type on a
        // no-return function is E150 too, the same as a flow/stitch.
        let diags = native_strict_diags("fn noop(): int {\n  let x = 1;\n}\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E150);
    }

    #[test]
    fn native_void_annotated_def_that_actually_returns_a_value_is_exempt() {
        // Regression guard (review finding on #1556): a `: void`-annotated
        // def whose body *does* carry a value-returning `return <expr>` —
        // a body/annotation mismatch, not covered by this checker — must
        // not fall into the Unknown-escape branch just because
        // `has_value_return` is true. `void` reads as "no return type" for
        // escape purposes on both a `fn` and a flow/stitch, so the two
        // must agree: neither emits anything here (in particular, no
        // spurious second `E065` alongside whatever caught the param).
        // The param is annotated (exempt from its own Unknown-escape) so
        // any diagnostic here can only be the spurious return-type check.
        let function_diags = native_strict_diags("fn f(x: int): void {\n  return x;\n}\n");
        assert!(
            function_diags.is_empty(),
            "a void-annotated fn's own return value must not escape-check: {function_diags:?}"
        );
        let flow_diags = native_strict_diags("flow gate(x: int): void ~{\n  return x;\n}\n");
        assert!(
            flow_diags.is_empty(),
            "the flow/stitch twin must agree with the fn case: {flow_diags:?}"
        );
    }

    #[test]
    fn native_value_returning_knot_with_a_partial_return_path_is_undocumented_gap() {
        // Pins the currently-undecided partial-path behavior the E150
        // message reword (review finding on #1556) made explicit: E150 only
        // fires when the body carries *no* value-returning `return`
        // anywhere (`has_value_return == false`). A body that returns a
        // value on *some* paths but can also fall through another (the
        // `else`-less `if` here) has `has_value_return == true`, so it
        // takes the ordinary escape-check branch instead — no E150, no
        // fall-through diagnostic of any kind, even though the `else` path
        // still falls through without a value. This is a known, documented
        // gap (#1551 asked to "decide (and document)" this shape; the
        // decision is deferred), not a fixed contract — this test exists
        // so a future change to that decision has to touch it deliberately.
        let diags =
            native_strict_diags("flow quest(): int ~{\n  if true {\n    return 1;\n  }\n}\n");
        assert!(
            diags.is_empty(),
            "partial-path fall-through is not currently detected: {diags:?}"
        );
    }

    #[test]
    fn handle_param_escapes_as_unknown_with_no_manifest_registered() {
        let (hir, index, res) = build("=== noop(x: Handle<AudioInstance>) ===\nHello.\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E065);
    }

    /// T1d-2b (issue #774, docs/t1d-spec.md §3 — the #767 acceptance
    /// criterion): "binding declared Handle<AudioInstance> rejects
    /// Handle<Timer> at compile time". `get_audio`/`get_timer` are leaf
    /// functions whose return type is annotated with a distinct handle
    /// kind each — their body-derived return type stays `Unknown` (an
    /// unregistered `EXTERNAL`'s result is untyped and unchecked, see
    /// [`external_binding_with_unregistered_name_is_unchecked`]), so the
    /// T1c annotation-firewall overlay supplies the concrete
    /// `Ty::Handle(K)`. That opaque producer is what these fixtures need:
    /// a handle is an opaque `{kind, id}` scalar (docs/t1d-spec.md §3), not
    /// an `int`, so the `~ return id` these bodies used to carry was a real
    /// type error that only passed because reading an annotated param as a
    /// value typed `Unknown` — the gap issue #1912 closed. `main`'s temps
    /// `a`/`b` pick
    /// those return types up purely through call-site inference (never an
    /// annotation of their own), then get compared — a genuine cross-kind
    /// handle mismatch detected *purely from body-usage inference*, exactly
    /// the gap PR #769 disclosed as deferred. Before T1d-2b threaded the
    /// manifest into `infer_project`/`solve_scc`, `Handle<K>` annotations
    /// never resolved during body inference at all (an empty kind set), so
    /// `get_audio`/`get_timer` would return `Ty::Unknown`, `unify` would
    /// never see two distinct `Ty::Handle` kinds meet, and this mismatch
    /// was silently unreachable — this test is the positive case proving
    /// it is now reachable end-to-end.
    #[test]
    fn cross_kind_handle_comparison_from_body_usage_is_conflicted_under_strict() {
        // `spawn_audio`/`spawn_timer` are genuinely-registered `EXTERNAL`
        // producers (issue #1942's Scope section proposes "a
        // natively-registered producer" as one construction path): each
        // declares a fixed `returns` naming its own `Handle`-based
        // `SemanticTypeDef`, so `get_audio`/`get_timer`'s body-derived
        // return resolves directly to the concrete `Ty::Handle(K)` —
        // replacing the earlier *unregistered* `opaque_handle` workaround
        // (PR #1938) whose result was untyped and relied on the
        // annotation-firewall overlay alone.
        let src = "EXTERNAL spawn_audio()\nEXTERNAL spawn_timer()\n\
=== function get_audio(): Handle<AudioInstance> ===\n~ return spawn_audio()\n\
=== function get_timer(): Handle<Timer> ===\n~ return spawn_timer()\n\
=== main ===\n~ temp a = get_audio()\n~ temp b = get_timer()\n{a == b:\n  ok\n}\n-> DONE\n";
        let (hir, index, res) = build(src);
        let manifest = brink_ir::HostManifest {
            markup: Vec::new(),
            types: vec![
                brink_ir::SemanticTypeDef {
                    name: "AudioInstance".to_string(),
                    base: brink_ir::BaseType::Handle,
                    constraint: None,
                    values: None,
                    widget: None,
                },
                brink_ir::SemanticTypeDef {
                    name: "Timer".to_string(),
                    base: brink_ir::BaseType::Handle,
                    constraint: None,
                    values: None,
                    widget: None,
                },
            ],
            externals: vec![
                brink_ir::ManifestExternal {
                    name: "spawn_audio".to_string(),
                    params: Vec::new(),
                    returns: brink_ir::TypeRef("AudioInstance".to_string()),
                    kind: brink_ir::ExternalKind::default(),
                    doc: None,
                    widgets: Vec::new(),
                    path: Vec::new(),
                },
                brink_ir::ManifestExternal {
                    name: "spawn_timer".to_string(),
                    params: Vec::new(),
                    returns: brink_ir::TypeRef("Timer".to_string()),
                    kind: brink_ir::ExternalKind::default(),
                    doc: None,
                    widgets: Vec::new(),
                    path: Vec::new(),
                },
            ],
        };
        let inference = crate::infer_project(
            &[(FileId(0), &hir)],
            &index,
            &res,
            Some(&manifest),
            &BTreeMap::new(),
        );
        let diags = check(
            &[(FileId(0), &hir)],
            &index,
            &inference,
            &res,
            Some(&manifest),
        );
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E066 && d.message.contains("temp `a`")),
            "cross-kind handle comparison must Conflicted-escape temp `a`: {diags:?}"
        );
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E066 && d.message.contains("temp `b`")),
            "cross-kind handle comparison must Conflicted-escape temp `b`: {diags:?}"
        );
        assert!(
            diags.iter().all(|d| d.code == DiagnosticCode::E066),
            "no other diagnostic code expected: {diags:?}"
        );
    }

    /// Negative counterpart: two locals of the *same* declared handle kind
    /// compared against each other unify cleanly (`unify(Handle(k),
    /// Handle(k)) == Handle(k)`, the T1d-2 lattice ruling) — no escape.
    #[test]
    fn same_kind_handle_comparison_from_body_usage_is_clean_under_strict() {
        // `spawn_audio` is a genuinely-registered `EXTERNAL` producer
        // (issue #1942) — see the sibling test above for the full
        // rationale.
        let src = "EXTERNAL spawn_audio()\n\
=== function get_audio(): Handle<AudioInstance> ===\n~ return spawn_audio()\n\
=== function get_audio2(): Handle<AudioInstance> ===\n~ return spawn_audio()\n\
=== main ===\n~ temp a = get_audio()\n~ temp c = get_audio2()\n{a == c:\n  ok\n}\n-> DONE\n";
        let (hir, index, res) = build(src);
        let manifest = brink_ir::HostManifest {
            markup: Vec::new(),
            types: vec![brink_ir::SemanticTypeDef {
                name: "AudioInstance".to_string(),
                base: brink_ir::BaseType::Handle,
                constraint: None,
                values: None,
                widget: None,
            }],
            externals: vec![brink_ir::ManifestExternal {
                name: "spawn_audio".to_string(),
                params: Vec::new(),
                returns: brink_ir::TypeRef("AudioInstance".to_string()),
                kind: brink_ir::ExternalKind::default(),
                doc: None,
                widgets: Vec::new(),
                path: Vec::new(),
            }],
        };
        let inference = crate::infer_project(
            &[(FileId(0), &hir)],
            &index,
            &res,
            Some(&manifest),
            &BTreeMap::new(),
        );
        let diags = check(
            &[(FileId(0), &hir)],
            &index,
            &inference,
            &res,
            Some(&manifest),
        );
        assert!(
            diags.is_empty(),
            "same-kind comparison must not escape: {diags:?}"
        );
    }

    /// Issue #994: a dotted field read on a `Struct`-typed temp (`t.x`) must
    /// not corrupt `t`'s own accumulated type with the field-read's usage
    /// context. Before the fix, `infer::body::InferPass::observe` joined
    /// `useInt`'s `int` param type into temp `t`'s own slot (the TM-4b
    /// resolution fallback maps the whole dotted path's range to `t`'s
    /// `DefinitionId` — no static field-type table exists yet, so `t.x` and
    /// bare `t` were indistinguishable to `observe`), producing
    /// `unify(Struct(Point), int) == Conflicted` and a spurious `E066` on
    /// `t` even though `t` itself — a `Point` — is never actually misused.
    #[test]
    fn temp_headed_dotted_field_read_does_not_corrupt_the_temp_s_own_type() {
        let src = "STRUCT Point = #{x: float}\n\
                   === function useInt(n: int): int ===\n~ return n\n\
                   === main ===\n~ temp t = Point#{x: 1.0}\n~ temp r = useInt(t.x)\n-> DONE\n";
        let (hir, index, res) = build(src);
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E066),
            "a dotted field read must never Conflicted-escape its head temp: {diags:?}"
        );
    }

    /// Control for the #994 fix above: the segment-count guard in `observe`
    /// only exempts a *dotted* field read — a bare (single-segment) temp
    /// whose own uses genuinely disagree must still Conflicted-escape.
    #[test]
    fn bare_temp_with_genuinely_conflicting_uses_still_escapes_as_conflicted() {
        let src = "=== function useInt(n: int): int ===\n~ return n\n\
                   === main ===\n~ temp t = \"hello\"\n~ temp r = useInt(t)\n-> DONE\n";
        let (hir, index, res) = build(src);
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E066 && d.message.contains("temp `t`")),
            "a bare temp with genuinely conflicting uses must still Conflicted-escape: {diags:?}"
        );
    }

    /// Before T1d-2b (issue #774), this exact cross-kind fixture was
    /// silently unreachable: `infer_project`/`solve_scc` had no manifest
    /// seam, so `Handle<K>` return annotations never resolved during body
    /// inference and both `get_audio`/`get_timer` returned `Ty::Unknown`
    /// instead of their distinct handle kinds — `unify(Unknown, Unknown)`
    /// stays `Unknown`, never `Conflicted`, so no escape ever fired even
    /// with a registered manifest. Pinned here as the regression guard for
    /// the specific "manifest reaches inference, not just `signature()`"
    /// gap PR #769 disclosed.
    #[test]
    fn cross_kind_handle_mismatch_is_unreachable_without_manifest_reaching_inference() {
        // `spawn_audio`/`spawn_timer` are genuinely-registered `EXTERNAL`
        // producers (issue #1942) — but the point of *this* test is that
        // `infer_project` below is handed `None`, so it never sees this
        // manifest at all: a registered producer's declared `returns` is
        // exactly as unresolvable as an unregistered external's absent one
        // when the manifest never reaches `infer_project`'s own
        // `ProjectCtx`. That is the pre-#774 gap this regression guards.
        let src = "EXTERNAL spawn_audio()\nEXTERNAL spawn_timer()\n\
=== function get_audio(): Handle<AudioInstance> ===\n~ return spawn_audio()\n\
=== function get_timer(): Handle<Timer> ===\n~ return spawn_timer()\n\
=== main ===\n~ temp a = get_audio()\n~ temp b = get_timer()\n{a == b:\n  ok\n}\n-> DONE\n";
        let (hir, index, res) = build(src);
        let manifest = brink_ir::HostManifest {
            markup: Vec::new(),
            types: vec![
                brink_ir::SemanticTypeDef {
                    name: "AudioInstance".to_string(),
                    base: brink_ir::BaseType::Handle,
                    constraint: None,
                    values: None,
                    widget: None,
                },
                brink_ir::SemanticTypeDef {
                    name: "Timer".to_string(),
                    base: brink_ir::BaseType::Handle,
                    constraint: None,
                    values: None,
                    widget: None,
                },
            ],
            externals: vec![
                brink_ir::ManifestExternal {
                    name: "spawn_audio".to_string(),
                    params: Vec::new(),
                    returns: brink_ir::TypeRef("AudioInstance".to_string()),
                    kind: brink_ir::ExternalKind::default(),
                    doc: None,
                    widgets: Vec::new(),
                    path: Vec::new(),
                },
                brink_ir::ManifestExternal {
                    name: "spawn_timer".to_string(),
                    params: Vec::new(),
                    returns: brink_ir::TypeRef("Timer".to_string()),
                    kind: brink_ir::ExternalKind::default(),
                    doc: None,
                    widgets: Vec::new(),
                    path: Vec::new(),
                },
            ],
        };
        // Manifest reaches `check()`'s own annotation resolution (the
        // pre-existing T1d-2 exemption seam), but `infer_project` here gets
        // `None` — simulating the pre-#774 gap where the manifest stopped
        // at `signature()`/the annotation firewall and never reached body
        // inference. `get_audio`/`get_timer`'s return types both come back
        // `Ty::Unknown` (the annotation can't resolve without the manifest
        // reaching `infer_project`'s own `ProjectCtx`), and `a`/`b`
        // inherit `Unknown`, which `check_escapes` reports as `E065`
        // (Unknown-escape), never `E066` (Conflicted) — proving the two
        // codes are genuinely distinguishing "never resolved" from "a real
        // kind mismatch", not interchangeable.
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(
            &[(FileId(0), &hir)],
            &index,
            &inference,
            &res,
            Some(&manifest),
        );
        assert!(
            diags.iter().all(|d| d.code == DiagnosticCode::E065),
            "with no manifest reaching inference, temps escape as Unknown, not Conflicted: {diags:?}"
        );
        assert!(
            !diags.iter().any(|d| d.code == DiagnosticCode::E066),
            "a real cross-kind mismatch must never be reachable without T1d-2b's fix: {diags:?}"
        );
    }

    // ─── `EXTERNAL` call-site argument checking (issue #786) ────────────
    //
    // docs/t1d-spec.md §3's own acceptance criterion: "under `types =
    // strict`, a binding declared to take `Handle<AudioInstance>` rejects a
    // `Handle<Timer>` argument at compile time". T1d-2b (#774) closed this
    // for two *locals* meeting through body-usage inference (comparison,
    // reassignment); this closes the literal reading of the sentence — the
    // binding itself, at its own call site — reusing the identical
    // `known_sigs`/`observe`/`Ty::Conflicted`/`E066` machinery, no parallel
    // checking surface (see `infer::collect_external_sigs`'s doc).

    fn audio_and_timer_manifest(play_sound_param_kind: &str) -> brink_ir::HostManifest {
        brink_ir::HostManifest {
            markup: Vec::new(),
            types: vec![
                brink_ir::SemanticTypeDef {
                    name: "AudioInstance".to_string(),
                    base: brink_ir::BaseType::Handle,
                    constraint: None,
                    values: None,
                    widget: None,
                },
                brink_ir::SemanticTypeDef {
                    name: "Timer".to_string(),
                    base: brink_ir::BaseType::Handle,
                    constraint: None,
                    values: None,
                    widget: None,
                },
            ],
            externals: vec![
                brink_ir::ManifestExternal {
                    name: "play_sound".to_string(),
                    params: vec![brink_ir::ManifestParam {
                        name: "inst".to_string(),
                        ty: brink_ir::TypeRef(play_sound_param_kind.to_string()),
                    }],
                    returns: brink_ir::TypeRef::default(),
                    kind: brink_ir::ExternalKind::default(),
                    doc: None,
                    widgets: Vec::new(),
                    path: Vec::new(),
                },
                // Genuinely-registered `EXTERNAL` producers (issue #1942's
                // Scope section proposes "a natively-registered producer"
                // as one construction path), replacing the earlier
                // *unregistered* `opaque_handle` workaround (PR #1938) the
                // tests below used to manufacture a handle value.
                brink_ir::ManifestExternal {
                    name: "spawn_audio".to_string(),
                    params: Vec::new(),
                    returns: brink_ir::TypeRef("AudioInstance".to_string()),
                    kind: brink_ir::ExternalKind::default(),
                    doc: None,
                    widgets: Vec::new(),
                    path: Vec::new(),
                },
                brink_ir::ManifestExternal {
                    name: "spawn_timer".to_string(),
                    params: Vec::new(),
                    returns: brink_ir::TypeRef("Timer".to_string()),
                    kind: brink_ir::ExternalKind::default(),
                    doc: None,
                    widgets: Vec::new(),
                    path: Vec::new(),
                },
            ],
        }
    }

    /// The #767/#786 acceptance criterion, literally: a binding
    /// (`EXTERNAL play_sound`) declared in the manifest to take
    /// `Handle<AudioInstance>` rejects a `Handle<Timer>`-kinded argument at
    /// compile time under `types = strict`.
    #[test]
    fn external_binding_rejects_cross_kind_handle_argument_under_strict() {
        let src = "EXTERNAL play_sound(inst)\nEXTERNAL spawn_timer()\n\
=== function get_timer(): Handle<Timer> ===\n~ return spawn_timer()\n\
=== main ===\n~ temp t = get_timer()\n~ play_sound(t)\n-> DONE\n";
        let (hir, index, res) = build(src);
        let manifest = audio_and_timer_manifest("AudioInstance");
        let inference = crate::infer_project(
            &[(FileId(0), &hir)],
            &index,
            &res,
            Some(&manifest),
            &BTreeMap::new(),
        );
        let diags = check(
            &[(FileId(0), &hir)],
            &index,
            &inference,
            &res,
            Some(&manifest),
        );
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E066 && d.message.contains("temp `t`")),
            "a Timer-kinded argument to an AudioInstance-declared binding must \
             Conflicted-escape temp `t`: {diags:?}"
        );
        assert!(
            diags.iter().all(|d| d.code == DiagnosticCode::E066),
            "no other diagnostic code expected: {diags:?}"
        );
    }

    /// Negative counterpart: an argument of the binding's *own* declared
    /// kind is clean — no escape, matching `unify(Handle(k), Handle(k)) ==
    /// Handle(k)`.
    #[test]
    fn external_binding_accepts_same_kind_handle_argument_under_strict() {
        let src = "EXTERNAL play_sound(inst)\nEXTERNAL spawn_audio()\n\
=== function get_audio(): Handle<AudioInstance> ===\n~ return spawn_audio()\n\
=== main ===\n~ temp t = get_audio()\n~ play_sound(t)\n-> DONE\n";
        let (hir, index, res) = build(src);
        let manifest = audio_and_timer_manifest("AudioInstance");
        let inference = crate::infer_project(
            &[(FileId(0), &hir)],
            &index,
            &res,
            Some(&manifest),
            &BTreeMap::new(),
        );
        let diags = check(
            &[(FileId(0), &hir)],
            &index,
            &inference,
            &res,
            Some(&manifest),
        );
        assert!(
            diags.is_empty(),
            "same-kind binding argument must not escape: {diags:?}"
        );
    }

    /// Gradual mode is unaffected: `strict_diagnostics` never even calls
    /// `infer_project`/`check` when `types = gradual` (this module's own
    /// `check` is only ever reached under strict), so a cross-kind argument
    /// to the same binding produces no compile-time diagnostic at all under
    /// gradual — the existing runtime fault at the binding boundary (T1d
    /// spec §3's own "under gradual, kind mismatch is a runtime fault"
    /// posture) stays the only enforcement, byte-identical to before this
    /// issue.
    #[test]
    fn external_binding_cross_kind_argument_is_not_checked_under_gradual() {
        let src = "EXTERNAL play_sound(inst)\nEXTERNAL spawn_timer()\n\
=== function get_timer(): Handle<Timer> ===\n~ return spawn_timer()\n\
=== main ===\n~ temp t = get_timer()\n~ play_sound(t)\n-> DONE\n";
        let (hir, index, res) = build(src);
        let manifest = audio_and_timer_manifest("AudioInstance");
        let opts = crate::AnalysisOptions {
            host_manifest: Some(manifest),
            dialect: crate::Dialect::Brink,
            types: Some(TypePolicy::Gradual),
            ..Default::default()
        };
        let diags = crate::strict_diagnostics(
            &[(FileId(0), &hir)],
            &index,
            &res,
            &opts,
            false,
            None,
            &BTreeMap::new(),
        );
        assert!(
            diags.is_empty(),
            "gradual mode must never run the strict handle-kind check: {diags:?}"
        );
    }

    /// An `EXTERNAL` with no matching registered manifest entry contributes
    /// no checkable signature — the argument's own inferred kind (`Timer`,
    /// from `get_timer`'s registered `spawn_timer` producer, not the
    /// annotated return) stays clean, same as today (this is the disclosed
    /// inline-doc-only gap `infer::collect_external_sigs`'s doc names, not a
    /// regression).
    #[test]
    fn external_binding_with_unregistered_name_is_unchecked() {
        let src = "EXTERNAL other_call(inst)\nEXTERNAL spawn_timer()\n\
=== function get_timer(): Handle<Timer> ===\n~ return spawn_timer()\n\
=== main ===\n~ temp t = get_timer()\n~ other_call(t)\n-> DONE\n";
        let (hir, index, res) = build(src);
        let manifest = audio_and_timer_manifest("AudioInstance");
        let inference = crate::infer_project(
            &[(FileId(0), &hir)],
            &index,
            &res,
            Some(&manifest),
            &BTreeMap::new(),
        );
        let diags = check(
            &[(FileId(0), &hir)],
            &index,
            &inference,
            &res,
            Some(&manifest),
        );
        assert!(
            diags.is_empty(),
            "an unregistered external's call sites stay unchecked: {diags:?}"
        );
    }

    // ── `EXTERNAL` *declaration* escape checking (issue #1004) ───────────
    //
    // The checks above verify call *arguments* against a binding's declared
    // param types. #1004 adds the dual: the binding's own declared params are
    // escape-checked, so a manifest whose `ManifestParam.ty` fails to resolve
    // is reported rather than silently treated as an untyped call. Exercised
    // through the shared `strict_diagnostics` seam (where `check_external_escapes`
    // is wired), the exact path both the analysis and compile pipelines take.

    fn get_thing_manifest(ty: &str) -> brink_ir::HostManifest {
        brink_ir::HostManifest {
            markup: Vec::new(),
            types: vec![brink_ir::SemanticTypeDef {
                name: "thing_id".to_string(),
                base: brink_ir::BaseType::Int,
                constraint: None,
                values: None,
                widget: None,
            }],
            externals: vec![brink_ir::ManifestExternal {
                name: "get_thing".to_string(),
                params: vec![brink_ir::ManifestParam {
                    name: "id".to_string(),
                    ty: brink_ir::TypeRef(ty.to_string()),
                }],
                returns: brink_ir::TypeRef("float".to_string()),
                kind: brink_ir::ExternalKind::default(),
                doc: None,
                widgets: Vec::new(),
                path: Vec::new(),
            }],
        }
    }

    fn strict_opts(manifest: Option<brink_ir::HostManifest>) -> crate::AnalysisOptions {
        crate::AnalysisOptions {
            host_manifest: manifest,
            dialect: crate::Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..Default::default()
        }
    }

    const EXT_SRC: &str = "EXTERNAL get_thing(id)\n=== start ===\n{get_thing(1)}\n-> DONE\n";

    #[test]
    fn manifest_typed_external_param_is_clean_under_strict() {
        let (hir, index, res) = build(EXT_SRC);
        let diags = crate::strict_diagnostics(
            &[(FileId(0), &hir)],
            &index,
            &res,
            &strict_opts(Some(get_thing_manifest("thing_id"))),
            false,
            None,
            &BTreeMap::new(),
        );
        assert!(
            diags.is_empty(),
            "a manifest-typed external param must not escape: {diags:?}"
        );
    }

    #[test]
    fn unresolvable_external_param_escapes_at_its_own_decl_span() {
        let (hir, index, res) = build(EXT_SRC);
        let diags = crate::strict_diagnostics(
            &[(FileId(0), &hir)],
            &index,
            &res,
            &strict_opts(Some(get_thing_manifest(""))),
            false,
            None,
            &BTreeMap::new(),
        );
        let escape = diags
            .iter()
            .find(|d| d.code == DiagnosticCode::E065)
            .expect("expected an E065 escape from the unresolvable external param");
        assert!(
            escape.message.contains("get_thing") && escape.message.contains("parameter `id`"),
            "escape must name the offending external param: {escape:?}"
        );
        // `EXTERNAL get_thing(id)` — the `get_thing` name spans bytes 9..18.
        assert_eq!(
            (
                u32::from(escape.range.start()),
                u32::from(escape.range.end())
            ),
            (9, 18),
            "escape anchors at the external's own declaration span: {escape:?}"
        );
    }

    #[test]
    fn unregistered_external_declaration_stays_unchecked_under_strict() {
        let (hir, index, res) = build(EXT_SRC);
        let diags = crate::strict_diagnostics(
            &[(FileId(0), &hir)],
            &index,
            &res,
            &strict_opts(None),
            false,
            None,
            &BTreeMap::new(),
        );
        assert!(
            diags.is_empty(),
            "an unregistered external's params must stay unchecked: {diags:?}"
        );
    }

    #[test]
    fn external_declaration_escapes_never_fire_under_gradual() {
        let (hir, index, res) = build(EXT_SRC);
        let mut opts = strict_opts(Some(get_thing_manifest("")));
        opts.types = Some(TypePolicy::Gradual);
        let diags = crate::strict_diagnostics(
            &[(FileId(0), &hir)],
            &index,
            &res,
            &opts,
            false,
            None,
            &BTreeMap::new(),
        );
        assert!(
            diags.is_empty(),
            "gradual mode never escape-checks external declarations: {diags:?}"
        );
    }

    #[test]
    fn unconstrained_empty_array_temp_escapes_as_unknown() {
        // spec §5's own worked example.
        let (hir, index, res) = build("=== main ===\n~ temp x = #[]\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E065);
    }

    #[test]
    fn annotated_empty_array_temp_is_exempt() {
        // spec §5: "if unconstrained, that's an Unknown escape -> annotate
        // the binding" — following that advice must silence the error.
        let (hir, index, res) = build("=== main ===\n~ temp x: Array<int> = #[]\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(diags.is_empty(), "ascription supplies the type: {diags:?}");
    }

    #[test]
    fn unannotated_function_with_no_return_statement_infers_void() {
        // Issue #1028: a function whose body never carries a value-returning
        // `return <expr>` — this one has no `return` at all — infers as void
        // exactly like an explicit `: void` annotation would, rather than
        // escaping as Unknown. Before #1028 this asserted an `E065` escape;
        // that was the exact gap the issue closed (typed-mode-spec §3 already
        // treats "no-return function" as `void`'s job — the annotation just
        // shouldn't be *required* to say what the body already proves).
        let (hir, index, res) = build("=== function noop() ===\nHello.\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unannotated_function_with_unresolvable_return_value_still_escapes() {
        // Issue #1028's flip side: a function *does* have a value-returning
        // `return`, but the value's own type can't be pinned down (`x` is an
        // otherwise-unconstrained param, which escapes in its own right too).
        // The return type must still `E065`-escape — void inference reads
        // "never returns a value", never "returns a value inference gave up
        // on".
        let (hir, index, res) = build("=== function noop(x) ===\n~ return x\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E065));
        assert!(
            diags.iter().any(|d| d.message.contains("return type")),
            "expected a return-type escape among {diags:?}"
        );
    }

    // ─── Issue #1168: Option-returning functions no longer E065-escape ──

    /// The issue's tightest repro, at the diagnostic level: `some(x)`
    /// where `x: int` is never evidenced anywhere else in the body used to
    /// infer `Option[Unknown]` and trip `E065` with no annotation escape
    /// hatch. Fixed at the inference layer (`infer::body::InferPass::
    /// or_own_annotation`) — `strict::check` needs no changes, this pins
    /// the diagnostic-level outcome.
    #[test]
    fn some_of_an_unevidenced_annotated_param_no_longer_escapes() {
        let (hir, index, res) = build("=== function f(x: int) ===\n~ return some(x)\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// `docs/book/src/toolchain/dialect/iteration.md`'s `first_over` fence
    /// (unmarked from `ink,proposed` in this same PR): a `for` loop over
    /// an annotated `Array<int>` param, `return some(<loop var>)` on one
    /// path and `return none` on the other — both the return type and the
    /// loop-var temp used to escape as `Unknown`.
    #[test]
    fn first_over_style_option_return_no_longer_escapes() {
        let (hir, index, res) = build(
            "=== function first_over(tab: Array<int>, floor: int) ===\n\
             ~ {\n    for coins in tab {\n        if coins > floor {\n            return some(coins)\n        }\n    }\n}\n\
             ~ return none\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn void_annotated_function_return_is_exempt() {
        let (hir, index, res) = build("=== function noop(): void ===\n~ return\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn non_function_knot_return_is_never_checked() {
        // An ordinary knot has no return-value concept at all — never flagged
        // regardless of whether the body ever exercises `~ return`.
        let (hir, index, res) = build("=== main ===\nHello.\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── issue #1028: void-return inference for a void-external wrapper ──

    fn notify_manifest() -> brink_ir::HostManifest {
        brink_ir::HostManifest {
            markup: Vec::new(),
            types: Vec::new(),
            externals: vec![brink_ir::ManifestExternal {
                name: "notify".to_string(),
                params: Vec::new(),
                returns: brink_ir::TypeRef("void".to_string()),
                kind: brink_ir::ExternalKind::default(),
                doc: None,
                widgets: Vec::new(),
                path: Vec::new(),
            }],
        }
    }

    #[test]
    fn wrapper_around_void_external_with_no_explicit_return_infers_void_and_is_strict_clean() {
        // The issue's own motivating shape: a function whose body only calls
        // a void external and never returns explicitly.
        let (hir, index, res) =
            build("EXTERNAL notify()\n=== function wrap_notify() ===\n~ notify()\n");
        let diags = crate::strict_diagnostics(
            &[(FileId(0), &hir)],
            &index,
            &res,
            &strict_opts(Some(notify_manifest())),
            false,
            None,
            &BTreeMap::new(),
        );
        assert!(
            diags.is_empty(),
            "a void-external wrapper with no explicit return must infer void, not \
             Unknown-escape: {diags:?}"
        );
    }

    #[test]
    fn wrapper_around_void_external_with_a_real_return_path_is_unaffected() {
        // Adding a genuine value-returning path alongside the void-external
        // call must suppress the void inference exactly as before #1028 —
        // the wrapper's own return type still resolves concretely (`int`)
        // and stays clean, not because it's void but because `5` is.
        let (hir, index, res) = build(
            "EXTERNAL notify()\n=== function wrap_and_report() ===\n~ notify()\n~ return 5\n",
        );
        let inference = crate::infer_project(
            &[(FileId(0), &hir)],
            &index,
            &res,
            Some(&notify_manifest()),
            &BTreeMap::new(),
        );
        let wrap_id =
            annotations::def_id_for(&index, FileId(0), SymbolKind::Knot, "wrap_and_report")
                .expect("wrap_and_report must resolve");
        assert_eq!(
            inference.signatures.get(&wrap_id).map(|s| &s.return_ty),
            Some(&Ty::Int),
            "a real return path must still infer its own concrete type, unaffected by the \
             sibling void-external call"
        );
        let diags = crate::strict_diagnostics(
            &[(FileId(0), &hir)],
            &index,
            &res,
            &strict_opts(Some(notify_manifest())),
            false,
            None,
            &BTreeMap::new(),
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── check(): Conflicted-escape ─────────────────────────────────

    #[test]
    fn genuinely_disjoint_param_uses_escape_as_conflicted() {
        let (hir, index, res) = build(
            "=== conflict_case(hp) ===\n{hp > 5:\n  ok\n}\n{hp == \"no\":\n  no\n}\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E066);
    }

    #[test]
    fn annotation_never_exempts_a_conflicted_slot() {
        // Annotating a genuinely conflicted param doesn't heal the body's
        // internal contradiction — Conflicted-escape still fires (E063 stays
        // silent for the same reason: `is_unresolved()` covers Conflicted).
        let (hir, index, res) = build(
            "=== conflict_case(hp: int) ===\n{hp > 5:\n  ok\n}\n{hp == \"no\":\n  no\n}\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E066);
    }

    #[test]
    fn heterogeneous_array_literal_temp_escapes_as_conflicted() {
        // spec §5: `#[1, "a"]` is an error — the join lattice already
        // produces `Array(Conflicted)`; this module's recursive classify
        // catches it through the nesting.
        let (hir, index, res) = build("=== main ===\n~ temp x = #[1, \"a\"]\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E066);
    }

    // ── §4 coercion lattice survives strict (regression guards) ────

    #[test]
    fn condition_position_int_truthiness_survives_strict() {
        // `{visited_knot: ...}`-style int truthiness in condition position
        // must never escape — the type resolves cleanly to a concrete `int`.
        let (hir, index, res) = build("=== main ===\nVAR gold = 5\n{gold:\n  rich\n}\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn int_to_float_join_survives_strict_with_no_escape() {
        let (hir, index, res) = build("=== spend(gold) ===\n{gold > 1.5:\n  ok\n}\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags.is_empty(),
            "int->float directional join is clean: {diags:?}"
        );
    }

    // ── E063 wiring ──────────────────────────────────────────────────

    #[test]
    fn check_wires_in_e063_mismatches() {
        let (hir, index, res) = build("=== heal(hp: string) ===\n{hp > 1:\n  ok\n}\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E063),
            "{diags:?}"
        );
    }

    // ── determinism ──────────────────────────────────────────────────

    #[test]
    fn escape_diagnostics_are_order_independent() {
        let forward =
            "=== conflict_fwd(hp) ===\n{hp > 5:\n  ok\n}\n{hp == \"no\":\n  no\n}\n-> DONE\n";
        let reversed =
            "=== conflict_rev(hp) ===\n{hp == \"no\":\n  no\n}\n{hp > 5:\n  ok\n}\n-> DONE\n";

        let (hir_f, index_f, res_f) = build(forward);
        let inference_f = crate::infer_project(
            &[(FileId(0), &hir_f)],
            &index_f,
            &res_f,
            None,
            &BTreeMap::new(),
        );
        let diags_f = check(&[(FileId(0), &hir_f)], &index_f, &inference_f, &res_f, None);

        let (hir_r, index_r, res_r) = build(reversed);
        let inference_r = crate::infer_project(
            &[(FileId(0), &hir_r)],
            &index_r,
            &res_r,
            None,
            &BTreeMap::new(),
        );
        let diags_r = check(&[(FileId(0), &hir_r)], &index_r, &inference_r, &res_r, None);

        assert_eq!(codes(&diags_f), vec![DiagnosticCode::E066]);
        assert_eq!(codes(&diags_r), vec![DiagnosticCode::E066]);
    }

    #[test]
    fn clean_strict_project_compiles_with_no_strict_diagnostics() {
        let (hir, index, res) = build(
            "=== function heal(hp: int): int ===\n~ temp bonus: int = 5\n~ return hp + bonus\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── effective_severity ──────────────────────────────────────────

    #[test]
    fn effective_severity_e063_is_warning_under_gradual() {
        assert_eq!(
            effective_severity(
                DiagnosticCode::E063,
                TypePolicy::Gradual,
                &LintPolicy::default()
            ),
            brink_ir::Severity::Warning
        );
    }

    #[test]
    fn effective_severity_e063_is_error_under_strict() {
        assert_eq!(
            effective_severity(
                DiagnosticCode::E063,
                TypePolicy::Strict,
                &LintPolicy::default()
            ),
            brink_ir::Severity::Error
        );
    }

    #[test]
    fn effective_severity_other_codes_are_policy_independent() {
        // A code with no strict-conditional carve-out keeps its default
        // severity regardless of policy — only E063 is ever conditioned.
        for policy in [TypePolicy::Gradual, TypePolicy::Strict] {
            assert_eq!(
                effective_severity(DiagnosticCode::E065, policy, &LintPolicy::default()),
                DiagnosticCode::E065.severity()
            );
            assert_eq!(
                effective_severity(DiagnosticCode::E022, policy, &LintPolicy::default()),
                DiagnosticCode::E022.severity()
            );
        }
    }

    // ── effective_severity: [lints] (issue #1160) ───────────────────

    #[test]
    fn absent_lints_table_is_byte_identical_to_default_severity() {
        // Every non-E063 code, under both policies, with an empty
        // `LintPolicy`: must match `DiagnosticCode::severity()` exactly —
        // the "absent table = today's behavior" acceptance criterion.
        for policy in [TypePolicy::Gradual, TypePolicy::Strict] {
            for code in [
                DiagnosticCode::E014,
                DiagnosticCode::E022,
                DiagnosticCode::E025,
                DiagnosticCode::E037,
            ] {
                assert_eq!(
                    effective_severity(code, policy, &LintPolicy::default()),
                    code.severity(),
                    "code {code:?} under {policy:?} must be unaffected by an empty LintPolicy"
                );
            }
        }
    }

    #[test]
    fn lint_override_deny_relevels_a_warning_code_to_error() {
        // E014 defaults to Warning.
        assert_eq!(DiagnosticCode::E014.severity(), brink_ir::Severity::Warning);
        let lints = LintPolicy {
            overrides: BTreeMap::from([("E014".to_owned(), LintLevel::Deny)]),
            deny_warnings: false,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E014, TypePolicy::Gradual, &lints),
            brink_ir::Severity::Error
        );
    }

    #[test]
    fn lint_override_allow_keeps_a_warning_code_at_warning() {
        let lints = LintPolicy {
            overrides: BTreeMap::from([("E014".to_owned(), LintLevel::Allow)]),
            deny_warnings: false,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E014, TypePolicy::Gradual, &lints),
            brink_ir::Severity::Warning
        );
    }

    // ── effective_severity: [lints] info/hint tier (issue #1162) ────

    #[test]
    fn lint_override_info_relevels_a_warning_code_to_info() {
        let lints = LintPolicy {
            overrides: BTreeMap::from([("E014".to_owned(), LintLevel::Info)]),
            deny_warnings: false,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E014, TypePolicy::Gradual, &lints),
            brink_ir::Severity::Info
        );
    }

    #[test]
    fn lint_override_hint_relevels_a_warning_code_to_hint() {
        let lints = LintPolicy {
            overrides: BTreeMap::from([("E014".to_owned(), LintLevel::Hint)]),
            deny_warnings: false,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E014, TypePolicy::Gradual, &lints),
            brink_ir::Severity::Hint
        );
    }

    #[test]
    fn deny_warnings_does_not_touch_an_info_or_hint_override() {
        // Like `Allow`, `Info`/`Hint` are deliberate downgrades and must stay
        // immune to `deny-warnings` — escalating them back up would defeat
        // the point of setting them.
        let lints_info = LintPolicy {
            overrides: BTreeMap::from([("E014".to_owned(), LintLevel::Info)]),
            deny_warnings: true,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E014, TypePolicy::Gradual, &lints_info),
            brink_ir::Severity::Info
        );
        let lints_hint = LintPolicy {
            overrides: BTreeMap::from([("E014".to_owned(), LintLevel::Hint)]),
            deny_warnings: true,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E014, TypePolicy::Gradual, &lints_hint),
            brink_ir::Severity::Hint
        );
    }

    #[test]
    fn hard_error_code_is_never_downgraded_to_info_or_hint() {
        // Same hard-error exemption as `Allow`/`Deny` — a code that is Error
        // by default is never even looked up in `[lints]`.
        assert_eq!(DiagnosticCode::E025.severity(), brink_ir::Severity::Error);
        let lints = LintPolicy {
            overrides: BTreeMap::from([("E025".to_owned(), LintLevel::Hint)]),
            deny_warnings: false,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E025, TypePolicy::Gradual, &lints),
            brink_ir::Severity::Error
        );
    }

    #[test]
    fn deny_warnings_promotes_unconfigured_warning_codes_to_error() {
        let lints = LintPolicy {
            overrides: BTreeMap::new(),
            deny_warnings: true,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E014, TypePolicy::Gradual, &lints),
            brink_ir::Severity::Error
        );
        assert_eq!(
            effective_severity(DiagnosticCode::E022, TypePolicy::Gradual, &lints),
            brink_ir::Severity::Error
        );
    }

    #[test]
    fn deny_warnings_does_not_touch_an_allow_override() {
        // `allow` is specifically the "immune to deny-warnings" knob.
        let lints = LintPolicy {
            overrides: BTreeMap::from([("E014".to_owned(), LintLevel::Allow)]),
            deny_warnings: true,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E014, TypePolicy::Gradual, &lints),
            brink_ir::Severity::Warning
        );
    }

    #[test]
    fn hard_error_code_is_never_downgraded_by_lints_or_deny_warnings() {
        // E025 (unresolved reference) defaults to Error and is not in the
        // Warning set — [lints] must never be consulted for it at all,
        // regardless of what a (nonsensical) override or deny-warnings say.
        assert_eq!(DiagnosticCode::E025.severity(), brink_ir::Severity::Error);
        let lints = LintPolicy {
            overrides: BTreeMap::from([("E025".to_owned(), LintLevel::Allow)]),
            deny_warnings: false,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E025, TypePolicy::Gradual, &lints),
            brink_ir::Severity::Error
        );
    }

    #[test]
    fn deny_override_wins_even_without_deny_warnings() {
        let lints = LintPolicy {
            overrides: BTreeMap::from([("E014".to_owned(), LintLevel::Deny)]),
            deny_warnings: false,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E014, TypePolicy::Gradual, &lints),
            brink_ir::Severity::Error
        );
    }

    /// Regression pin for the #1674 refactor: an *explicit* `[lints] E014 =
    /// "warn"` on a `Warning`-base code must still be escalated by
    /// `deny-warnings`, exactly like an unconfigured code (the pre-#1674
    /// implementation grouped `Some(Warn) | None` under one `if
    /// deny_warnings {Error} else {Warning}` arm — the generalized version
    /// must reach the same answer via its `candidate == Warning &&
    /// deny_warnings` check).
    #[test]
    fn explicit_warn_override_is_still_escalated_by_deny_warnings() {
        let lints = LintPolicy {
            overrides: BTreeMap::from([("E014".to_owned(), LintLevel::Warn)]),
            deny_warnings: true,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E014, TypePolicy::Gradual, &lints),
            brink_ir::Severity::Error
        );
    }

    // ── effective_severity: Info/Hint-base codes (issue #1674) ──────
    //
    // `E157` is the first code whose *default* severity is `Info` rather
    // than `Warning` — these pin the generalized `effective_severity`/
    // `validate_lint_code` behavior for that base, alongside the
    // `Warning`-base regression coverage above (proving the widened
    // resolution order reaches the byte-identical answer for every
    // pre-#1674 case).

    #[test]
    fn info_base_code_defaults_to_info_with_no_lints() {
        assert_eq!(
            DiagnosticCode::E157.severity(),
            brink_ir::Severity::Info,
            "E157 is the off/info-by-default lint issue #1674 rules for"
        );
        assert_eq!(
            effective_severity(
                DiagnosticCode::E157,
                TypePolicy::Gradual,
                &LintPolicy::default()
            ),
            brink_ir::Severity::Info
        );
    }

    #[test]
    fn info_base_code_is_immune_to_deny_warnings_when_unconfigured() {
        // The whole point of defaulting to `Info`: a project that never
        // touches `[lints]` for E157 must not have `deny-warnings` promote
        // it to `Error` behind the author's back.
        let lints = LintPolicy {
            overrides: BTreeMap::new(),
            deny_warnings: true,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E157, TypePolicy::Gradual, &lints),
            brink_ir::Severity::Info
        );
    }

    #[test]
    fn info_base_code_can_be_raised_to_warn_via_lints() {
        let lints = LintPolicy {
            overrides: BTreeMap::from([("E157".to_owned(), LintLevel::Warn)]),
            deny_warnings: false,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E157, TypePolicy::Gradual, &lints),
            brink_ir::Severity::Warning
        );
    }

    #[test]
    fn info_base_code_raised_to_warn_is_then_escalated_by_deny_warnings() {
        let lints = LintPolicy {
            overrides: BTreeMap::from([("E157".to_owned(), LintLevel::Warn)]),
            deny_warnings: true,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E157, TypePolicy::Gradual, &lints),
            brink_ir::Severity::Error
        );
    }

    #[test]
    fn info_base_code_can_be_denied_straight_to_error() {
        let lints = LintPolicy {
            overrides: BTreeMap::from([("E157".to_owned(), LintLevel::Deny)]),
            deny_warnings: false,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E157, TypePolicy::Gradual, &lints),
            brink_ir::Severity::Error
        );
    }

    #[test]
    fn info_base_code_can_be_downleveled_to_hint() {
        let lints = LintPolicy {
            overrides: BTreeMap::from([("E157".to_owned(), LintLevel::Hint)]),
            deny_warnings: true,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E157, TypePolicy::Gradual, &lints),
            brink_ir::Severity::Hint,
            "an explicit Hint downgrade must stay immune to deny-warnings too"
        );
    }

    #[test]
    fn info_base_code_allow_override_is_a_no_op() {
        let lints = LintPolicy {
            overrides: BTreeMap::from([("E157".to_owned(), LintLevel::Allow)]),
            deny_warnings: true,
        };
        assert_eq!(
            effective_severity(DiagnosticCode::E157, TypePolicy::Gradual, &lints),
            brink_ir::Severity::Info,
            "Allow keeps the code at its own base — Info here, not Warning"
        );
    }

    // ── check(): void-assignment (E067) ────────────────────────────

    #[test]
    fn void_assigned_to_temp_is_e067() {
        let (hir, index, res) = build(
            "=== function noop(): void ===\n~ return\n\
             === main ===\n~ temp x = noop()\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E067),
            "{diags:?}"
        );
    }

    #[test]
    fn void_assigned_to_var_is_e067() {
        let (hir, index, res) = build(
            "VAR gold = 0\n=== function noop(): void ===\n~ return\n\
             === main ===\n~ gold = noop()\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E067),
            "{diags:?}"
        );
    }

    #[test]
    fn void_call_in_statement_position_is_clean() {
        let (hir, index, res) = build(
            "=== function noop(): void ===\n~ return\n\
             === main ===\n~ noop()\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            !diags.iter().any(|d| d.code == DiagnosticCode::E067),
            "statement-position void call must never be flagged: {diags:?}"
        );
    }

    #[test]
    fn non_void_call_assigned_is_clean_of_e067() {
        let (hir, index, res) = build(
            "=== function give(): int ===\n~ return 5\n\
             === main ===\n~ temp x: int = give()\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            !diags.iter().any(|d| d.code == DiagnosticCode::E067),
            "{diags:?}"
        );
    }

    #[test]
    fn inferred_void_assigned_to_temp_is_e067() {
        // Issue #1054: `noop` carries no `): void ===` annotation at all —
        // its void-ness is purely inferred (#1046: no value-returning
        // `return` anywhere in the body). Before this fix `collect_void_defs`
        // only ever consulted `knot.return_type`, so this assignment was
        // silently accepted; it must now `E067` exactly like the
        // explicitly-annotated case does.
        let (hir, index, res) = build(
            "=== function noop() ===\nHello.\n\
             === main ===\n~ temp x = noop()\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E067),
            "{diags:?}"
        );
    }

    #[test]
    fn inferred_void_assigned_to_var_is_e067() {
        let (hir, index, res) = build(
            "VAR gold = 0\n=== function noop() ===\nHello.\n\
             === main ===\n~ gold = noop()\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E067),
            "{diags:?}"
        );
    }

    #[test]
    fn inferred_void_call_in_statement_position_is_clean() {
        // Same firewall as the explicitly-annotated case: a statement-
        // position call never assigns the (nonexistent) result anywhere, so
        // it must stay clean regardless of whether void-ness is annotated or
        // inferred.
        let (hir, index, res) = build(
            "=== function noop() ===\nHello.\n\
             === main ===\n~ noop()\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            !diags.iter().any(|d| d.code == DiagnosticCode::E067),
            "statement-position inferred-void call must never be flagged: {diags:?}"
        );
    }

    #[test]
    fn function_with_real_return_path_is_not_inferred_void_and_stays_clean_of_e067() {
        // Flip side of #1046's own inference rule: an unannotated function
        // that *does* return a value is not void — assigning its result must
        // not `E067`.
        let (hir, index, res) = build(
            "=== function give() ===\n~ return 5\n\
             === main ===\n~ temp x = give()\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            !diags.iter().any(|d| d.code == DiagnosticCode::E067),
            "{diags:?}"
        );
    }

    #[test]
    fn stitch_return_value_reached_by_fallthrough_is_not_inferred_void_and_stays_clean_of_e067() {
        // Regression for the reviewer-caught gap in `collect_void_defs`: the
        // value-returning `return` lives in a *stitch* (`compute`), reached
        // by falling straight through the knot's own (empty) body — not by
        // an explicit divert. A stitch under a function knot is a separate
        // `Def` (`infer::collect_defs`, qualified name `f.compute`,
        // `SymbolKind::Stitch`) with its own `BodyTypes`, so the knot's own
        // `BodyTypes.has_value_return` is `false` even though the function
        // as a whole always returns a value. Before the fix this silently
        // inferred `f` as void and flagged `E067` on the assignment below.
        let (hir, index, res) = build(
            "=== function f() ===\n= compute\n~ return 5\n\
             === main ===\n~ temp x: int = f()\nx={x}\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            !diags.iter().any(|d| d.code == DiagnosticCode::E067),
            "{diags:?}"
        );
    }

    #[test]
    fn declared_non_void_return_falling_through_is_e150_not_e067() {
        // Issue #1054's own excluded shape (see `collect_void_defs`'s doc
        // comment): a *declared*, non-`void` return type whose body never
        // returns a value is the #1551 checker error (`E150`, "declares a
        // return type but its body never returns a value") — not an
        // inferred-void function. It must never also `E067`-flag its own
        // assignment: the function is broken, not void, and reporting E067
        // on top would be misleading (asking to remove an assignment that
        // isn't actually the bug).
        let (hir, index, res) = build(
            "=== function broken(): int ===\nHello.\n\
             === main ===\n~ temp x = broken()\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E150),
            "{diags:?}"
        );
        assert!(
            !diags.iter().any(|d| d.code == DiagnosticCode::E067),
            "a declared-return-type fall-through must be E150, not also E067: {diags:?}"
        );
    }

    #[test]
    fn stitch_return_value_reached_by_fallthrough_is_not_e150() {
        // Issue #1591: the exact false positive from the issue body — the
        // value-returning `return` lives in the stitch (`compute`), reached
        // by falling straight through the knot's own (empty) body, not by
        // an explicit divert. `check_def`'s E150 path previously only read
        // the knot's own `BodyTypes.has_value_return` (`false`, since the
        // knot's own body before the first stitch never returns), so it
        // fired E150 even though the function as a whole always returns a
        // value. Twin of `stitch_return_value_reached_by_fallthrough_is_not_
        // inferred_void_and_stays_clean_of_e067` above, but for the E150
        // consumer instead of E067 — both now read the same shared
        // has-value-return-over-stitches fact.
        let (hir, index, res) = build("=== function f(): int ===\n= compute\n~ return 5\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            !diags.iter().any(|d| d.code == DiagnosticCode::E150),
            "{diags:?}"
        );
    }

    #[test]
    fn unannotated_function_return_value_reached_by_fallthrough_stitch_is_not_e065() {
        // Regression for a reviewer-caught escape-check false positive:
        // an unannotated `fn` whose only value-returning `return` lives in
        // a fall-through stitch must stay clean under strict, exactly like
        // the identically-shaped own-body spelling (`=== function g() ===
        // \n~ return 5\n`, clean on both this fix and main). Before the
        // fix, `check_def`'s E065/E066 escape branch read the *merged*
        // has-value-return fact (the def's own body plus its stitches, per
        // `has_value_return_over_stitches`) instead of the def's own body
        // alone — so it treated the stitch's `return 5` as proof the
        // *knot's own* inferred return type was resolved, when
        // `sig.return_ty` (the thing actually being escape-checked) is
        // still `Unknown`: inference never merges a stitch's return type
        // into its owning knot's signature, only the has-value-return
        // *fact* is merged, and only for the E150/E067 consumers. That
        // made this fall-through spelling a hard compile error while the
        // own-body spelling compiled clean.
        let (hir, index, res) = build("=== function f() ===\n= compute\n~ return 5\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn void_assignment_never_checked_under_gradual() {
        // `check`'s void-assignment pass is unconditional — it's
        // `finish_analysis` that gates the whole `strict::check` call behind
        // `opts.types == TypePolicy::Strict`. Exercise that real gate (not
        // `check` directly) to prove a void assignment stays silent under
        // gradual, matching this module's "byte-identical forever" contract.
        let parsed = brink_syntax::parse(
            "=== function noop(): void ===\n~ return\n\
             === main ===\n~ temp x = noop()\n-> DONE\n",
        );
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            // These tests TEST gradual behavior — explicit opt-out knob
            // (#1127: the brink dialect's implicit default is now strict).
            types: Some(TypePolicy::Gradual),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E067),
            "gradual must never surface E067: {:?}",
            result.diagnostics
        );
    }

    // ── T1c: calls through function values (docs/t1c-spec.md §4/§8) ────

    /// The spec's worked example, end to end at the analysis layer: a
    /// well-formed creation + a well-typed call through the value is clean
    /// under strict.
    const HEAL: &str = "=== function heal(ref hp: int, amount: int): int ===\n~ hp = hp + amount\n~ return hp\n\
         VAR player_hp = 10\n";

    #[test]
    fn well_typed_call_through_a_fn_value_is_clean_under_strict() {
        let (hir, index, res) = build(&format!(
            "{HEAL}=== main ===\n~ temp heal_player = #fn(heal, player_hp)\n\
             ~ temp result: int = heal_player(5)\n-> DONE\n"
        ));
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// Issue #1680 step 2 — the regression this whole step exists for.
    ///
    /// `apply`'s unannotated `cb` param is reassigned to `#fn(bump)` inside
    /// the body, so its *inferred* type carries `bump` as its effect row
    /// (issue #1680 step 3). A caller that passes `#fn(twice)` in that
    /// position is passing a perfectly well-typed `fn(int): int` — but the
    /// two rows differ, so a **structural** `unify(param, arg) == param`
    /// test sees the join widen and reports `ValueCallKind::ArgMismatch`,
    /// which `effective_severity` promotes to an `E063` **error** under
    /// `types = strict`. The message is self-refuting ("expected
    /// `fn(int): int`, found `fn(int): int`") because rows are not part of
    /// `Ty::display`.
    ///
    /// `infer::assignable` erases rows on both sides, which is what keeps
    /// this clean.
    #[test]
    fn differing_effect_rows_are_not_an_argument_mismatch() {
        let (hir, index, res) = build(
            "=== function bump(n: int): int ===\n~ return n + 1\n\
             === function twice(n: int): int ===\n~ return n * 2\n\
             === function apply(cb, x: int): int ===\n\
             ~ cb = #fn(bump)\n~ return cb(x)\n\
             === main ===\n~ temp a = #fn(apply)\n\
             ~ temp r: int = a(#fn(twice), 1)\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            !diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "a differing effect row is not a type mismatch: {diags:?}"
        );
    }

    /// The second `ValueCallKind::ArgMismatch` site — `check_bind_value`,
    /// the `bind(f, args…)` form. Same fixture shape as
    /// [`differing_effect_rows_are_not_an_argument_mismatch`], routed
    /// through partial application instead of a direct value call, because
    /// the two sites carry independent copies of the assignability test.
    #[test]
    fn differing_effect_rows_are_not_a_bind_argument_mismatch() {
        let (hir, index, res) = build(
            "=== function bump(n: int): int ===\n~ return n + 1\n\
             === function twice(n: int): int ===\n~ return n * 2\n\
             === function apply(cb, x: int): int ===\n\
             ~ cb = #fn(bump)\n~ return cb(x)\n\
             === main ===\n~ temp a = #fn(apply)\n\
             ~ temp p = bind(a, #fn(twice))\n~ temp r: int = p(1)\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            !diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "a differing effect row is not a bind mismatch: {diags:?}"
        );
    }

    #[test]
    fn int_to_float_coercion_applies_to_fn_value_call_arguments() {
        // `fn(float): float` called with an int literal — the one legal
        // directional coercion (spec §4) applies exactly as at direct calls.
        let (hir, index, res) = build(
            "=== function scale(factor: float): float ===\n~ return factor * 2.0\n\
             === main ===\n~ temp f = #fn(scale)\n~ temp r: float = f(2)\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn fn_value_call_arity_mismatch_is_a_typed_mismatch_error() {
        let (hir, index, res) = build(&format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal, player_hp)\n\
             ~ temp r: int = f(5, 6)\n-> DONE\n"
        ));
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E063);
        assert!(diags[0].message.contains("2 argument"), "{diags:?}");
    }

    #[test]
    fn fn_value_call_argument_type_mismatch_is_a_typed_mismatch_error() {
        let (hir, index, res) = build(&format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal, player_hp)\n\
             ~ temp r: int = f(\"lots\")\n-> DONE\n"
        ));
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "{diags:?}"
        );
    }

    #[test]
    fn float_to_int_narrowing_at_a_fn_value_call_is_an_error() {
        // The coercion is directional: int -> float only. `fn(int): int`
        // called with a float literal must be flagged.
        let (hir, index, res) = build(&format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal, player_hp)\n\
             ~ temp r: int = f(1.5)\n-> DONE\n"
        ));
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "{diags:?}"
        );
    }

    #[test]
    fn unknown_callee_in_call_position_is_an_escape_error() {
        // A call through a value whose type never resolves — the TM-3
        // escape rule applied to call position (spec §4: "if the callee's
        // type is Unknown/Conflicted, that is an escape error").
        let (hir, index, res) = build("=== main(g) ===\n~ temp r = g(1)\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E065
                && d.message.contains("called as a function value")),
            "{diags:?}"
        );
    }

    #[test]
    fn conflicted_callee_in_call_position_is_a_conflicted_escape_error() {
        let (hir, index, res) =
            build("=== main ===\n~ temp f = 1\n{f == \"x\":\n  no\n}\n~ temp r = f(5)\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E066
                && d.message.contains("called as a function value")),
            "{diags:?}"
        );
    }

    #[test]
    fn calling_a_known_non_fn_value_is_a_typed_mismatch_error() {
        let (hir, index, res) = build("=== main ===\n~ temp n = 5\n~ temp r = n(1)\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("not callable")),
            "{diags:?}"
        );
    }

    #[test]
    fn annotated_fn_typed_param_is_callable_under_strict() {
        // The boundary-annotation form (spec §4: "fn-typed params can cross
        // host boundaries under strict"): `cb`'s only constraint is its
        // annotation, and the call through it checks against that row.
        let (hir, index, res) =
            build("=== function apply(cb: fn(int): int, x: int): int ===\n~ return cb(x)\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn annotated_fn_typed_param_call_still_checks_argument_types() {
        let (hir, index, res) =
            build("=== function apply(cb: fn(int): int): int ===\n~ return cb(\"nope\")\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "{diags:?}"
        );
    }

    #[test]
    fn fn_value_call_checks_never_surface_under_gradual() {
        // The real production gate: `finish_analysis` only calls
        // `strict::check` under `types = strict` — gradual stays advisory
        // (the §3 runtime fault is its backstop).
        let parsed = brink_syntax::parse("=== main ===\n~ temp n = 5\n~ temp r = n(1)\n-> DONE\n");
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            // These tests TEST gradual behavior — explicit opt-out knob
            // (#1127: the brink dialect's implicit default is now strict).
            types: Some(TypePolicy::Gradual),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            !result.diagnostics.iter().any(|d| matches!(
                d.code,
                DiagnosticCode::E063 | DiagnosticCode::E065 | DiagnosticCode::E066
            )),
            "gradual must never surface value-call checks: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn strict_fn_value_mismatch_fires_through_the_real_pipeline() {
        // analyze_with_options -> finish_analysis -> whole_project_
        // diagnostics -> strict::check — the wiring, not just the unit.
        let parsed = brink_syntax::parse(
            "=== function heal(ref hp: int, amount: int): int ===\n~ hp = hp + amount\n~ return hp\n\
             VAR player_hp = 10\n\
             === main ===\n~ temp f = #fn(heal, player_hp)\n~ temp r: int = f(\"x\")\n-> DONE\n",
        );
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "{:?}",
            result.diagnostics
        );
    }

    // ── T1c follow-up (issue #712): declaration-derived Ty::Fn for global
    //    VARs (docs/t1c-spec.md §4) ───────────────────────────────────

    /// Same worked example as `HEAL`, but the fn value itself is a *global*
    /// (`VAR heal_player = #fn(heal, player_hp)`), not a local temp — the
    /// exact shape #712 closes: a global's declaration-derived signature
    /// must carry `Ty::Fn` so a call *through the global directly* type-
    /// checks under strict instead of escaping as Unknown.
    const HEAL_GLOBAL: &str = "=== function heal(ref hp: int, amount: int): int ===\n\
         ~ hp = hp + amount\n~ return hp\n\
         VAR player_hp = 10\n\
         VAR heal_player = #fn(heal, player_hp)\n";

    #[test]
    fn well_typed_call_through_a_global_fn_value_is_clean_under_strict() {
        let (hir, index, res) = build(&format!(
            "{HEAL_GLOBAL}=== main ===\n~ temp result: int = heal_player(5)\n-> DONE\n"
        ));
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn arity_mismatch_through_a_global_fn_value_is_a_typed_mismatch_error() {
        let (hir, index, res) = build(&format!(
            "{HEAL_GLOBAL}=== main ===\n~ temp r: int = heal_player(5, 6)\n-> DONE\n"
        ));
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E063);
        assert!(diags[0].message.contains("2 argument"), "{diags:?}");
    }

    #[test]
    fn argument_type_mismatch_through_a_global_fn_value_is_a_typed_mismatch_error() {
        let (hir, index, res) = build(&format!(
            "{HEAL_GLOBAL}=== main ===\n~ temp r: int = heal_player(\"lots\")\n-> DONE\n"
        ));
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "{diags:?}"
        );
    }

    #[test]
    fn explicitly_annotated_global_fn_value_wins_over_an_unannotated_target() {
        // `identity` carries no param/return annotations at all, so the
        // `#fn(identity)` initializer alone would infer an all-`Unknown`
        // row — but `f`'s own `fn(int): int` annotation must win (the same
        // TM-2 firewall rule `value_type` already applies), so the
        // wrong-typed call below is still caught, not silently waved
        // through as an Unknown-escape.
        let (hir, index, res) = build(
            "=== function identity(x) ===\n~ return x\n\
             VAR f: fn(int): int = #fn(identity)\n\
             === main ===\n~ temp r: int = f(\"x\")\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "{diags:?}"
        );
    }

    #[test]
    fn cross_signature_reassignment_through_globals_is_a_conflicted_escape() {
        // Two globals with genuinely incompatible `fn(T…): R` shapes; a
        // temp bound from one and reassigned from the other joins to a
        // `Ty::Fn` row carrying `Conflicted` components (the pre-existing
        // #627 pointwise Fn×Fn unify — no new unify logic needed here),
        // which the ordinary temp Conflicted-escape check (`E066`) already
        // catches once the globals themselves carry real `Ty::Fn`s instead
        // of both escaping as `Unknown` (which would `unify` to `Unknown`,
        // not `Conflicted`, silently hiding the disagreement).
        let (hir, index, res) = build(
            "=== function heal(ref hp: int, amount: int): int ===\n\
             ~ hp = hp + amount\n~ return hp\n\
             === function greet(name: string): string ===\n~ return name\n\
             VAR player_hp = 10\n\
             VAR heal_fn = #fn(heal, player_hp)\n\
             VAR greet_fn = #fn(greet)\n\
             === main ===\n~ temp f = heal_fn\n~ f = greet_fn\n~ temp r = f(1)\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E066 && d.message.contains("temp `f`")),
            "{diags:?}"
        );
    }

    #[test]
    fn global_fn_value_call_checks_never_surface_under_gradual() {
        let parsed = brink_syntax::parse(&format!(
            "{HEAL_GLOBAL}=== main ===\n~ temp r: int = heal_player(5, 6)\n-> DONE\n"
        ));
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            // These tests TEST gradual behavior — explicit opt-out knob
            // (#1127: the brink dialect's implicit default is now strict).
            types: Some(TypePolicy::Gradual),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            !result.diagnostics.iter().any(|d| matches!(
                d.code,
                DiagnosticCode::E063 | DiagnosticCode::E065 | DiagnosticCode::E066
            )),
            "gradual must never surface value-call checks: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn strict_global_fn_value_mismatch_fires_through_the_real_pipeline() {
        // analyze_with_options -> finish_analysis -> whole_project_
        // diagnostics -> strict::check — the real production entry point
        // (`brink-compiler`/IDE), not just the `infer_project`/`check`
        // units above.
        let parsed = brink_syntax::parse(&format!(
            "{HEAL_GLOBAL}=== main ===\n~ temp r: int = heal_player(\"x\")\n-> DONE\n"
        ));
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "{:?}",
            result.diagnostics
        );
    }

    // ── issue #628: list-literal global VAR carries its nominal LIST type
    //    (docs/typed-mode-spec.md §2/§5) ─────────────────────────────────

    /// A VAR initialized directly to a list literal must infer its nominal
    /// `List<L>` type end-to-end, not collapse to `Unknown` (the phase-0
    /// `Sig` stub bug this issue reports). A temp assigned straight from
    /// such a VAR is the concrete, checkable consequence: before the fix,
    /// `weather`'s `Sig::value_type` fed `collect_globals` as `Ty::Unknown`
    /// (`infer::mod`'s `From<InferredType> for Ty` collapse), so `w` would
    /// spuriously trip the Unknown-escape check (`E065`) under strict even
    /// though its value is plainly a `List<Weathers>` — the same "resolved
    /// nominal type is clean" treatment `Ty::Struct`/`Ty::Handle` already
    /// get (`classify`'s doc above).
    #[test]
    fn list_literal_global_var_temp_is_clean_under_strict() {
        let (hir, index, res) = build(
            "LIST Weathers = sunny, rainy, snowy\n\
             VAR weather = (sunny)\n\
             === main ===\n~ temp w = weather\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags.is_empty(),
            "list-literal VAR's nominal type must flow through, not escape as Unknown: {diags:?}"
        );
    }

    #[test]
    fn list_literal_global_var_is_clean_through_the_real_pipeline_under_strict() {
        // analyze_with_options -> finish_analysis -> whole_project_
        // diagnostics -> strict::check — the real production entry point
        // (`brink-compiler`/IDE), proving the fix is reachable outside the
        // unit-level `infer_project`/`check` harness too.
        let parsed = brink_syntax::parse(
            "LIST Weathers = sunny, rainy, snowy\n\
             VAR weather = (sunny)\n\
             === main ===\n~ temp w = weather\n-> DONE\n",
        );
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E065),
            "list-literal VAR must not escape as Unknown under strict: {:?}",
            result.diagnostics
        );
    }

    // ── T1c follow-up (issue #733): call()/bind() explicit intrinsic forms
    //    wired into the same strict checker as direct calls / #fn (docs/
    //    t1c-spec.md §3/§4) ───────────────────────────────────────────────

    #[test]
    fn well_typed_explicit_call_through_a_fn_value_is_clean_under_strict() {
        let (hir, index, res) = build(&format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal, player_hp)\n\
             ~ temp result: int = call(f, 5)\n-> DONE\n"
        ));
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn explicit_call_arity_mismatch_is_a_typed_mismatch_error() {
        let (hir, index, res) = build(&format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal, player_hp)\n\
             ~ temp r: int = call(f, 5, 6)\n-> DONE\n"
        ));
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("2 argument")),
            "{diags:?}"
        );
    }

    #[test]
    fn explicit_call_argument_type_mismatch_is_a_typed_mismatch_error() {
        let (hir, index, res) = build(&format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal, player_hp)\n\
             ~ temp r: int = call(f, \"lots\")\n-> DONE\n"
        ));
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "{diags:?}"
        );
    }

    #[test]
    fn unknown_callee_in_explicit_call_is_an_escape_error() {
        let (hir, index, res) = build("=== main(g) ===\n~ temp r = call(g, 1)\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E065
                && d.message.contains("called as a function value")),
            "{diags:?}"
        );
    }

    #[test]
    fn annotated_fn_typed_param_is_callable_through_explicit_call_under_strict() {
        // Same boundary-annotation firewall as the direct-call form (spec
        // §4), reached through `call(cb, …)` instead of `cb(…)`.
        let (hir, index, res) =
            build("=== function apply(cb: fn(int): int, x: int): int ===\n~ return call(cb, x)\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn well_typed_bind_consumes_the_head_of_the_param_row() {
        // `bind` consumes only the head it's given (spec §3): `f`'s
        // remaining row is `fn(int): int` (`amount`); binding `5` leaves
        // `fn(): int`, callable with no further args.
        let (hir, index, res) = build(&format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal, player_hp)\n\
             ~ temp g = bind(f, 5)\n~ temp r: int = call(g)\n-> DONE\n"
        ));
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn over_binding_more_than_the_remaining_param_row_is_a_typed_mismatch_error() {
        // `f`'s remaining row has one param (`amount`); binding two is an
        // over-bind, not an arity mismatch — `bind` never truncates.
        let (hir, index, res) = build(&format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal, player_hp)\n\
             ~ temp g = bind(f, 5, 6)\n-> DONE\n"
        ));
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E063
                && d.message.contains("supplies 2")
                && d.message.contains("1 parameter")),
            "{diags:?}"
        );
    }

    #[test]
    fn bind_argument_type_mismatch_is_a_typed_mismatch_error() {
        let (hir, index, res) = build(&format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal, player_hp)\n\
             ~ temp g = bind(f, \"lots\")\n-> DONE\n"
        ));
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "{diags:?}"
        );
    }

    #[test]
    fn unknown_callee_in_bind_is_an_escape_error() {
        let (hir, index, res) = build("=== main(g) ===\n~ temp b = bind(g, 1)\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E065
                && d.message.contains("called as a function value")),
            "{diags:?}"
        );
    }

    #[test]
    fn conflicted_callee_in_bind_is_a_conflicted_escape_error() {
        let (hir, index, res) = build(
            "=== main ===\n~ temp f = 1\n{f == \"x\":\n  no\n}\n~ temp b = bind(f, 1)\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E066
                && d.message.contains("called as a function value")),
            "{diags:?}"
        );
    }

    #[test]
    fn explicit_call_and_bind_checks_never_surface_under_gradual() {
        // The real production gate (mirrors `fn_value_call_checks_never_
        // surface_under_gradual`): `finish_analysis` only calls
        // `strict::check` under `types = strict` — `call`/`bind` stay
        // advisory under gradual, the runtime fault their backstop.
        let parsed = brink_syntax::parse(&format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal, player_hp)\n\
             ~ temp r: int = call(f, 5, 6)\n~ temp g = bind(f, \"lots\")\n-> DONE\n"
        ));
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            // These tests TEST gradual behavior — explicit opt-out knob
            // (#1127: the brink dialect's implicit default is now strict).
            types: Some(TypePolicy::Gradual),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            !result.diagnostics.iter().any(|d| matches!(
                d.code,
                DiagnosticCode::E063 | DiagnosticCode::E065 | DiagnosticCode::E066
            )),
            "gradual must never surface call()/bind() value-call checks: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn strict_explicit_call_mismatch_fires_through_the_real_pipeline() {
        // analyze_with_options -> finish_analysis -> whole_project_
        // diagnostics -> strict::check — the wiring, not just the unit.
        let parsed = brink_syntax::parse(&format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal, player_hp)\n\
             ~ temp r: int = call(f, \"x\")\n-> DONE\n"
        ));
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "{:?}",
            result.diagnostics
        );
    }

    #[test]
    fn strict_bind_over_bind_fires_through_the_real_pipeline() {
        let parsed = brink_syntax::parse(&format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal, player_hp)\n\
             ~ temp g = bind(f, 5, 6)\n-> DONE\n"
        ));
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("supplies 2")),
            "{:?}",
            result.diagnostics
        );
    }

    // ── TM-4b structs wiring (docs/typed-mode-spec.md §6) ──────────────

    #[test]
    fn strict_check_wires_in_struct_construction_errors_through_the_real_pipeline() {
        // Exercises the full production path (`analyze_with_options` ->
        // `finish_analysis` -> `whole_project_diagnostics` ->
        // `strict::check` -> `crate::structs::check`), not `structs::check`
        // in isolation — proves the wiring, not just the unit.
        let parsed = brink_syntax::parse(
            "STRUCT Point = #{x: float, y: float}\n\
             === main ===\n~ p = Point#{x: 1.0}\n-> DONE\n",
        );
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E069),
            "missing field must surface through the real strict pipeline: {:?}",
            result.diagnostics
        );
    }

    // ── T1e-1 path projections (docs/t1e-spec.md §6, issue #831) ──────

    #[test]
    fn strict_check_wires_in_ref_projection_segment_errors_through_the_real_pipeline() {
        // Same "exercises the full production path, not the unit in
        // isolation" rationale as the struct-construction test just above.
        let parsed = brink_syntax::parse(
            "STRUCT NPC = #{hp: int}\n\
             VAR npc: NPC = NPC#{hp: 10}\n\
             === function heal(ref hp, k) ===\n~ hp = hp + k\n\n\
             === main ===\n~ heal(ref npc.mana, 5)\n-> DONE\n",
        );
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E098),
            "unknown field segment must surface through the real strict pipeline: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn ref_projection_segment_errors_never_surface_under_gradual() {
        let parsed = brink_syntax::parse(
            "STRUCT NPC = #{hp: int}\n\
             VAR npc: NPC = NPC#{hp: 10}\n\
             === function heal(ref hp, k) ===\n~ hp = hp + k\n\n\
             === main ===\n~ heal(ref npc.mana, 5)\n-> DONE\n",
        );
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            // These tests TEST gradual behavior — explicit opt-out knob
            // (#1127: the brink dialect's implicit default is now strict).
            types: Some(TypePolicy::Gradual),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E098),
            "gradual must never surface ref-projection segment errors: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn struct_construction_errors_never_surface_under_gradual() {
        let parsed = brink_syntax::parse(
            "STRUCT Point = #{x: float, y: float}\n\
             === main ===\n~ p = Point#{x: 1.0}\n-> DONE\n",
        );
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            // These tests TEST gradual behavior — explicit opt-out knob
            // (#1127: the brink dialect's implicit default is now strict).
            types: Some(TypePolicy::Gradual),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E069
                    || d.code == DiagnosticCode::E070
                    || d.code == DiagnosticCode::E071),
            "gradual must never surface construction errors: {:?}",
            result.diagnostics
        );
    }

    // ── Issue #1995/#1920: `ref` parameter arguments are invariant ───────

    /// The ruling's own worked example (#1995), a direct call: `scale`'s
    /// `ref x` parameter is declared `float`, and the caller passes a bare
    /// `int` cell (T1e §2: the `ref` sigil is only required to bind a
    /// *projection* like `npc.hp`; a bare durable-cell argument at a `ref`
    /// position needs no sigil — see `record_ref_param_writes`'s doc). No
    /// sigil means `arg_tys` infers `i` as an ordinary `Ty::Int` read, not
    /// the always-`Unknown` `Expr::RefArg` escape, so this exercises the
    /// checked path (`ref_assignable`), not the projection-typed one T1e
    /// deliberately leaves unchecked. `assignable(Float, Int)` is `true`
    /// (by-value widening would let this through), but a `ref` slot writes
    /// back through the caller's own storage — `ref_assignable` requires an
    /// exact match, so this is `E063` under strict.
    #[test]
    fn direct_call_ref_param_widening_is_rejected_under_strict() {
        let parsed = brink_syntax::parse(
            "=== function scale(ref x: float, k: int): float ===\n\
             ~ x = x * k\n~ return x\n\
             VAR i = 3\n\
             === main ===\n~ scale(i, 2)\n-> DONE\n",
        );
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "a ref argument's int cell must not widen into a declared-float ref \
             parameter: {:?}",
            result.diagnostics
        );
    }

    /// The by-value sibling of the same call stays clean: passing an
    /// exactly-`float` cell into the `ref` slot, plus an `int` into `k`'s
    /// ordinary by-value `int` parameter, is unaffected by the invariant
    /// check above — it only rejects widening at the `ref` position.
    #[test]
    fn direct_call_by_value_param_is_unaffected_by_ref_invariance() {
        let (hir, index, res) = build(
            "=== function scale(ref x: float, k: int): float ===\n\
             ~ x = x * k\n~ return x\n\
             VAR f: float = 1.0\n\
             === main ===\n~ scale(f, 2)\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            !diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument")),
            "a well-typed ref argument plus an exactly-typed by-value argument must stay \
             clean: {diags:?}"
        );
    }

    /// The UFCS-desugared sibling (#1881/PR #1914): `i.scale()` desugars to
    /// `scale(i)` under D5's auto-ref (`scale`'s first parameter is `ref`),
    /// so `i`'s `int` receiver lands in the same `ref float` slot the
    /// direct-call test above exercises. Must reject uniformly — this is
    /// exactly the "same laxity in a different spelling" the ruling warns
    /// about.
    ///
    /// **Native, not ink** (rule 12c): UFCS's multi-segment callee-path
    /// shape (`ink_never_produces_a_multi_segment_callee_path`, this same
    /// module's `ufcs`-adjacent tests) is a `.brink`-only surface — an ink
    /// fixture's `i.scale()` never reaches `try_free_fn_desugar` at all, so
    /// this must go through `build_native`/`native_strict_diags`, not
    /// `build`/`brink_syntax::parse`.
    #[test]
    fn ufcs_ref_receiver_widening_is_rejected_under_strict() {
        let diags = native_strict_diags(
            "var i: int = 3;\n\
             fn scale(ref x: float): float {\n  x = x * 2.0;\n  return x;\n}\n\
             fn main() {\n  let r = i.scale();\n}\n",
        );
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "a UFCS auto-ref receiver's int cell must not widen into a declared-float \
             ref parameter either: {diags:?}"
        );
    }

    /// Review finding on this issue's own PR (BLOCKING): the direct-call
    /// check's argument-mismatch fact used to be gated on
    /// `!self.arg_is_observed_local(arg)` for the whole check, not just the
    /// non-`ref` arm — which silently skipped a `ref` widening whenever the
    /// argument was a bare Param/Temp local, because `unify(Int, Float)`
    /// never goes `Conflicted`, so `observe`'s own join never reports it as
    /// `E066` either. `direct_call_ref_param_widening_is_rejected_under_
    /// strict` above only exercises a global `VAR` argument — the one
    /// argument kind the skip never covered — so it passed identically with
    /// this exact soundness hole still open. This is the "same laxity in a
    /// different spelling" the ruling warns about, on the **native** local
    /// (`let`) spelling.
    #[test]
    fn direct_call_ref_param_widening_through_a_local_is_rejected_under_strict() {
        let diags = native_strict_diags(
            "fn scale(ref x: float): float {\n  x = x * 2.0;\n  return x;\n}\n\
             fn main() {\n  let i: int = 3;\n  scale(i);\n}\n",
        );
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "a bare local int cell must not widen into a declared-float ref parameter \
             either, the same as the global-VAR case above: {diags:?}"
        );
    }

    /// The **ink** sibling of the test above: a `~ temp` argument at a `ref`
    /// position must be checked the same way a native `let` local is.
    #[test]
    fn direct_call_ref_param_widening_through_an_ink_temp_is_rejected_under_strict() {
        let parsed = brink_syntax::parse(
            "=== function scale(ref x: float, k: int): float ===\n\
             ~ x = x * k\n~ return x\n\
             === main ===\n~ temp i: int = 3\n~ scale(i, 2)\n-> DONE\n",
        );
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "a `~ temp` int cell must not widen into a declared-float ref parameter \
             either: {:?}",
            result.diagnostics
        );
    }

    // ── Issue #2001: `#fn` creation-site ref-invariance ───────────────────

    /// Repro from #2001 (the tracked remainder of #1995/#1920 left after PR
    /// #1999): `#fn(target, args…)`'s bound-argument loop
    /// (`InferPass::infer_fn_literal`) never ran *any* argument-type check —
    /// neither `ref_assignable` nor `assignable` — even though this literal
    /// **is** the by-ref binding site (docs/t1c-spec.md §2: "all `ref`
    /// params must be bound at creation"), the one place a `Ty::Fn` value's
    /// remaining param row can never contain a `ref` param. `#fn`'s own
    /// `fn_values::check` (`E080`) only checks that a `ref` position is
    /// bound to *some* durable cell — never that the cell's static type
    /// agrees with the declared `ref` param type — so this is a genuinely
    /// separate gap from that check. Ink-only fixture (`#fn`'s binding form
    /// is ink-only, ruled 2026-08-01 per #1862).
    #[test]
    fn fn_literal_ref_param_widening_is_rejected_under_strict() {
        let parsed = brink_syntax::parse(
            "=== function scale(ref x: float, k: int): float ===\n\
             ~ x = x * k\n~ return x\n\
             VAR i = 3\n\
             === main ===\n~ temp f = #fn(scale, i)\n~ temp r: float = f(2)\n-> DONE\n",
        );
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "a #fn-bound int cell must not widen into a declared-float ref parameter \
             either: {:?}",
            result.diagnostics
        );
    }

    /// The by-value sibling: binding an ordinary (non-`ref`) `int` argument
    /// into `k` alongside an exactly-typed `ref float` binding must stay
    /// clean — the invariant check only rejects widening at the `ref`
    /// position, and #2001 explicitly declines to add a *new* by-value
    /// check at this creation site (that is its own scope call per the
    /// issue body, not assumed yes).
    #[test]
    fn fn_literal_by_value_param_is_unaffected_by_ref_invariance() {
        let (hir, index, res) = build(
            "=== function scale(ref x: float, k: int): float ===\n\
             ~ x = x * k\n~ return x\n\
             VAR f: float = 1.0\n\
             === main ===\n~ temp fv = #fn(scale, f, 2)\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            !diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument")),
            "a well-typed ref argument plus an exactly-typed by-value argument bound at \
             creation must stay clean: {diags:?}"
        );
    }

    // ── Issue #2127: divert-with-args (`-> knot(args)`) `ref`-position ────
    //    argument checking

    /// Repro from #2127: `InferPass::infer_target` (the `-> knot(args)`
    /// divert-with-args site) computed `arg_tys` and then explicitly
    /// discarded it (`let _ = arg_tys;`) — it called
    /// `record_ref_param_writes` so a `ref` param's *write* was tracked for
    /// effect purposes, but never compared the argument's static type
    /// against the declared param type in either direction. Same shape as
    /// `direct_call_ref_param_widening_is_rejected_under_strict` above: a
    /// bare `int` `VAR` cell must not widen into a declared-`float` `ref`
    /// parameter, this time reached via a divert rather than a call
    /// expression. Uses a plain (non-`function`) knot — `-> ` is the
    /// ordinary way to reach one, unlike `scale(...)`'s call-expression
    /// sibling tests, which exercise a `function` knot.
    #[test]
    fn divert_target_ref_param_widening_is_rejected_under_strict() {
        let parsed = brink_syntax::parse(
            "=== scale(ref x: float, k: int) ===\n\
             ~ x = x * k\n-> DONE\n\
             VAR i = 3\n\
             === main ===\n-> scale(i, 2)\n",
        );
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "a divert-with-args int cell must not widen into a declared-float ref \
             parameter either: {:?}",
            result.diagnostics
        );
    }

    /// Review finding (BLOCKING) on this issue's own PR: the test above only
    /// exercises a global `VAR` argument — per the precedent set on the
    /// sibling direct-call check (`direct_call_ref_param_widening_is_
    /// rejected_under_strict`'s own doc, and the review finding that
    /// produced `direct_call_ref_param_widening_through_a_local_is_
    /// rejected_under_strict`/`..._through_an_ink_temp_...` above), a global
    /// `VAR` is "the one argument kind the observed-local skip never
    /// covered" — `arg_is_observed_local` only recognizes a bare
    /// Param/Temp local, not a `VAR`. Rewriting the new guard as
    /// `!observed && !ref_assignable(...)` (dropping the `(!observed ||
    /// assignable(...))` carve-out this fix mirrors from `infer_call`)
    /// would leave the VAR-only test above green, since a VAR argument is
    /// never "observed" in the first place. This is the divert-target
    /// sibling of `direct_call_ref_param_widening_through_an_ink_temp_is_
    /// rejected_under_strict`: a `~ temp` local (not a global VAR) at the
    /// `ref` position must still be rejected.
    #[test]
    fn divert_target_ref_param_widening_through_an_ink_temp_is_rejected_under_strict() {
        let parsed = brink_syntax::parse(
            "=== scale(ref x: float, k: int) ===\n\
             ~ x = x * k\n-> DONE\n\
             === main ===\n~ temp i: int = 3\n-> scale(i, 2)\n",
        );
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "a `~ temp` int cell must not widen into a declared-float ref parameter \
             at a divert target either: {:?}",
            result.diagnostics
        );
    }

    /// The by-value sibling: an exactly-typed `ref float` argument alongside
    /// a `float`-declared by-value param `k` fed an `int` literal must stay
    /// clean — #2127 deliberately leaves by-value divert-target argument
    /// checking unimplemented (its own design call, same posture #2001 took
    /// for `infer_fn_literal`), so this proves the `ref`-only check doesn't
    /// spuriously fire on the by-value position either.
    ///
    /// Review finding (BLOCKING) on this issue's own PR: the original
    /// fixture passed the int literal `2` into `k: int` — an exact match
    /// that stays clean whether by-value positions are unchecked (actual),
    /// checked covariantly, or checked invariantly, so it could not
    /// distinguish any of those. `k` is declared `float` here instead
    /// (still fed the int literal `2`): `assignable(Float, Int)` is `true`
    /// (the covariant widening direction) but `ref_assignable(Float, Int)`
    /// is `false`, so this fixture is clean today under the actual
    /// (by-value-unchecked) behavior and goes red the moment ref
    /// invariance ever leaks into a by-value slot.
    #[test]
    fn divert_target_by_value_param_is_unaffected_by_ref_invariance() {
        let (hir, index, res) = build(
            "=== scale(ref x: float, k: float) ===\n\
             ~ x = x * k\n-> DONE\n\
             VAR f: float = 1.0\n\
             === main ===\n-> scale(f, 2)\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            !diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument")),
            "a well-typed ref argument plus a covariantly-widened by-value argument at a \
             divert target must stay clean: {diags:?}"
        );
    }

    /// The `root_content` sibling of the test above (mirrors
    /// `ink_root_content_direct_call_ref_widening_is_checked`): a divert
    /// with a ref-mismatched argument written at an ink file's literal
    /// top-level weave must still reach a diagnostic through
    /// `check_direct_call_args`'s existing `root_content` synthetic-ID
    /// handling (issue #1903) — this fix pushes onto the same
    /// `direct_call_arg_mismatches` vec every other producer already uses,
    /// so no additional plumbing should be needed, but this proves it.
    #[test]
    fn ink_root_content_divert_target_ref_widening_is_checked() {
        let src = "VAR i = 3\n\
                   -> scale(i, 2)\n\
                   === scale(ref x: float, k: int) ===\n\
                   ~ x = x * k\n-> DONE\n";
        let (hir, index, res) = build(src);
        assert!(
            !hir.root_content.stmts.is_empty(),
            "fixture precondition: the ink frontend must populate root_content"
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "a divert-with-args ref-argument mismatch written at file root must be \
             reported, not silently dropped: {diags:?}"
        );
    }

    /// Issue #2136: before native's `hir::lower_native::body::lower_
    /// divert_target` wired `-> knot(args)` call args into
    /// `DivertTarget::args`, this exact fixture failed to *compile* at all
    /// (a hard `E129`, "parses but has no HIR lowering yet") — PR #2128's
    /// review disposition confirmed directly that #2127/#2128's ref-
    /// position check therefore had structurally nothing to check on the
    /// native surface, since `target.args` was always empty by the time
    /// this pass ran. Now the arg survives lowering and reaches
    /// `infer_target` exactly like the ink-dialect fixture above — this is
    /// the native sibling of `divert_target_ref_param_widening_is_
    /// rejected_under_strict`, proving #2127/#2128's existing check now
    /// fires on a native fixture with no changes to `brink-analyzer`
    /// itself.
    #[test]
    fn divert_target_ref_param_widening_is_rejected_under_strict_on_native() {
        let diags = native_strict_diags(
            "fn scale(ref x: float, k: int) {\n  x = x * k;\n}\n\
             var i: int = 3;\n\
             flow main() {\n  -> scale(i, 2)\n}\n",
        );
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "a divert-with-args int cell must not widen into a declared-float ref \
             parameter on native either: {diags:?}"
        );
    }

    // ── Review finding on #2001: root_content reaches check_direct_call_args ──

    /// Review finding (BLOCKING) on this issue's own PR: `check_direct_call_args`
    /// built `def_ids` from `hir.knots` + stitches only, never gaining the
    /// #1903 `root_content` synthetic-ID block its structurally parallel
    /// sibling `check_typed_assign_mismatches` has — so a direct-call
    /// argument-type mismatch written at an ink file's literal top-level
    /// weave was recorded by inference but silently dropped by strict,
    /// never reaching a diagnostic. Mirrors
    /// `ink_root_content_declared_temp_init_is_checked` above, but for
    /// `check_direct_call_args`'s own fact kind, and MUST fail with the
    /// `check_direct_call_args` `root_content` block reverted.
    #[test]
    fn ink_root_content_direct_call_ref_widening_is_checked() {
        let src = "VAR i = 3\n\
                   ~ scale(i, 2)\nHello.\n-> END\n\
                   === function scale(ref x: float, k: int): float ===\n\
                   ~ x = x * k\n~ return x\n";
        let (hir, index, res) = build(src);
        assert!(
            !hir.root_content.stmts.is_empty(),
            "fixture precondition: the ink frontend must populate root_content"
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "a direct-call ref-argument mismatch written at file root must be \
             reported, not silently dropped: {diags:?}"
        );
    }

    /// The `#fn` creation-site sibling of the test above, in `root_content` —
    /// same gap, same fix, same fact kind #2001 introduced.
    #[test]
    fn ink_root_content_fn_literal_ref_widening_is_checked() {
        let src = "VAR i = 3\n\
                   ~ temp f = #fn(scale, i)\nHello.\n-> END\n\
                   === function scale(ref x: float, k: int): float ===\n\
                   ~ x = x * k\n~ return x\n";
        let (hir, index, res) = build(src);
        assert!(
            !hir.root_content.stmts.is_empty(),
            "fixture precondition: the ink frontend must populate root_content"
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
            "a #fn-bound ref-argument mismatch written at file root must be \
             reported, not silently dropped: {diags:?}"
        );
    }

    // ── Issue #1903: `root_content` reaches the strict walk ──────────────

    /// Issue #1903 regression. `collect_defs` walked only `hir.knots`, so
    /// `root_content` — which the **ink** frontend populates from the file's
    /// literal top-level weave — never reached inference, and a declared-type
    /// violation written at file root was silently unchecked.
    ///
    /// ⚠ **Note the dialect.** This fixture is ink, not native, and that is
    /// load-bearing rather than incidental: `lower_native::entry_root_content`
    /// makes a `.brink` file's `root_content` either empty or a *single
    /// synthesized `Divert`* to `main`, never user statements. So native
    /// root content holds nothing type-bearing and cannot exercise this path
    /// at all — a `.brink` fixture would pass identically with the fix
    /// reverted. See this test's companion,
    /// [`native_root_content_holds_no_type_bearing_statements`].
    #[test]
    fn ink_root_content_declared_temp_init_is_checked() {
        let src = "~ temp n: int = \"hello\"\nHello.\n-> END\n";
        let (hir, index, res) = build(src);
        assert!(
            !hir.root_content.stmts.is_empty(),
            "fixture precondition: the ink frontend must populate root_content"
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E063),
            "a declared-type violation at file root must be reported: {diags:?}"
        );
    }

    /// Pins the asymmetry the test above depends on: a native file's
    /// `root_content` is only ever the synthesized entry `Divert`, so #1903's
    /// walk finds no assignment or temp there. If native ever grows real
    /// file-root statements this test fails, which is the signal to add a
    /// native counterpart of the test above.
    #[test]
    fn native_root_content_holds_no_type_bearing_statements() {
        let (hir, _index, _res) = build_native("flow main() {\n  Hello.\n}\n");
        assert_eq!(hir.root_content.stmts.len(), 1);
        assert!(
            matches!(hir.root_content.stmts[0], brink_ir::Stmt::Divert(_)),
            "native root_content must be the synthesized entry divert, got {:?}",
            hir.root_content.stmts[0]
        );
    }
}
