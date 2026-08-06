//! Issue #801 determinism enforcement: this crate's `clippy.toml` disallows
//! bare `std::collections::{HashMap, HashSet}` so a future `.values()` /
//! `.keys()` / `for x in a_hash_map` that leaks per-process iteration order
//! into codegen output, a diagnostic, or a first-wins fold fails the lint
//! instead of shipping quietly (the #795 regression class — nondeterministic
//! `brink-analyzer` module attribution from exactly this pattern).
//!
//! The two aliases below are the crate's allow-list. Every existing
//! `std`-hashed-collection use in this crate is one of these, and each has
//! been individually audited: either the collection is queried only by
//! `.get()`/`.contains()`/keyed insertion (order never observed), or every
//! place it's iterated for output already sorts first (see `lir::lower::mod`'s
//! `private_defs`/`aliases` construction, and `lir::lower::structs`'
//! `struct_shape_defs`, which places each entry at its own fixed `ShapeId`
//! index rather than trusting iteration order). Routing an audited site
//! through the alias — instead of a bare per-site
//! `#[expect(clippy::disallowed_types)]` — keeps the lint live for every
//! *new* use: a fresh `HashMap`/`HashSet` anywhere else in the crate still
//! trips it and forces the same audit before it can iterate for output.
//!
//! Do not reach for these aliases on a new site without doing that audit
//! first — if the new use is ever iterated for output, reach for
//! `BTreeMap`/`BTreeSet` (this crate's default, e.g. `SymbolIndex`'s sibling
//! `aliases: Vec<AliasEntry>` sort, `infer::graph`'s `BTreeMap`/`BTreeSet`
//! dependency graph) or sort at the point of consumption instead.

/// A hashed map used only for keyed lookup/insertion, or iterated only
/// where the iteration order provably cannot reach output (documented at
/// each use site). Current uses: `SymbolIndex::{symbols, by_name}`
/// (project-wide symbol table — see `modules.rs` in `brink-analyzer` for
/// the one iteration site's order-independence proof), the LIR lowering
/// context's per-file/per-name/per-slot maps (`context.rs`), `decls.rs`'s
/// `const_values` (keyed const-eval memo, `.get()`-only), and
/// `structs.rs`'s shape tables (`field_index`, `by_def`, `by_name`,
/// `GlobalShapeMap` — all keyed lookup; `struct_shape_defs` iterates
/// `by_def.values()` but only to place each entry at its own fixed
/// `ShapeId` index, independent of iteration order).
#[expect(
    clippy::disallowed_types,
    reason = "the crate's audited allow-list — see module doc"
)]
pub(crate) type LookupMap<K, V> = std::collections::HashMap<K, V>;

/// A hashed set used only for membership tests. Current uses:
/// `lir::lower::mod`'s `block_scoped_temp_names` (declared-name guard,
/// `.contains()`/`.insert()` only) and `structs.rs`'s `build_struct_shape_data`
/// `seen` set (identity-scoped duplicate-declaration guard, same shape).
#[expect(
    clippy::disallowed_types,
    reason = "the crate's audited allow-list — see module doc"
)]
pub(crate) type LookupSet<T> = std::collections::HashSet<T>;
