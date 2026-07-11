//! The persistent map type used throughout the runtime.
//!
//! Under the default `std` feature this is `std::collections::HashMap` —
//! bit-for-bit the same type the runtime always used, so builds with `std`
//! enabled (every existing consumer, all tests, the oracle gate) see zero
//! behavior change.
//!
//! Without `std` it falls back to `alloc::collections::BTreeMap` (`core` +
//! `alloc` only, no external crate), which also happens to give
//! deterministic iteration order — see the project's "determinism matters"
//! rule. All keys used as map keys in this crate are already `Ord` (they're
//! sorted elsewhere for the same reason), so the fallback is a drop-in.
//!
//! `BTreeMap` has no capacity concept, so [`map_with_capacity`] is a no-op
//! under `no_std`; under `std` it still calls `HashMap::with_capacity`,
//! preserving the existing allocation behavior exactly.

#[cfg(feature = "std")]
pub(crate) use std::collections::HashMap as Map;

#[cfg(not(feature = "std"))]
pub(crate) use alloc::collections::BTreeMap as Map;

#[cfg(feature = "std")]
pub(crate) fn map_with_capacity<K, V>(capacity: usize) -> Map<K, V> {
    Map::with_capacity(capacity)
}

#[cfg(not(feature = "std"))]
pub(crate) fn map_with_capacity<K, V>(_capacity: usize) -> Map<K, V> {
    Map::new()
}
