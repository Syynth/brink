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
//! This slice does **not** implement: `VAR`/`CONST` cross-type-reassignment
//! detection (the inference substrate never joins a global's declaration-
//! derived type against its assignment sites — `infer::body`'s `observe`
//! only accumulates for `Param`/`Temp` locals; extending it is a `BodyCtx`
//! change, fenced off by #619 itself), or the boundary-annotation-*required*
//! diagnostic (spec's "host-callable functions... and entry points require
//! explicit annotations" has no ratified, mechanically-checkable definition
//! of either term in the codebase today — inventing one here would be
//! unilateral architecture, not wiring). The `int()`/`float()`/`string()`
//! pure conversion intrinsics (TM-3 completion, issue #659) now exist —
//! VM-native ops plus the `conversions` module's strict-mode domain check,
//! wired in below alongside `structs::check`.

use std::collections::{BTreeMap, BTreeSet};

use brink_format::DefinitionId;
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
///    them are reachable through this table at all.
/// 3. **`[lints]` per-code override**, for a `Warning`-base code only:
///    `Deny` → `Error`; `Allow` → `Warning`, immune to step 4;
///    `Warn`/unset → falls through to step 4 exactly like an unconfigured
///    code.
/// 4. **`deny-warnings`**: a `Warning`-base code with no override (or an
///    explicit `Warn`) becomes `Error` if `lints.deny_warnings` is set —
///    the `-D warnings` equivalent.
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

    if base != brink_ir::Severity::Warning {
        return base;
    }

    match lints.overrides.get(code.as_str()) {
        Some(LintLevel::Deny) => brink_ir::Severity::Error,
        Some(LintLevel::Allow) => brink_ir::Severity::Warning,
        Some(LintLevel::Warn) | None => {
            if lints.deny_warnings {
                brink_ir::Severity::Error
            } else {
                brink_ir::Severity::Warning
            }
        }
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
/// the `handle<K>` annotation-firewall vocabulary source, threaded through
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
    out.extend(check_void_assignments(files, index, resolutions));
    // T1c (docs/t1c-spec.md §4/§8): calls through function values are
    // statically checked under strict — the facts inference already
    // recorded map onto the existing TM-3 codes (E065/E066 escapes, E063
    // typed mismatches), never parallel ones.
    out.extend(check_value_calls(files, index, inference));
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
    out
}

/// Unknown-escape (`E065`) + Conflicted-escape (`E066`) over every inferable
/// def's params, return type (function knots only — an ordinary knot has no
/// return-value concept), and temps.
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
                    &mut out,
                );
            }
            for stitch in &knot.stitches {
                let qualified = format!("{}.{}", knot.name.text, stitch.name.text);
                if let Some(id) =
                    annotations::def_id_for(index, file, SymbolKind::Stitch, &qualified)
                {
                    check_def(
                        id,
                        file,
                        &qualified,
                        stitch.name.range,
                        false,
                        None,
                        &stitch.params,
                        &stitch.body,
                        &names,
                        inference,
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

    // Return type: only function knots carry return-value semantics; a
    // `void`-annotated function never needs a concrete return type either.
    if is_function {
        let has_void_annotation = return_type
            .is_some_and(|rt| matches!(rt, TypeExpr::Named { name, .. } if name == "void"));
        // Issue #1028: a function whose body never carries a value-returning
        // `return <expr>` — it either falls off the end or only ever
        // bare-`return`s — infers as void exactly like an explicit `: void`
        // annotation, rather than escaping as Unknown. typed-mode-spec §3
        // only documents `void` as something the *author* writes for a
        // no-return function; it is silent on what a same-shaped body
        // without the annotation should infer as. The conservative reading
        // (spec gap, flagged in the PR) is that inference shouldn't demand
        // an annotation to say what the body already proves: nothing ever
        // flows out of it. `body_types.has_value_return` is exactly that
        // proof — `sig.return_ty.is_unknown()` alone can't distinguish
        // "never returns a value" from "returns a value inference couldn't
        // pin down" (a genuine Unknown-escape, left unaffected below).
        let is_void = has_void_annotation || !body_types.has_value_return;
        let annotated =
            is_void || return_type.is_some_and(|rt| annotations::resolve(rt, names).is_some());
        if !is_void {
            emit_escape(
                file,
                def_label,
                "return type",
                name_range,
                &sig.return_ty,
                annotated,
                out,
            );
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
        Ty::Fn(params, ret) => {
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
        Ty::Int
        | Ty::Float
        | Ty::Bool
        | Ty::String
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
/// returning function is a compile error under strict (spec §3: "assigning a
/// `void` call is an error in strict mode"). Only the assignment/temp-decl's
/// RHS *root* expression is checked — a statement-position call (`~ f()`) or
/// a call nested inside interpolation is never flagged, since neither
/// assigns the (nonexistent) result anywhere.
#[must_use]
fn check_void_assignments(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
) -> Vec<brink_ir::Diagnostic> {
    let void_defs = collect_void_defs(files, index);
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

/// Every function knot whose `): void ===` return annotation resolves to
/// `void`, by `DefinitionId`. Stitches never carry `return_type` (only
/// `Knot` does — see the field's doc comment), so only `hir.knots` entries
/// with `is_function` set are candidates, mirroring `check_escapes`' own
/// def-id lookup (`kind` tracks `knot.ptr`, since a top-level stitch
/// promoted to knot status is indexed under `SymbolKind::Stitch`, #626).
fn collect_void_defs(files: &[(FileId, &HirFile)], index: &SymbolIndex) -> BTreeSet<DefinitionId> {
    let mut out = BTreeSet::new();
    for &(file, hir) in files {
        for knot in &hir.knots {
            if !knot.is_function {
                continue;
            }
            let is_void = knot
                .return_type
                .as_ref()
                .is_some_and(|rt| matches!(rt, TypeExpr::Named { name, .. } if name == "void"));
            if !is_void {
                continue;
            }
            let kind = knot.symbol_kind();
            if let Some(id) = annotations::def_id_for(index, file, kind, &knot.name.text) {
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
                check_void_block(file, branch, void_defs, resolution_by_range, out);
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
        match part {
            ContentPart::InlineConditional(c) => {
                for branch in &c.branches {
                    check_void_block(file, &branch.body, void_defs, resolution_by_range, out);
                }
            }
            ContentPart::InlineSequence(s) => {
                for branch in &s.branches {
                    check_void_block(file, branch, void_defs, resolution_by_range, out);
                }
            }
            ContentPart::Interpolation(_)
            | ContentPart::Text(_)
            | ContentPart::Glue
            | ContentPart::Spring => {}
        }
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
                collect_temps_block(branch, names, out);
            }
        }
        Stmt::Content(c) => collect_temps_content(c, names, out),
        Stmt::LogicBlock(lb) => {
            for bs in &lb.stmts {
                collect_temps_block_stmt(bs, names, out);
            }
        }
        // An `await` condition is an expression — it declares no temps
        // (docs/flow-suspension-spec.md §3).
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
        match part {
            ContentPart::InlineConditional(c) => {
                for branch in &c.branches {
                    collect_temps_block(&branch.body, names, out);
                }
            }
            ContentPart::InlineSequence(s) => {
                for branch in &s.branches {
                    collect_temps_block(branch, names, out);
                }
            }
            ContentPart::Interpolation(_)
            | ContentPart::Text(_)
            | ContentPart::Glue
            | ContentPart::Spring => {}
        }
    }
}

fn collect_temps_block_stmt(
    bs: &BlockStmt,
    names: &annotations::TypeNames,
    out: &mut BTreeMap<String, TempDecl>,
) {
    match bs {
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

    /// T1d-2 (docs/t1d-spec.md §3): a `handle<K>`-annotated, otherwise-unused
    /// param is exempt from `E065` the same way any other resolvable
    /// annotation is — "strict kind-checking via existing TM-3 machinery",
    /// reusing the annotation-firewall exemption path, no new code needed.
    /// Reachable only when the manifest declaring `K` is registered — with
    /// none registered, the annotation doesn't resolve and the slot escapes
    /// as `Unknown` exactly like an unrecognized type name would.
    #[test]
    fn annotated_handle_param_is_exempt_from_unknown_escape_when_kind_is_registered() {
        let (hir, index, res) = build("=== noop(x: handle<AudioInstance>) ===\nHello.\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let manifest = brink_ir::HostManifest {
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

    #[test]
    fn handle_param_escapes_as_unknown_with_no_manifest_registered() {
        let (hir, index, res) = build("=== noop(x: handle<AudioInstance>) ===\nHello.\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res, None);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E065);
    }

    /// T1d-2b (issue #774, docs/t1d-spec.md §3 — the #767 acceptance
    /// criterion): "binding declared handle<AudioInstance> rejects
    /// handle<Timer> at compile time". `get_audio`/`get_timer` are leaf
    /// functions whose return type is annotated with a distinct handle
    /// kind each — their body-derived return type stays `Unknown` (`id` is
    /// never otherwise constrained), so the T1c annotation-firewall overlay
    /// supplies the concrete `Ty::Handle(K)`. `main`'s temps `a`/`b` pick
    /// those return types up purely through call-site inference (never an
    /// annotation of their own), then get compared — a genuine cross-kind
    /// handle mismatch detected *purely from body-usage inference*, exactly
    /// the gap PR #769 disclosed as deferred. Before T1d-2b threaded the
    /// manifest into `infer_project`/`solve_scc`, `handle<K>` annotations
    /// never resolved during body inference at all (an empty kind set), so
    /// `get_audio`/`get_timer` would return `Ty::Unknown`, `unify` would
    /// never see two distinct `Ty::Handle` kinds meet, and this mismatch
    /// was silently unreachable — this test is the positive case proving
    /// it is now reachable end-to-end.
    #[test]
    fn cross_kind_handle_comparison_from_body_usage_is_conflicted_under_strict() {
        let src = "\
=== function get_audio(id: int): handle<AudioInstance> ===\n~ return id\n\
=== function get_timer(id: int): handle<Timer> ===\n~ return id\n\
=== main ===\n~ temp a = get_audio(1)\n~ temp b = get_timer(1)\n{a == b:\n  ok\n}\n-> DONE\n";
        let (hir, index, res) = build(src);
        let manifest = brink_ir::HostManifest {
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
            ..Default::default()
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
        let src = "\
=== function get_audio(id: int): handle<AudioInstance> ===\n~ return id\n\
=== function get_audio2(id: int): handle<AudioInstance> ===\n~ return id\n\
=== main ===\n~ temp a = get_audio(1)\n~ temp c = get_audio2(1)\n{a == c:\n  ok\n}\n-> DONE\n";
        let (hir, index, res) = build(src);
        let manifest = brink_ir::HostManifest {
            types: vec![brink_ir::SemanticTypeDef {
                name: "AudioInstance".to_string(),
                base: brink_ir::BaseType::Handle,
                constraint: None,
                values: None,
                widget: None,
            }],
            ..Default::default()
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
    /// seam, so `handle<K>` return annotations never resolved during body
    /// inference and both `get_audio`/`get_timer` returned `Ty::Unknown`
    /// instead of their distinct handle kinds — `unify(Unknown, Unknown)`
    /// stays `Unknown`, never `Conflicted`, so no escape ever fired even
    /// with a registered manifest. Pinned here as the regression guard for
    /// the specific "manifest reaches inference, not just `signature()`"
    /// gap PR #769 disclosed.
    #[test]
    fn cross_kind_handle_mismatch_is_unreachable_without_manifest_reaching_inference() {
        let src = "\
=== function get_audio(id: int): handle<AudioInstance> ===\n~ return id\n\
=== function get_timer(id: int): handle<Timer> ===\n~ return id\n\
=== main ===\n~ temp a = get_audio(1)\n~ temp b = get_timer(1)\n{a == b:\n  ok\n}\n-> DONE\n";
        let (hir, index, res) = build(src);
        let manifest = brink_ir::HostManifest {
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
            ..Default::default()
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
    // strict`, a binding declared to take `handle<AudioInstance>` rejects a
    // `handle<Timer>` argument at compile time". T1d-2b (#774) closed this
    // for two *locals* meeting through body-usage inference (comparison,
    // reassignment); this closes the literal reading of the sentence — the
    // binding itself, at its own call site — reusing the identical
    // `known_sigs`/`observe`/`Ty::Conflicted`/`E066` machinery, no parallel
    // checking surface (see `infer::collect_external_sigs`'s doc).

    fn audio_and_timer_manifest(play_sound_param_kind: &str) -> brink_ir::HostManifest {
        brink_ir::HostManifest {
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
            externals: vec![brink_ir::ManifestExternal {
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
            }],
        }
    }

    /// The #767/#786 acceptance criterion, literally: a binding
    /// (`EXTERNAL play_sound`) declared in the manifest to take
    /// `handle<AudioInstance>` rejects a `handle<Timer>`-kinded argument at
    /// compile time under `types = strict`.
    #[test]
    fn external_binding_rejects_cross_kind_handle_argument_under_strict() {
        let src = "EXTERNAL play_sound(inst)\n\
=== function get_timer(id: int): handle<Timer> ===\n~ return id\n\
=== main ===\n~ temp t = get_timer(1)\n~ play_sound(t)\n-> DONE\n";
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
        let src = "EXTERNAL play_sound(inst)\n\
=== function get_audio(id: int): handle<AudioInstance> ===\n~ return id\n\
=== main ===\n~ temp t = get_audio(1)\n~ play_sound(t)\n-> DONE\n";
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
        let src = "EXTERNAL play_sound(inst)\n\
=== function get_timer(id: int): handle<Timer> ===\n~ return id\n\
=== main ===\n~ temp t = get_timer(1)\n~ play_sound(t)\n-> DONE\n";
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
    /// no checkable signature — the argument's own inferred kind
    /// (`AudioInstance`, from the annotated return) stays clean, same as
    /// today (this is the disclosed inline-doc-only gap
    /// `infer::collect_external_sigs`'s doc names, not a regression).
    #[test]
    fn external_binding_with_unregistered_name_is_unchecked() {
        let src = "EXTERNAL other_call(inst)\n\
=== function get_timer(id: int): handle<Timer> ===\n~ return id\n\
=== main ===\n~ temp t = get_timer(1)\n~ other_call(t)\n-> DONE\n";
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
        let (hir, index, res) = build("=== main ===\n~ temp x: array<int> = #[]\n-> DONE\n");
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
    /// `list<L>` type end-to-end, not collapse to `Unknown` (the phase-0
    /// `Sig` stub bug this issue reports). A temp assigned straight from
    /// such a VAR is the concrete, checkable consequence: before the fix,
    /// `weather`'s `Sig::value_type` fed `collect_globals` as `Ty::Unknown`
    /// (`infer::mod`'s `From<InferredType> for Ty` collapse), so `w` would
    /// spuriously trip the Unknown-escape check (`E065`) under strict even
    /// though its value is plainly a `list<Weathers>` — the same "resolved
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
}
