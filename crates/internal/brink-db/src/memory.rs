//! Salsa memo-table memory introspection (issue #529).
//!
//! Behind the `memory-introspection` Cargo feature — an explicit opt-in so
//! ordinary consumers (`brink-lsp`, `brink-ide`, `brink-web`/wasm) never pull
//! in salsa's `salsa_unstable` surface; only the editor-session memory
//! profiling harness (`brink-test-harness/src/bin/editor_session_bench.rs`,
//! #529) enables it. Its public surface is plain data — this module (like
//! [`crate::db`]) is one of the only places in this crate allowed to see a
//! salsa type; nothing salsa-shaped escapes it.
//!
//! `count` is the number of live memo entries for a query, or live instances
//! for a struct/input table. It is the primary growth signal this module
//! exists to surface: salsa's own LRU capacity (the scripting-substrate
//! spec's §8 follow-up, deliberately out of scope here) trims memo tables by
//! *count*, not by byte size. `metadata_bytes`/`fields_bytes`/`heap_bytes`
//! are real numbers salsa tracks regardless, but for any query whose output
//! is `Arc<T>`-wrapped (most of layer 2/3 — `symbol_index_query`,
//! `resolve_query`, `lir_query`, `story_data_query`, …) `fields_bytes` is
//! only the pointer's size: salsa cannot see inside an `Arc` without an
//! explicit `heap_size` estimator, and none of this crate's queries specify
//! one (a deliberate scope boundary — a wrong or stale hand-rolled estimator
//! would silently under-report and mislead the next reader more than an
//! honest `None`/pointer-sized number does). `heap_bytes` is therefore
//! always `None` today; wiring up `heap_size` for specific queries is a
//! natural follow-up once this pass's `count` data identifies which ones
//! are worth it.

use crate::queries::BrinkDatabase;

/// Whether an [`IngredientMemory`] row describes a salsa input/tracked
/// struct table, or a memoized query function's output table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IngredientKind {
    /// A `#[salsa::input]`/`#[salsa::tracked]` struct or `#[salsa::interned]`
    /// key table (e.g. `SourceFile`, `ProjectInput`, `DefKey`).
    Struct,
    /// A memoized `#[salsa::tracked]` query function's output table (e.g.
    /// `parse_query`, `lir_query`).
    Query,
}

/// One row of a [`crate::ProjectDb::memory_snapshot`]: a single salsa
/// ingredient and its current footprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngredientMemory {
    /// Ingredient/query debug name. `Query` rows use the query function's
    /// own name (`"parse_query"`, matching this crate's source); `Struct`
    /// rows use the salsa struct's type name (`"SourceFile"`).
    pub name: String,
    /// Struct/input table, or memoized query output table.
    pub kind: IngredientKind,
    /// Number of live instances (structs) or memoized results (queries).
    pub count: usize,
    /// Total salsa bookkeeping bytes (memo/slot metadata) across all rows.
    pub metadata_bytes: usize,
    /// Total stack-resident bytes of the output/field values across all
    /// rows — see the module docs for why this undercounts `Arc<T>` outputs.
    pub fields_bytes: usize,
    /// Total heap bytes the fields own, only if the ingredient specifies a
    /// `heap_size` estimator. No query in this crate does yet — always
    /// `None` (see module docs).
    pub heap_bytes: Option<usize>,
}

/// Snapshot every salsa ingredient's memory footprint, sorted by
/// `(kind, name)` for deterministic, greppable output — a query or struct
/// that has never been invoked/created simply has no row (salsa's
/// `memory_usage` only reports what actually has memo/instance data), so the
/// row set itself can grow across a session as more of the query graph gets
/// exercised for the first time.
pub(crate) fn snapshot(db: &BrinkDatabase) -> Vec<IngredientMemory> {
    let info = <dyn salsa::Database>::memory_usage(db);

    let mut rows: Vec<IngredientMemory> = info
        .structs
        .iter()
        .map(|i| IngredientMemory {
            name: i.debug_name().to_string(),
            kind: IngredientKind::Struct,
            count: i.count(),
            metadata_bytes: i.size_of_metadata(),
            fields_bytes: i.size_of_fields(),
            heap_bytes: i.heap_size_of_fields(),
        })
        .collect();

    rows.extend(info.queries.iter().map(|(name, i)| IngredientMemory {
        name: (*name).to_string(),
        kind: IngredientKind::Query,
        count: i.count(),
        metadata_bytes: i.size_of_metadata(),
        fields_bytes: i.size_of_fields(),
        heap_bytes: i.heap_size_of_fields(),
    }));

    rows.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
    rows
}
