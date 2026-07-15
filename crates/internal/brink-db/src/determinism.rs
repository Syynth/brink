//! Issue #801 determinism enforcement: this crate's `clippy.toml` disallows
//! bare `std::collections::{HashMap, HashSet}` so a future `.values()` /
//! `.keys()` / `for x in a_hash_map` that leaks per-process iteration order
//! into a diagnostic or a cycle-detection result fails the lint instead of
//! shipping quietly. `IncludeGraph::find_cycle` was exactly this class of
//! bug (issue #801 fix): it picked its DFS start node from
//! `self.forward.keys()` — a `HashMap` — directly, so which rotation of a
//! multi-file `INCLUDE` cycle it reported (and therefore the exact text of
//! `DiscoverError::CircularInclude`, which reaches the user byte-for-byte)
//! depended on that map's per-instance `RandomState` seed.
//!
//! The two aliases below are the crate's allow-list. Every existing
//! `std`-hashed-collection use in this crate is one of these, and each has
//! been individually audited: either the collection is queried only by
//! `.get()`/`.contains()`/keyed insertion (order never observed downstream),
//! or every place it's iterated for output already sorts first (see
//! `include_graph.rs`'s `find_cycle`, `roots`, `compute_projects`, both of
//! which sort their candidate lists by `FileId.0` before use — and
//! `queries::mod`'s `partition_diagnostics`, which sorts by `FileId` before
//! re-emitting). `topological_order`'s DFS never iterates a hashed
//! collection for output at all (issue #815): its `visited` set is queried
//! only by `.insert()`'s membership test, and the emitted order comes solely
//! from `IncludeGraph::includes`' `Vec<FileId>` (insertion-ordered from each
//! file's own `INCLUDE` statements), not from map iteration. Routing an audited site
//! through the alias — instead of a bare per-site
//! `#[expect(clippy::disallowed_types)]` — keeps the lint live for every
//! *new* use: a fresh `HashMap`/`HashSet` anywhere else in the crate still
//! trips it and forces the same audit before it can iterate for output.
//!
//! Do not reach for these aliases on a new site without doing that audit
//! first — if the new use is ever iterated for output, reach for
//! `BTreeMap`/`BTreeSet` (this crate's default elsewhere, e.g.
//! `infer::graph`'s dependency graph) or sort at the point of consumption
//! instead, the way every existing site in this crate already does.

/// A hashed map used only for keyed lookup/insertion, or iterated only
/// where the iteration order provably cannot reach output (documented at
/// each use site). Current uses: `ProjectDb`'s `files`/`path_to_id`/
/// `id_to_path`/`retired` (salsa-input-keyed lookup tables), and
/// `IncludeGraph`'s `forward`/`reverse` adjacency maps (`.get()`-only
/// except `find_cycle`'s DFS-start enumeration, which sorts before use).
#[expect(
    clippy::disallowed_types,
    reason = "the crate's audited allow-list — see module doc"
)]
pub(crate) type LookupMap<K, V> = std::collections::HashMap<K, V>;

/// A hashed set used only for membership tests, or as scratch state in a
/// graph walk whose final result is separately sorted before being
/// returned (documented at each use site) — never itself the source of
/// output order. Current uses: `IncludeGraph`'s DFS `visited`/`on_stack`/
/// `claimed` walk state (`find_cycle`, `topological_order`,
/// `compute_projects`, `roots`) and `queries::mod`'s `live_ids`/
/// `member_files` membership filters.
#[expect(
    clippy::disallowed_types,
    reason = "the crate's audited allow-list — see module doc"
)]
pub(crate) type LookupSet<T> = std::collections::HashSet<T>;
