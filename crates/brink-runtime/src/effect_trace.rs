//! Ground-truth effect-atom recorder (issue #870, T2 effects epic,
//! `docs/effects-spec.md`). The effects analogue of the oracle: this module
//! records, per **executing definition scope**, the atomic effects the VM
//! actually performs — cells read, cells written, external kinds called —
//! so `brink-test-harness` can assert the statically-inferred `effects(def)`
//! row (`brink-db::ProjectDb::effects`) covers every one of them for every
//! def a real run executed. A purely structural inter-row consistency check
//! (caller's row ⊇ callee's row) cannot catch an under-report where *both*
//! rows silently agree on the wrong (too-small) answer — exactly the #866
//! ref-param-write regression this issue is named for. This is the
//! independent, run-the-bytecode-and-look check that closes that gap.
//!
//! **Attribution mirrors the static analyzer's own model exactly**
//! (`brink_analyzer::infer::body::record_ref_param_writes`), not naively
//! "whichever def's bytecode happens to be executing": a `ref` argument's
//! pointer/projection is constructed exactly once, at the call site, inside
//! the *caller's* own bytecode (`Opcode::PushVarPointer`/`Opcode::
//! MakeProjection` — both are emitted *only* there, never for a plain read,
//! confirmed against `brink-codegen-inkb`'s `expr.rs`). The eventual
//! dereference deep inside the callee's frame (`SetTemp`/`GetTemp`/
//! `TakeTemp`'s pointer/projection arms, `ProjRead`/`ProjWrite`) is
//! deliberately **not** re-recorded — the callee's own row is generic over
//! whichever concrete cell a caller bound its `ref` parameter to, so the
//! static model charges the write to the call site that names the concrete
//! global, never to the callee. Recording at construction time reproduces
//! that attribution for free: whichever def's bytecode is running when the
//! pointer/projection value is built is, by construction, the def the
//! static analyzer also charges. See the call sites in `vm.rs`'s
//! `note_effect_*` helpers for the exact opcodes instrumented.
//!
//! Feature-gated exactly like the `bench-counters` module (issue #821):
//! this module and every call site are compiled out entirely unless
//! `effect-trace` is enabled (not part of `default` — no released consumer
//! should ever turn it on), so an ordinary build pays exactly zero cost.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use std::sync::Mutex;

use brink_format::DefinitionId;

/// Atoms observed for one executed definition scope (`docs/effects-spec.md`
/// §2) — the runtime counterpart of `brink_analyzer::EffectRow`'s
/// `{reads, writes, calls}` (this module never constructs an opaque row:
/// every atom the VM performs is concrete).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObservedRow {
    pub reads: BTreeSet<DefinitionId>,
    pub writes: BTreeSet<DefinitionId>,
    pub calls: BTreeSet<String>,
    /// NS-A2 (issue #1108): the def emitted visible content (a line ref,
    /// value, glue, or spring on the visible output channel — string-eval
    /// captures excluded; see `vm::note_effect_emit`).
    pub emits: bool,
    /// NS-A2: the def produced a tag (any `EndTag` destination).
    pub tags: bool,
    /// NS-A2: a tracked turn-terminating fault fired while the def was
    /// executing (see [`is_tracked_fault`] for the inventory).
    pub faults: bool,
}

static OBSERVED: Mutex<BTreeMap<DefinitionId, ObservedRow>> = Mutex::new(BTreeMap::new());

/// Run `f` against the map, recovering from lock poisoning rather than
/// panicking (`unwrap`/`expect` on a `PoisonError` are denied outside tests
/// by workspace lint policy) — a panicking test elsewhere in the same
/// process must never wedge every subsequent recorder call.
fn with_map<R>(f: impl FnOnce(&mut BTreeMap<DefinitionId, ObservedRow>) -> R) -> R {
    let mut guard = OBSERVED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(&mut guard)
}

/// Record a cell read, attributed to `def` (the definition scope executing
/// when the read happened — see the module docs for what "attributed to"
/// means for a pointer/projection-mediated access).
pub fn record_read(def: DefinitionId, cell: DefinitionId) {
    with_map(|m| {
        m.entry(def).or_default().reads.insert(cell);
    });
}

/// Record a cell write, attributed to `def`.
pub fn record_write(def: DefinitionId, cell: DefinitionId) {
    with_map(|m| {
        m.entry(def).or_default().writes.insert(cell);
    });
}

/// Record an external-kind call, attributed to `def`.
pub fn record_call(def: DefinitionId, name: String) {
    with_map(|m| {
        m.entry(def).or_default().calls.insert(name);
    });
}

/// NS-A2 (issue #1108): record a visible content emission, attributed to
/// `def`.
pub fn record_emit(def: DefinitionId) {
    with_map(|m| {
        m.entry(def).or_default().emits = true;
    });
}

/// NS-A2: record a tag-channel touch, attributed to `def`.
pub fn record_tag(def: DefinitionId) {
    with_map(|m| {
        m.entry(def).or_default().tags = true;
    });
}

/// NS-A2: record a tracked turn-terminating fault, attributed to `def` (the
/// definition scope executing when `vm::step` returned the fault).
pub fn record_fault(def: DefinitionId) {
    with_map(|m| {
        m.entry(def).or_default().faults = true;
    });
}

/// NS-A2 (issue #1108, from #1097): is this error one of the **designed
/// domain faults** the `faults` row dimension tracks? The inventory mirrors
/// the static harvest in `brink-analyzer::infer::body` exactly — every
/// variant listed here must be raisable only by a construct that sets the
/// static `faults` bit (indexing, `/`/`mod`, the faulting stdlib
/// intrinsics, conversions, `ref` projections, value calls), or the
/// ground-truth harness would report a false under-report.
///
/// Deliberately NOT tracked (not part of the dimension v1):
/// - gradual-mode type errors (`TypeError`, `NotARecord`,
///   `RecordFieldNotFound`, …) — the strict-mode-eliminated species;
/// - infrastructure/malformed-bytecode errors (stack underflows, invalid
///   ids, decode errors, step/line limits, `RanOutOfContent`);
/// - host-surface errors (`ArgCountMismatch`, `UnknownPath`,
///   `PrivateAccess`, external-resolution errors).
#[must_use]
pub fn is_tracked_fault(e: &crate::RuntimeError) -> bool {
    use crate::RuntimeError as E;
    matches!(
        e,
        E::DivisionByZero
            | E::IndexOutOfBounds { .. }
            | E::MapKeyNotFound { .. }
            | E::NotIndexable(_)
            | E::InvalidArrayIndex(_)
            | E::InvalidMapKeyType(_)
            | E::ConversionParseFailure { .. }
            | E::InvalidConversionDomain { .. }
            | E::CharAtIndexNotInt(_)
            | E::CharAtOutOfBounds { .. }
            | E::StdlibWrongType { .. }
            | E::NotOrderable { .. }
            | E::ProjectionInvalidated(_)
            | E::NotCallable(_)
            | E::FunctionValueArity { .. }
            | E::FunctionValueCrossFlowLocal(_)
            | E::FunctionValueRehydrationMismatch(_)
    )
}

/// Clear every recorded atom. Call before each measured run — the recorder
/// is a single process-wide map, so a caller driving multiple programs (or
/// multiple explored episodes of one program) in the same process must
/// reset between the units it wants to compare independently.
pub fn reset() {
    with_map(BTreeMap::clear);
}

/// Snapshot every def's observed atoms recorded since the last [`reset`].
#[must_use]
pub fn snapshot() -> BTreeMap<DefinitionId, ObservedRow> {
    with_map(|m| m.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_format::DefinitionTag;

    fn def(n: u64) -> DefinitionId {
        DefinitionId::new(DefinitionTag::Address, n)
    }
    fn cell(n: u64) -> DefinitionId {
        DefinitionId::new(DefinitionTag::GlobalVar, n)
    }

    #[test]
    fn records_are_attributed_per_def_and_reset_clears_everything() {
        reset();
        record_read(def(1), cell(10));
        record_write(def(1), cell(11));
        record_call(def(1), "Play".to_string());
        record_write(def(2), cell(20));

        let snap = snapshot();
        assert_eq!(snap[&def(1)].reads, [cell(10)].into_iter().collect());
        assert_eq!(snap[&def(1)].writes, [cell(11)].into_iter().collect());
        assert_eq!(
            snap[&def(1)].calls,
            ["Play".to_string()].into_iter().collect()
        );
        assert_eq!(snap[&def(2)].writes, [cell(20)].into_iter().collect());

        reset();
        assert!(snapshot().is_empty(), "reset must clear every def's row");
    }
}
