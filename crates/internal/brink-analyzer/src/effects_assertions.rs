//! T2-2 (docs/effects-spec.md §10, issue #861): compile-time check of every
//! `#@effects(…)` assertion against its definition's inferred effect row —
//! the *only* diagnostic the T2 sitting-2 ruling (2026-07-14) assigns this
//! surface, **exceedance** (`E103`): the inferred row is not covered by
//! (⊄) the declared upper bound. Per that ruling there is no drift policy —
//! an inferred row that is *narrower* than its bound is silent; nothing
//! else warns.
//!
//! One other error class lives here: a clause naming an identifier that
//! isn't a declared global cell (`reads`/`writes`) or a declared `EXTERNAL`
//! (`calls`) anywhere in the project (`E102`). This is ordinary directive
//! well-formedness, not "drift" — the assertion can't even be built into a
//! row without it. The grammar-level `E100`/`E101` (missing argument,
//! malformed clause) are minted by `brink-ir`'s directive recognizer before
//! this module ever runs.
//!
//! Callers only run this under `dialect = brink`, mirroring TM-2's
//! annotation-content precedent (`per_file_diagnostics`'s doc): under
//! `strict-ink` the directive is already rejected whole by `dialect_gate`
//! (`E051`), so critiquing its declared names would be noise.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use brink_format::DefinitionId;
use brink_ir::{
    ContainerPtr, Diagnostic, DiagnosticCode, EffectsAssertion, FileId, HirFile, SymbolIndex,
    SymbolKind,
};
use rowan::TextRange;

use crate::infer::EffectRow;
use crate::resolve::{ImportScope, lookup_by_name};

/// The index + import scope every name lookup in this module needs together
/// (issue #881) — bundled so `check_one` doesn't carry them as two separate
/// parameters (`clippy::too_many_arguments`).
struct Ctx<'a> {
    index: &'a SymbolIndex,
    scope: &'a ImportScope,
}

/// Check every knot/stitch's `#@effects(…)` assertion in `hir` against
/// `rows` — that def's inferred [`EffectRow`], however the caller computed
/// it: the whole-project pure [`crate::effects_project`] for the analyzer's
/// monolithic path, or, for the salsa-memoized production path, a small map
/// built from individual per-def `effects(def)` queries (only for the defs
/// that actually carry an assertion, preserving the advisory/lazy
/// invariant — an unannotated project never triggers effect inference at
/// all).
///
/// A def whose own id can't be resolved, or whose row is missing from
/// `rows`, produces no diagnostic here — both are the caller's contract to
/// uphold (every assertion-carrying def gets an entry), not a case this
/// function can distinguish from "not computed yet".
///
/// `scope` is `hir`'s own [`ImportScope`] (issue #881, the T2 follow-up to
/// M-2d/#790): a `reads`/`writes`/`calls` clause name is resolved through the
/// exact same import-scoped [`lookup_by_name`] the reference resolver uses,
/// so a `#@effects` assertion in a file that imports one of several
/// same-name cross-module cells binds to *that* importer's cell — never a
/// flat first-inserted winner that could silently name a different module's
/// definition than the one the body's own inferred row actually touches.
#[must_use]
pub fn check(
    file: FileId,
    hir: &HirFile,
    index: &SymbolIndex,
    scope: &ImportScope,
    rows: &BTreeMap<DefinitionId, EffectRow>,
) -> Vec<Diagnostic> {
    let ctx = Ctx { index, scope };
    let mut out = Vec::new();
    for knot in &hir.knots {
        let kind = match knot.ptr {
            ContainerPtr::Knot(_) => SymbolKind::Knot,
            ContainerPtr::Stitch(_) => SymbolKind::Stitch,
        };
        check_one(
            file,
            knot.effects_assertion.as_ref(),
            kind,
            &knot.name.text,
            &ctx,
            rows,
            &mut out,
        );
        for stitch in &knot.stitches {
            let qualified = format!("{}.{}", knot.name.text, stitch.name.text);
            check_one(
                file,
                stitch.effects_assertion.as_ref(),
                SymbolKind::Stitch,
                &qualified,
                &ctx,
                rows,
                &mut out,
            );
        }
    }
    out
}

/// Every def carrying a `#@effects(…)` assertion in `hir`, paired with the
/// [`DefinitionId`] the exceedance check needs its row for — the seam a
/// salsa caller uses to fetch exactly those rows (and no others) via the
/// per-def `effects(def)` query, keeping unannotated projects inference-free.
#[must_use]
pub fn assertion_defs(hir: &HirFile, index: &SymbolIndex, file: FileId) -> Vec<DefinitionId> {
    let mut out = Vec::new();
    for knot in &hir.knots {
        let kind = match knot.ptr {
            ContainerPtr::Knot(_) => SymbolKind::Knot,
            ContainerPtr::Stitch(_) => SymbolKind::Stitch,
        };
        if knot.effects_assertion.is_some()
            && let Some(id) = find_def_id(index, file, kind, &knot.name.text)
        {
            out.push(id);
        }
        for stitch in &knot.stitches {
            if stitch.effects_assertion.is_some() {
                let qualified = format!("{}.{}", knot.name.text, stitch.name.text);
                if let Some(id) = find_def_id(index, file, SymbolKind::Stitch, &qualified) {
                    out.push(id);
                }
            }
        }
    }
    out
}

fn check_one(
    file: FileId,
    assertion: Option<&EffectsAssertion>,
    kind: SymbolKind,
    name: &str,
    ctx: &Ctx<'_>,
    rows: &BTreeMap<DefinitionId, EffectRow>,
    out: &mut Vec<Diagnostic>,
) {
    let Some(assertion) = assertion else {
        return;
    };
    let Some(def_id) = find_def_id(ctx.index, file, kind, name) else {
        return;
    };
    let Some(inferred) = rows.get(&def_id) else {
        return;
    };

    // ── NS-A2 (issue #1108): the output/fault dimension assertions —
    // `silent` (no emits) and `total` (no faults), each exceedance-only
    // with its own code. Opaque rows are unbounded on every dimension
    // (spec §3), so they exceed any concrete assertion.
    if assertion.silent && (inferred.emits || inferred.opaque) {
        out.push(Diagnostic {
            file,
            range: assertion.range,
            code: DiagnosticCode::E108,
            message: if inferred.opaque {
                "inferred effects are unbounded (a call through a function value, or an                  unresolved callee) — the `silent` assertion cannot cover this definition"
                    .to_string()
            } else {
                "inferred effects exceed the `silent` assertion: the definition can produce                  content (a content line, or a transitive call to an emitter)"
                    .to_string()
            },
        });
    }
    if assertion.total && (inferred.faults || inferred.opaque) {
        out.push(Diagnostic {
            file,
            range: assertion.range,
            code: DiagnosticCode::E109,
            message: if inferred.opaque {
                "inferred effects are unbounded (a call through a function value, or an                  unresolved callee) — the `total` assertion cannot cover this definition"
                    .to_string()
            } else {
                "inferred effects exceed the `total` assertion: the definition can raise a                  turn-terminating fault"
                    .to_string()
            },
        });
    }

    // ── The state-row bound (`pure`, or one or more reads/writes/calls
    // clauses) — the pre-NS-A2 `E102`/`E103` surface, unchanged. An
    // assertion carrying only `silent`/`total` leaves the state row
    // unbounded, so there is nothing further to check.
    if !assertion.pure
        && assertion.reads.is_empty()
        && assertion.writes.is_empty()
        && assertion.calls.is_empty()
    {
        return;
    }

    let mut well_formed = true;
    let mut declared_reads = BTreeSet::new();
    for n in &assertion.reads {
        if let Some(id) = resolve_cell(ctx, n) {
            declared_reads.insert(id);
        } else {
            out.push(unknown_name_diagnostic(file, assertion.range, n));
            well_formed = false;
        }
    }
    let mut declared_writes = BTreeSet::new();
    for n in &assertion.writes {
        if let Some(id) = resolve_cell(ctx, n) {
            declared_writes.insert(id);
        } else {
            out.push(unknown_name_diagnostic(file, assertion.range, n));
            well_formed = false;
        }
    }
    let mut declared_calls = BTreeSet::new();
    for n in &assertion.calls {
        if external_declared(ctx, n) {
            declared_calls.insert(n.clone());
        } else {
            out.push(unknown_name_diagnostic(file, assertion.range, n));
            well_formed = false;
        }
    }
    if !well_formed {
        // Malformed names already diagnosed (E102) — skip the exceedance
        // check to avoid a confusing second diagnostic over an assertion
        // that can't even be resolved into a row yet.
        return;
    }

    // The state bound never constrains the output/fault dimensions (those
    // have their own assertion args above), so the declared row mirrors the
    // inferred row on emits/tags/faults — `covers` then compares exactly
    // the reads/writes/calls sets plus the opaque top.
    let declared_row = EffectRow {
        reads: declared_reads,
        writes: declared_writes,
        calls: declared_calls,
        opaque: false,
        emits: inferred.emits,
        tags: inferred.tags,
        faults: inferred.faults,
        // Mirrored like the other output/fault dimensions — the refined
        // bit (F29) is not part of `covers` semantics and never
        // assertable.
        faults_refined: inferred.faults_refined,
    };
    if !declared_row.covers(inferred) {
        out.push(Diagnostic {
            file,
            range: assertion.range,
            code: DiagnosticCode::E103,
            message: exceedance_message(&declared_row, inferred, ctx.index),
        });
    }
}

/// This definition's own [`DefinitionId`] — the merged index's `by_name`
/// reverse lookup, disambiguated by file + [`SymbolKind`] (mirrors
/// `infer::collect_defs`'s `def_of` construction, one name at a time
/// instead of building the whole project's map up front — this is only
/// ever called for the handful of defs that actually carry an assertion).
fn find_def_id(
    index: &SymbolIndex,
    file: FileId,
    kind: SymbolKind,
    name: &str,
) -> Option<DefinitionId> {
    index.by_name.get(name)?.iter().copied().find(|id| {
        index
            .symbols
            .get(id)
            .is_some_and(|info| info.file == file && info.kind == kind)
    })
}

/// Resolve a `reads`/`writes` clause name to a global `VAR`/`CONST`
/// [`DefinitionId`], through the same import-scoped [`lookup_by_name`] the
/// reference resolver uses (issue #881 — the T2 follow-up to M-2d/#790:
/// "twin semantic checks share one helper, never re-derive", #811's
/// lesson). Before this fix the clause was resolved by an independent
/// flat `by_name` scan picking the smallest same-named `DefinitionId`,
/// which could silently disagree with which module's cell the assertion's
/// own def actually reads/writes whenever two declared modules publicly
/// define the same name — `lookup_by_name` picks the referrer's own-module
/// candidate first, then an imported one, exactly like every other
/// reference in this file resolves.
fn resolve_cell(ctx: &Ctx<'_>, name: &str) -> Option<DefinitionId> {
    let resolved = lookup_by_name(
        ctx.index,
        ctx.scope,
        name,
        &[SymbolKind::Variable, SymbolKind::Constant],
    );
    if resolved.is_some() {
        return resolved;
    }
    // NS-A6 (issue #1112, `docs/stdlib-spec.md` §7): `rng` names the
    // compiler-owned `std::rand` RNG state cell — the cell every draw
    // verb writes — so a draw-bearing def can carry a covering bound
    // (`@[effects(writes rng)]`). A user-declared `VAR`/`CONST` named
    // `rng` shadows this (the lookup above wins), consistent with the
    // stdlib-name shadowing rule everywhere else.
    if name == "rng" {
        return Some(DefinitionId::RNG_CELL);
    }
    None
}

/// Whether `name` is a declared `EXTERNAL` visible to this file's import
/// scope (issue #881, same fix as [`resolve_cell`]). `calls` clauses match
/// [`EffectRow::calls`] by raw name (T2-1 collects external call atoms the
/// same way), so only existence of an in-scope candidate is needed, not its
/// id.
fn external_declared(ctx: &Ctx<'_>, name: &str) -> bool {
    lookup_by_name(ctx.index, ctx.scope, name, &[SymbolKind::External]).is_some()
}

fn unknown_name_diagnostic(file: FileId, range: TextRange, name: &str) -> Diagnostic {
    Diagnostic {
        file,
        range,
        code: DiagnosticCode::E102,
        message: format!(
            "the effects assertion names `{name}`, which isn't a declared global VAR/CONST or EXTERNAL anywhere in the project"
        ),
    }
}

/// Build the `E103` exceedance message: an opaque inferred row (a call
/// through a function value, or an unresolved callee — spec §3) can never
/// be bounded by a concrete assertion, so it gets its own explanatory
/// message; otherwise the message lists every atom the assertion under-
/// declares.
fn exceedance_message(declared: &EffectRow, inferred: &EffectRow, index: &SymbolIndex) -> String {
    if inferred.opaque {
        return "inferred effects are unbounded (a call through a function value, or an \
                 unresolved callee) — no effects assertion can cover this definition"
            .to_string();
    }
    let name_of = |id: &DefinitionId| {
        // The compiler-owned RNG cell has no symbol-index entry — name it
        // the way the assertion surface spells it (`writes rng`).
        if *id == DefinitionId::RNG_CELL {
            return "rng (the std::rand RNG state cell)".to_string();
        }
        index
            .symbols
            .get(id)
            .map_or_else(|| format!("{id:?}"), |info| info.name.clone())
    };
    let mut parts = Vec::new();
    let extra_reads: Vec<String> = inferred
        .reads
        .difference(&declared.reads)
        .map(name_of)
        .collect();
    if !extra_reads.is_empty() {
        parts.push(format!("reads {}", extra_reads.join(", ")));
    }
    let extra_writes: Vec<String> = inferred
        .writes
        .difference(&declared.writes)
        .map(name_of)
        .collect();
    if !extra_writes.is_empty() {
        parts.push(format!("writes {}", extra_writes.join(", ")));
    }
    let extra_calls: Vec<String> = inferred
        .calls
        .difference(&declared.calls)
        .cloned()
        .collect();
    if !extra_calls.is_empty() {
        parts.push(format!("calls {}", extra_calls.join(", ")));
    }
    format!(
        "inferred effects exceed the effects assertion's declared bound: {}",
        parts.join("; ")
    )
}
