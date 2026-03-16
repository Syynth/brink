# brink-driver specification

`brink-driver` is the pipeline orchestration layer for the brink compiler. It owns file discovery, cross-file analysis, diagnostic collection, and LIR input preparation. It sits between `brink-db` (stateful cache) and the product crates (`brink-compiler`, `brink-ide`) that consume its results.

See also: [brink-ide-spec](brink-ide-spec.md) (query layer that depends on brink-driver), [compiler-spec](compiler-spec.md) (compilation pipeline).

## Motivation

Three responsibilities are currently scattered across `brink-db`, `brink-compiler/src/driver.rs`, and `brink-lsp/src/backend.rs`:

1. **File discovery** — `ProjectDb::discover()` does BFS traversal of `INCLUDE` directives with a caller-provided `read_file` callback. This is pipeline orchestration, not cache behavior.

2. **Cross-file analysis** — `ProjectDb::analyze()` calls `brink_analyzer::analyze()` and caches the result. The decision of *when* to analyze and *what* to do with the result is orchestration, not storage.

3. **Diagnostic collection** — both the compiler (`driver.rs:24-77`) and the LSP (`backend.rs:2886-2971`) independently:
   - Collect per-file lowering diagnostics from `ProjectDb`
   - Collect cross-file analysis diagnostics
   - Apply suppression directives per file
   - Partition into errors vs warnings

   This is ~50 lines in the compiler, ~90 lines in the LSP, doing the same core work. The LSP adds multi-project annotation on top, but the gather-suppress-partition logic is identical.

4. **Project computation** — `ProjectDb::compute_projects()` discovers independent project groups from include relationships. This is a query over the include graph, not a storage operation.

5. **Include path resolution** — `resolve_include_path()` in `brink-db` uses `std::path::Path` for relative path resolution. Ink uses `/` as path separator universally (even on Windows, inklecate normalizes), so this should be string-based.

Extracting these into `brink-driver` gives:
- A single source of truth for diagnostic pipeline logic
- A cleaner `brink-db` that is purely a cache
- Platform-independent include path resolution (important for wasm)
- A clear layering: db stores data, driver orchestrates the pipeline, products consume results

## Architecture

### Before (current state)

```
brink-compiler/driver.rs ──→ ProjectDb::discover()
                              ProjectDb::analyze()
                              manual diagnostic collection ← DUPLICATED
                              manual LIR input assembly

brink-lsp/backend.rs ──────→ ProjectDb (via Arc<Mutex>)
                              brink_analyzer::analyze() directly
                              manual diagnostic collection ← DUPLICATED
                              ProjectDb::compute_projects()
```

### After (with brink-driver)

```
brink-compiler ──→ Driver::discover()
                   Driver::analyze()
                   Driver::collect_diagnostics()  ← SHARED
                   Driver::lir_inputs()

brink-lsp ──────→ Driver (wraps ProjectDb)
                  Driver::analyze_project()
                  Driver::collect_diagnostics()   ← SHARED
                  Driver::compute_projects()
```

### Dependency graph position

```
TIER 3: brink-db       → brink-syntax, brink-ir  (no brink-analyzer)
TIER 4: brink-driver   → brink-db, brink-analyzer, brink-ir
TIER 5: brink-compiler → brink-driver, brink-codegen-inkb, brink-codegen-json
         brink-ide     → brink-driver, brink-db, brink-syntax, brink-ir, brink-fmt
```

## What moves where

### Out of `brink-db`, into `brink-driver`

| Current location | What | Why it doesn't belong in the cache |
|------------------|------|-------------------------------------|
| `ProjectDb::discover()` | BFS file discovery via `read_file` callback | I/O + pipeline orchestration, not storage |
| `ProjectDb::analyze()` | Calls `brink_analyzer::analyze()`, caches result | Analysis orchestration, not storage |
| `ProjectDb::compute_projects()` | Groups files into independent projects by include relationships | Query over graph structure, not per-file caching |
| `ProjectDb::analysis_inputs()` | Snapshots `(FileId, HirFile, SymbolManifest)` tuples | Convenience for analysis callers, not storage |
| `ProjectDb::analysis_inputs_for()` | Same, filtered to a subset | Same |
| `ProjectDb::file_ids_topo()` | Topological sort of files by include order | Graph query, not storage |
| `ProjectDb::file_metadata()` | Snapshots `(FileId, path, source)` tuples | Convenience for callers |
| `resolve_include_path()` | Resolves relative include paths | Moves with discover(), gets string-based fix |
| `DiscoverError` | Error type for discovery | Moves with discover() |

### Out of `brink-compiler/driver.rs`, into `brink-driver`

| What | Lines | Description |
|------|-------|-------------|
| Diagnostic collection + suppression | 24-77 | Gather lowering + analysis diagnostics, apply suppressions, partition |
| LIR input assembly | 92-100 | Topo-sorted HIR files + file path map |

### Out of `brink-lsp/backend.rs`, into `brink-driver`

| What | Description |
|------|-------------|
| Diagnostic collection in `publish_all_diagnostics` | Same gather-suppress-partition logic as compiler (multi-project annotation stays in LSP) |
| `compute_projects()` call in `analysis_loop` | Project grouping |

### What stays in `brink-db`

After extraction, `ProjectDb` is a pure per-file cache:

- `set_file(path, source) → FileId` — parse, lower, cache
- `update_file(path, source) → FileId` — incremental re-parse with knot-level diffing
- `remove_file(path)` — remove from cache
- `file_id(path) → Option<FileId>` — path → id lookup
- `file_path(id) → Option<&str>` — id → path lookup
- `file_ids() → impl Iterator<Item = FileId>` — iterate all ids
- `parse(id) → Option<&Parse>` — cached parse tree
- `hir(id) → Option<&HirFile>` — cached HIR
- `manifest(id) → Option<&SymbolManifest>` — cached symbol manifest
- `source(id) → Option<&str>` — cached source text
- `file_diagnostics(id) → Option<&[Diagnostic]>` — per-file parse+lowering diagnostics
- `suppressions(id) → Option<&Suppressions>` — parsed suppression directives
- `rebuild_include_graph()` — re-scan all files for include edges (called after batch loading)
- Include graph data structure (forward/reverse edges, `update`, `remove`)

`brink-db` drops its dependency on `brink-analyzer`. It no longer imports or calls `brink_analyzer::analyze()`. The `AnalysisResult` re-export is removed.

### What stays in `brink-compiler`

- `compile()` / `compile_path()` / `compile_to_json()` public API
- `CompileError` type
- Final `emit()` call (LIR → `StoryData`)

### What stays in `brink-lsp`

- `Backend` struct, concurrency primitives, `LanguageServer` impl
- `analysis_loop` — still owns the debounce + background task, but calls `brink-driver` for diagnostic collection
- Multi-project diagnostic annotation (LSP-specific UX)
- Filesystem operations (`load_file_from_disk`, `walk_and_load`)

## API surface

### `Driver`

The primary public type. Wraps a `ProjectDb` and provides orchestration methods.

```rust
pub struct Driver {
    db: ProjectDb,
}

impl Driver {
    /// Create a new driver with an empty project database.
    pub fn new() -> Self;

    /// Create a driver wrapping an existing database.
    pub fn from_db(db: ProjectDb) -> Self;

    /// Borrow the underlying database (for cache queries).
    pub fn db(&self) -> &ProjectDb;

    /// Mutably borrow the underlying database (for set_file/update_file/remove_file).
    pub fn db_mut(&mut self) -> &mut ProjectDb;

    /// Consume the driver, returning the underlying database.
    pub fn into_db(self) -> ProjectDb;
}
```

### File discovery

```rust
impl Driver {
    /// Discover all files reachable from `entry` via INCLUDE directives.
    ///
    /// Calls `read_file` for the entry point and each discovered include.
    /// Files are parsed, lowered, and cached in the database.
    /// Detects circular includes.
    pub fn discover<F>(
        &mut self,
        entry: &str,
        read_file: F,
    ) -> Result<(), DiscoverError>
    where
        F: FnMut(&str) -> Result<String, std::io::Error>;
}

/// Errors from file discovery.
#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("circular INCLUDE: {0}")]
    CircularInclude(String),
}
```

### Analysis

```rust
impl Driver {
    /// Run cross-file analysis on all files in the database.
    /// Returns a reference to the cached result.
    pub fn analyze(&mut self) -> &AnalysisResult;

    /// Run cross-file analysis on a specific subset of files (one project).
    /// Does not cache the result (the caller owns it).
    pub fn analyze_project(&self, file_ids: &[FileId]) -> AnalysisResult;

    /// Snapshot analysis inputs for a subset of files.
    /// Returns owned (FileId, HirFile, SymbolManifest) tuples suitable for
    /// passing to brink_analyzer::analyze() outside a lock.
    pub fn analysis_inputs_for(
        &self,
        file_ids: &[FileId],
    ) -> Vec<(FileId, HirFile, SymbolManifest)>;

    /// Snapshot all analysis inputs.
    pub fn analysis_inputs(&self) -> Vec<(FileId, HirFile, SymbolManifest)>;
}
```

### Diagnostic collection

This is the core deduplication — the logic currently duplicated between the compiler and LSP.

```rust
impl Driver {
    /// Collect all diagnostics (lowering + analysis), apply suppressions,
    /// and partition into errors and warnings.
    ///
    /// `analysis` is the result of a prior `analyze()` or `analyze_project()` call.
    /// `entry` is the entry-point FileId; if its suppressions include `disable_all`,
    /// analysis diagnostics are skipped for all files in the project.
    pub fn collect_diagnostics(
        &self,
        analysis: &AnalysisResult,
        entry: Option<FileId>,
    ) -> DiagnosticReport;
}

/// Partitioned diagnostics after suppression filtering.
pub struct DiagnosticReport {
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
}
```

The implementation:
1. Iterates all files in the database
2. For each file, collects per-file lowering diagnostics via `db.file_diagnostics()`
3. Collects analysis diagnostics from `analysis.diagnostics` grouped by file
4. Applies `brink_ir::suppressions::apply_suppressions()` per file
5. Checks `disable_all` on the entry file
6. Partitions results by `code.severity()`

### Project computation

```rust
impl Driver {
    /// Discover independent projects from include relationships.
    ///
    /// A "project" is a root .ink file plus everything it transitively INCLUDEs.
    /// Roots are files not included by any other file.
    /// Returns (root, members) pairs sorted by root FileId.
    pub fn compute_projects(&self) -> Vec<(FileId, Vec<FileId>)>;
}
```

### LIR input preparation

```rust
impl Driver {
    /// Prepare inputs for LIR lowering: topo-sorted HIR files and file path map.
    ///
    /// Files are returned in topological include order (included files before
    /// the files that include them), matching ink's INCLUDE paste semantics.
    pub fn lir_inputs(
        &self,
        entry: FileId,
    ) -> (Vec<(FileId, &HirFile)>, HashMap<FileId, String>);

    /// Return file IDs in topological include order from the entry point.
    pub fn file_ids_topo(&self, entry: FileId) -> Vec<FileId>;
}
```

### File metadata

```rust
impl Driver {
    /// Snapshot file metadata for all files in the database.
    /// Returns (FileId, path, source) tuples.
    pub fn file_metadata(&self) -> Vec<(FileId, String, String)>;
}
```

### Include path resolution

```rust
/// Resolve an INCLUDE path relative to the including file's directory.
///
/// Uses string-based resolution (splits on '/') rather than std::path::Path,
/// since ink paths are always '/'-separated regardless of platform.
pub fn resolve_include_path(from_file: &str, include_path: &str) -> String;
```

Implementation:
```rust
pub fn resolve_include_path(from_file: &str, include_path: &str) -> String {
    match from_file.rfind('/') {
        Some(i) => format!("{}/{include_path}", &from_file[..i]),
        None => include_path.to_string(),
    }
}
```

This replaces the current `std::path::Path`-based version in `brink-db`. The `std::path` approach is subtly wrong on Windows (would use `\` separators) and creates a platform dependency that blocks clean wasm usage, even though `std::path` technically compiles for wasm.

## Changes to `brink-db`

### Removed from public API

| Method | Replacement |
|--------|-------------|
| `ProjectDb::discover()` | `Driver::discover()` |
| `ProjectDb::analyze()` | `Driver::analyze()` |
| `ProjectDb::compute_projects()` | `Driver::compute_projects()` |
| `ProjectDb::analysis_inputs()` | `Driver::analysis_inputs()` |
| `ProjectDb::analysis_inputs_for()` | `Driver::analysis_inputs_for()` |
| `ProjectDb::file_ids_topo()` | `Driver::file_ids_topo()` |
| `ProjectDb::file_metadata()` | `Driver::file_metadata()` |

### Removed dependency

`brink-db` drops `brink-analyzer` from its `[dependencies]`. The `pub use brink_analyzer::AnalysisResult` re-export is removed. Callers import `AnalysisResult` from `brink_analyzer` directly (or from `brink-driver` if re-exported).

### Internal changes

The `IncludeGraph` remains in `brink-db` as a `pub(crate)` data structure — it's part of the per-file state that `set_file`/`update_file`/`remove_file` maintain. However, the *query* methods on the graph (`topological_order`, `compute_projects`, `roots`, `find_cycle`) become accessible via `brink-driver`, which calls through `ProjectDb` accessors:

```rust
// New public methods on ProjectDb (read-only graph access)
impl ProjectDb {
    /// File IDs in topological include order from entry.
    pub fn file_ids_topo(&self, entry: FileId) -> Vec<FileId>;

    /// Compute independent projects from include relationships.
    pub fn compute_projects(&self) -> Vec<(FileId, Vec<FileId>)>;

    /// Detect circular includes. Returns the cycle path if found.
    pub fn find_cycle(&self) -> Option<Vec<FileId>>;
}
```

These are thin wrappers that delegate to `IncludeGraph` methods. The graph data stays in brink-db because `set_file`/`update_file`/`remove_file` need to maintain it incrementally.

**Rationale:** Moving the entire `IncludeGraph` to brink-driver would require either duplicating graph state or passing the graph back and forth between crates on every file mutation. Keeping the data in brink-db and exposing read-only queries is cleaner.

## Changes to `brink-compiler`

The compiler driver simplifies significantly:

### Before (~135 lines in driver.rs)

```rust
fn compile_lir<F>(entry: &str, mut read_file: F) -> Result<LirOutput, CompileError> {
    let mut db = ProjectDb::new();
    db.discover(entry, &mut read_file)?;

    // ~25 lines: manual lowering diagnostic collection
    // ~20 lines: manual analysis diagnostic collection with suppress logic
    // ~10 lines: manual LIR input assembly

    let (program, lir_warnings) = lower_to_program(&files, ...);
    Ok(LirOutput { program, warnings })
}
```

### After (~30 lines)

```rust
fn compile_lir<F>(entry: &str, read_file: F) -> Result<LirOutput, CompileError> {
    let mut driver = Driver::new();
    driver.discover(entry, read_file)?;

    let analysis = driver.analyze().clone();
    let entry_id = driver.db().file_id(entry).ok_or_else(|| ...)?;

    let report = driver.collect_diagnostics(&analysis, Some(entry_id));
    if !report.errors.is_empty() {
        let mut all = report.errors;
        all.extend(report.warnings);
        return Err(CompileError::Diagnostics(all));
    }

    let (files, file_paths) = driver.lir_inputs(entry_id);
    let (program, lir_warnings) = lower_to_program(&files, &analysis.index, ...);

    let mut warnings = report.warnings;
    warnings.extend(lir_warnings);

    Ok(LirOutput { program, warnings })
}
```

### Dependency changes

`brink-compiler`'s `Cargo.toml`:
- Add: `brink-driver`
- Remove: `brink-db`, `brink-analyzer` (accessed through brink-driver)
- Keep: `brink-ir`, `brink-codegen-inkb`, `brink-codegen-json`, `brink-format`, `brink-json`

## Changes to `brink-lsp`

### `analysis_loop`

Currently reimplements diagnostic collection. After migration:

```rust
pub async fn analysis_loop(...) {
    loop {
        trigger.notified().await;
        tokio::task::yield_now().await;

        // Snapshot under lock
        let (project_defs, file_meta, file_suppressions) = {
            let db = lock_db(&db);
            let driver = Driver::from_db_ref(&db);  // borrows, no clone
            let projects = driver.compute_projects();
            let meta = driver.file_metadata();
            // ... snapshot suppressions
            (projects, meta, suppressions)
        };

        // Analyze each project outside the lock
        for (root, members) in &project_defs {
            let inputs = /* snapshotted earlier */;
            let analysis = brink_analyzer::analyze(&inputs);
            let report = Driver::collect_diagnostics_from(
                &analysis, /* file sources + suppressions */
            );
            // Convert to LSP diagnostics, add multi-project annotations
        }

        // Publish
    }
}
```

The LSP still owns the debounce/background task and multi-project annotation. It just calls brink-driver for the gather-suppress-partition step.

### Dependency changes

`brink-lsp`'s `Cargo.toml`:
- Add: `brink-driver` (after brink-ide is created, this becomes `brink-ide` instead)
- Keep: `brink-db` (still needs direct access for `set_file`/`update_file`/`remove_file` under lock)
- Keep: `brink-analyzer` (for `analyze()` calls in the analysis loop — the LSP runs analysis outside the lock on snapshotted inputs)

## Wasm compatibility

`brink-driver` MUST compile for `wasm32-unknown-unknown`:

- `discover()` takes a `read_file` callback — no filesystem access
- `resolve_include_path()` is string-based — no `std::path`
- No async, no threads, no platform-specific code
- Dependencies: `brink-db`, `brink-analyzer`, `brink-ir`, `thiserror`, `tracing` — all wasm-compatible

## Determinism

Same rules as the rest of the project:

- Never iterate `HashMap` where order affects output. `collect_diagnostics` groups diagnostics by `FileId` — iteration over this map must be sorted.
- `file_ids_topo` and `compute_projects` already sort their output by `FileId`.
- `DiagnosticReport` vectors preserve insertion order (sorted by file, then by range within file).

## Crate metadata

```toml
[package]
name = "brink-driver"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
readme.workspace = true
keywords.workspace = true
categories.workspace = true
description = "Pipeline orchestration for the brink ink compiler"
publish = false

[dependencies]
brink-db = { path = "../brink-db" }
brink-analyzer = { path = "../brink-analyzer" }
brink-ir = { path = "../brink-ir" }
thiserror.workspace = true
tracing.workspace = true

[lints]
workspace = true
```

## Migration plan

### Phase 1: Create brink-driver with Driver struct

1. Create `crates/internal/brink-driver/`
2. Add to workspace `Cargo.toml`
3. Implement `Driver` struct wrapping `ProjectDb`
4. Implement `resolve_include_path` (string-based)
5. Move `discover()` logic from `ProjectDb` — calls `db.set_file()` internally
6. Move `analyze()` — calls `brink_analyzer::analyze()` on db inputs
7. Move `analysis_inputs()` / `analysis_inputs_for()` / `file_metadata()`
8. Move `file_ids_topo()` (delegates to db's include graph)
9. Move `compute_projects()` (delegates to db's include graph)
10. Implement `collect_diagnostics()` — consolidation of compiler + LSP logic
11. Implement `lir_inputs()`

All `ProjectDb` methods still exist in this phase (marked deprecated or kept as-is). brink-driver delegates to them. No existing callers break.

### Phase 2: Migrate brink-compiler

1. Change `brink-compiler` to depend on `brink-driver` instead of calling `ProjectDb` directly
2. Rewrite `driver.rs` to use `Driver`
3. Remove `brink-db` and `brink-analyzer` from `brink-compiler`'s dependencies
4. Verify: `cargo test -p brink-compiler`, episode corpus unchanged

### Phase 3: Migrate brink-lsp

1. Add `brink-driver` to `brink-lsp` dependencies
2. Refactor `analysis_loop` to use `Driver::collect_diagnostics()` for the gather-suppress-partition step
3. Multi-project annotation stays in the LSP
4. Verify: LSP still works with all features

### Phase 4: Clean up brink-db

1. Remove deprecated methods from `ProjectDb` (`discover`, `analyze`, `compute_projects`, `analysis_inputs`, etc.)
2. Remove `brink-analyzer` dependency from `brink-db`
3. Remove `pub use brink_analyzer::AnalysisResult` re-export
4. Add `pub fn file_ids_topo()`, `pub fn compute_projects()`, `pub fn find_cycle()` as thin graph query wrappers
5. Verify: full workspace builds, all tests pass

### Phase 5: Fix resolve_include_path

1. Replace `std::path::Path`-based resolution in brink-db's `set_file`/`update_file`/`rebuild_include_graph` with calls to `brink_driver::resolve_include_path` (or inline the string-based version)
2. Remove `use std::path::Path` from brink-db
3. Verify: episode corpus unchanged, wasm builds clean

Each phase is independently committable and testable. The system works correctly at every intermediate step.
