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
#[must_use]
pub fn check(
    file: FileId,
    hir: &HirFile,
    index: &SymbolIndex,
    rows: &BTreeMap<DefinitionId, EffectRow>,
) -> Vec<Diagnostic> {
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
            index,
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
                index,
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
    index: &SymbolIndex,
    rows: &BTreeMap<DefinitionId, EffectRow>,
    out: &mut Vec<Diagnostic>,
) {
    let Some(assertion) = assertion else {
        return;
    };
    let Some(def_id) = find_def_id(index, file, kind, name) else {
        return;
    };

    let mut well_formed = true;
    let mut declared_reads = BTreeSet::new();
    for n in &assertion.reads {
        if let Some(id) = resolve_cell(index, n) {
            declared_reads.insert(id);
        } else {
            out.push(unknown_name_diagnostic(file, assertion.range, n));
            well_formed = false;
        }
    }
    let mut declared_writes = BTreeSet::new();
    for n in &assertion.writes {
        if let Some(id) = resolve_cell(index, n) {
            declared_writes.insert(id);
        } else {
            out.push(unknown_name_diagnostic(file, assertion.range, n));
            well_formed = false;
        }
    }
    let mut declared_calls = BTreeSet::new();
    for n in &assertion.calls {
        if external_declared(index, n) {
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

    let declared_row = EffectRow {
        reads: declared_reads,
        writes: declared_writes,
        calls: declared_calls,
        opaque: false,
    };
    let Some(inferred) = rows.get(&def_id) else {
        return;
    };
    if !declared_row.covers(inferred) {
        out.push(Diagnostic {
            file,
            range: assertion.range,
            code: DiagnosticCode::E103,
            message: exceedance_message(&declared_row, inferred, index),
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
/// [`DefinitionId`]. Picks the smallest id among same-named candidates for
/// a deterministic answer (module-scoped disambiguation is out of scope —
/// same simplification `resolve_cell`'s doc-neighbor `external_declared`
/// makes for `calls`).
fn resolve_cell(index: &SymbolIndex, name: &str) -> Option<DefinitionId> {
    index
        .by_name
        .get(name)?
        .iter()
        .copied()
        .filter(|id| {
            index.symbols.get(id).is_some_and(|info| {
                matches!(info.kind, SymbolKind::Variable | SymbolKind::Constant)
            })
        })
        .min()
}

/// Whether `name` is a declared `EXTERNAL` anywhere in the project. `calls`
/// clauses match [`EffectRow::calls`] by raw name (T2-1 collects external
/// call atoms the same way), so no id resolution is needed — only
/// existence.
fn external_declared(index: &SymbolIndex, name: &str) -> bool {
    index.by_name.get(name).is_some_and(|ids| {
        ids.iter().any(|id| {
            index
                .symbols
                .get(id)
                .is_some_and(|info| info.kind == SymbolKind::External)
        })
    })
}

fn unknown_name_diagnostic(file: FileId, range: TextRange, name: &str) -> Diagnostic {
    Diagnostic {
        file,
        range,
        code: DiagnosticCode::E102,
        message: format!(
            "`#@effects` names `{name}`, which isn't a declared global VAR/CONST or EXTERNAL anywhere in the project"
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
                 unresolved callee) — no `#@effects` assertion can cover this definition"
            .to_string();
    }
    let name_of = |id: &DefinitionId| {
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
        "inferred effects exceed the `#@effects` assertion's declared bound: {}",
        parts.join("; ")
    )
}
