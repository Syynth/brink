//! Bench-only debug counters for Arc-clone / COW-copy events (issue #821
//! Workstream B seed, `docs/runtime-bench.md`).
//!
//! The value model's performance claims (`docs/value-model-spec.md` §5/§6)
//! rest on two mechanisms: sharing a collection is an O(1) `Arc::clone`, and
//! mutating a shared collection pays exactly one O(n) copy (via
//! `Arc::make_mut` inside `Value::array_make_mut`/`map_make_mut`/
//! `record_make_mut`) before becoming unique again. Wall-clock benchmarks
//! can only *infer* whether these mechanisms actually fired; these counters
//! measure it directly.
//!
//! This entire module exists only when the `bench-counters` feature is
//! enabled — it is not part of the `default` feature set, so `cargo build
//! -p brink-runtime` (no extra flags) never compiles it in: there is no
//! `bench_counters` module, no atomics, no call-site branches — a
//! compile-time cut, not a runtime toggle. The call sites in
//! `collection_ops.rs`/`record_ops.rs`/`vm.rs` that report into this module
//! do so through tiny `note_*` wrapper functions that are themselves
//! `#[cfg]`-gated to a no-op empty body when the feature is off, so the
//! wrapper call inlines away to nothing (verified by the gate: `cargo build
//! -p brink-runtime`/`cargo clippy` with no `bench-counters` feature builds
//! clean with the module physically absent).

use core::sync::atomic::{AtomicU64, Ordering};

static COW_COPIES: AtomicU64 = AtomicU64::new(0);
static ARC_CLONES: AtomicU64 = AtomicU64::new(0);

/// A point-in-time read of the counters. Cheap `Copy` value so benches can
/// snapshot before/after a measured section and diff.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BenchCounters {
    /// Number of times `array_make_mut`/`map_make_mut`/`record_make_mut`
    /// found a shared `Arc` (`strong_count > 1`) and paid the one-time O(n)
    /// copy — the "mutate-while-shared" cost value-model-spec §5 argues is
    /// bounded per share, not unbounded.
    pub cow_copies: u64,
    /// Number of times a collection-typed `Value` (`Array`/`Map`/`Record`)
    /// was duplicated via a cheap `Arc::clone` (an O(1) snapshot/read) —
    /// the "sharing is O(1)" half of the same claim.
    pub arc_clones: u64,
}

/// Record one COW copy event.
pub fn record_cow_copy() {
    COW_COPIES.fetch_add(1, Ordering::Relaxed);
}

/// Record one Arc-clone (cheap share) event.
pub fn record_arc_clone() {
    ARC_CLONES.fetch_add(1, Ordering::Relaxed);
}

/// Read the current counter values without resetting them.
pub fn snapshot() -> BenchCounters {
    BenchCounters {
        cow_copies: COW_COPIES.load(Ordering::Relaxed),
        arc_clones: ARC_CLONES.load(Ordering::Relaxed),
    }
}

/// Zero both counters. Benches call this before the measured section so
/// setup work (compiling, linking, building initial fixtures) doesn't
/// pollute the count.
pub fn reset() {
    COW_COPIES.store(0, Ordering::Relaxed);
    ARC_CLONES.store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::{record_arc_clone, record_cow_copy, reset, snapshot};

    /// Counters start at zero after `reset`, and each `record_*` call
    /// increments only its own field — proves the two counters are
    /// independent, not aliased to the same atomic.
    #[test]
    fn counters_are_independent_and_resettable() {
        reset();
        record_cow_copy();
        record_cow_copy();
        record_arc_clone();
        let snap = snapshot();
        assert_eq!(snap.cow_copies, 2);
        assert_eq!(snap.arc_clones, 1);

        reset();
        let snap = snapshot();
        assert_eq!(snap.cow_copies, 0);
        assert_eq!(snap.arc_clones, 0);
    }
}
