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
//! Do not reach for [`LookupSet`] to silence the lint on a new site without
//! doing that audit first — if the new use is ever iterated for output,
//! reach for `BTreeSet` (already used pervasively across this crate) or
//! sort at the point of consumption instead.

/// A hashed set used only for membership tests (`.contains()`/`.insert()`
/// as a "have I seen this" guard) — never iterated. Current uses:
/// `dialect_gate::check`'s `resolved` set (already-resolved reference
/// sites, queried by `.contains()` only) and `structs::check_literal_duplicates`'s
/// `seen` set (duplicate-field guard walked alongside the field `Vec`,
/// which is what actually orders the emitted diagnostics).
#[expect(
    clippy::disallowed_types,
    reason = "the crate's audited allow-list — see module doc"
)]
pub(crate) type LookupSet<T> = std::collections::HashSet<T>;
