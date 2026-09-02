//! Issue #801 determinism enforcement: this crate's `clippy.toml` disallows
//! bare `std::collections::{HashMap, HashSet}` so a future `.values()` /
//! `.keys()` / `for x in a_hash_map` that leaks iteration order into
//! ordered output, a diagnostic, or a first-wins fold (the #795 regression
//! — see `modules.rs`'s module-attribution fix) fails the lint instead of
//! shipping.
//!
//! [`LookupSet`] is the crate's allow-list — every hashed-collection use in
//! this crate is one, and each has been individually audited to be
//! **provably order-free**: used only for `.contains()`/`.insert()`
//! membership, never iterated. Using the alias (rather than a bare
//! `#[expect(clippy::disallowed_types)]` at each call site) keeps the lint
//! live for every *new* use: a genuinely fresh `HashMap`/`HashSet` anywhere
//! else in the crate still trips it and forces the same audit. This crate
//! has no keyed-lookup `HashMap` use as of #801 (everything project-wide
//! goes through `brink-ir::SymbolIndex`'s own audited maps), so there's no
//! `LookupMap` alias here — add one, with the same audit discipline, if a
//! real use ever shows up.
//!
//! Do not reach for [`LookupSet`]/[`LookupMap`] to silence the lint on a new
//! site without doing that audit first — if the new use is ever iterated
//! for output, reach for `BTreeSet`/`BTreeMap` (already used pervasively
//! across this crate) or sort at the point of consumption instead.

/// A hashed set used only for membership tests (`.contains()`/`.insert()`
/// as a "have I seen this" guard) — never iterated. Current uses:
/// `dialect_gate::check`'s `resolved` set (already-resolved reference
/// sites, queried by `.contains()` only), `structs::check_literal_duplicates`'s
/// `seen` set (duplicate-field guard walked alongside the field `Vec`,
/// which is what actually orders the emitted diagnostics),
/// `admission`'s reference-range/name-bucket sets (B0.3, issue #1172 —
/// membership tests only, built once per file so a per-node manifest scan
/// stays O(n) instead of O(n^2)), and `temp_dominance`'s/
/// `compat_deny::knot_temp_from_stitch`'s (#3354/#3373) dominated-range and
/// discounted-write-range sets, plus the `ReadCollector` visitor both
/// passes share — every one queried by `.contains()` only, never iterated
/// (the diagnostics both passes emit are already ordered by the `Vec` of
/// reads/reads-per-stitch they walk, not by set order).
#[expect(
    clippy::disallowed_types,
    reason = "the crate's audited allow-list — see module doc"
)]
pub(crate) type LookupSet<T> = std::collections::HashSet<T>;

/// A hashed map used only for keyed lookups (`.get()`/`.insert()`/
/// `.contains_key()`) — never iterated (no `.values()`/`.keys()`/
/// `for (k, v) in`). Current uses: `admission::check_is_function_sentinel`
/// (B0.3, issue #1172) — a name→`DeclaredSymbol` index built once per file
/// so the per-knot sentinel check stays O(n) instead of the O(n^2) a
/// `Vec::iter().find()` per knot would give — and `temp_dominance`'s/
/// `compat_deny::knot_temp_from_stitch`'s (#3354/#3373) name→`DeclSite`
/// declaration maps, keyed lookups only.
#[expect(
    clippy::disallowed_types,
    reason = "the crate's audited allow-list — see module doc"
)]
pub(crate) type LookupMap<K, V> = std::collections::HashMap<K, V>;
