# Implementation plan: brink-driver + brink-ide

Consolidated plan for introducing `brink-driver` and `brink-ide`. Each step is independently committable and testable. The system works correctly at every intermediate state.

Specs: [brink-driver-spec](brink-driver-spec.md), [brink-ide-spec](brink-ide-spec.md), [compiler-spec](compiler-spec.md).

## Verification commands

```sh
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --exclude brink-lsp -- -D warnings
cargo test -p brink-test-harness --test brink_native_episodes   # ratchet unchanged
cargo check --target wasm32-unknown-unknown -p brink-driver      # wasm compat
cargo check --target wasm32-unknown-unknown -p brink-ide         # wasm compat
```

---

## Phase 1: Create brink-driver

Reference: [brink-driver-spec § API surface](brink-driver-spec.md#api-surface)

### Step 1.1: Scaffold crate
- [ ] Create `crates/internal/brink-driver/` with Cargo.toml (deps: brink-db, brink-analyzer, brink-ir, thiserror, tracing)
- [ ] Add to workspace Cargo.toml
- [ ] Create `src/lib.rs` with `Driver` struct wrapping `ProjectDb`, `DiscoverError` type
- [ ] Verify: `cargo check -p brink-driver`

### Step 1.2: Move discover
- [ ] Implement `Driver::discover()` — move BFS logic from `ProjectDb::discover()`
- [ ] Implement `resolve_include_path()` as string-based (`rfind('/')`) — replaces `std::path::Path` version
- [ ] Add tests for `resolve_include_path` (basic, nested, no-directory, trailing-slash edge cases)
- [ ] Verify: `cargo test -p brink-driver`

### Step 1.3: Move analysis orchestration
- [ ] Implement `Driver::analyze()` — calls `brink_analyzer::analyze()` on db inputs, caches result
- [ ] Implement `Driver::analyze_project()` — analysis for a subset of files (no cache)
- [ ] Implement `Driver::analysis_inputs()` / `Driver::analysis_inputs_for()`
- [ ] Verify: `cargo test -p brink-driver`

### Step 1.4: Move project computation + graph queries
- [ ] Implement `Driver::compute_projects()` — delegates to db's include graph
- [ ] Implement `Driver::file_ids_topo()` — delegates to db's include graph
- [ ] Implement `Driver::file_metadata()`
- [ ] Add thin public wrappers on `ProjectDb` for graph queries (`file_ids_topo`, `compute_projects`, `find_cycle`) if not already public
- [ ] Verify: `cargo test -p brink-driver`

### Step 1.5: Implement diagnostic collection
- [ ] Implement `Driver::collect_diagnostics()` — consolidation of compiler + LSP logic
- [ ] Implement `DiagnosticReport` type
- [ ] Logic: gather lowering diagnostics per file + analysis diagnostics, apply suppressions, partition by severity, handle `disable_all` on entry file
- [ ] Add tests: basic partitioning, suppression filtering, disable_all behavior
- [ ] Verify: `cargo test -p brink-driver`

### Step 1.6: Implement LIR input preparation
- [ ] Implement `Driver::lir_inputs()` — topo-sorted HIR files + file path map
- [ ] Verify: `cargo test -p brink-driver`, full workspace check

---

## Phase 2: Migrate brink-compiler to brink-driver

Reference: [brink-driver-spec § Changes to brink-compiler](brink-driver-spec.md#changes-to-brink-compiler)

### Step 2.1: Rewrite compiler driver
- [ ] Change `brink-compiler` to depend on `brink-driver`
- [ ] Rewrite `driver.rs` to use `Driver` (discover → analyze → collect_diagnostics → lir_inputs → lower → emit)
- [ ] Remove direct `brink-db` and `brink-analyzer` from `brink-compiler` Cargo.toml
- [ ] Verify: `cargo test -p brink-compiler`, episode corpus ratchet unchanged

---

## Phase 3: Migrate brink-lsp to brink-driver

Reference: [brink-driver-spec § Changes to brink-lsp](brink-driver-spec.md#changes-to-brink-lsp)

### Step 3.1: Use driver for diagnostic collection
- [ ] Add `brink-driver` to `brink-lsp` dependencies
- [ ] Refactor `analysis_loop` to use `Driver::collect_diagnostics()` for gather-suppress-partition
- [ ] Multi-project annotation stays in LSP
- [ ] Verify: LSP tests pass, manual smoke test if feasible

---

## Phase 4: Clean up brink-db

Reference: [brink-driver-spec § Changes to brink-db](brink-driver-spec.md#changes-to-brink-db)

### Step 4.1: Remove migrated methods
- [ ] Remove `ProjectDb::discover()` (callers now use `Driver::discover()`)
- [ ] Remove `ProjectDb::analyze()` (callers now use `Driver::analyze()`)
- [ ] Remove `ProjectDb::analysis_inputs()` / `analysis_inputs_for()` / `file_metadata()`
- [ ] Remove `pub use brink_analyzer::AnalysisResult` re-export from `brink-db/src/lib.rs`
- [ ] Remove `brink-analyzer` from `brink-db` Cargo.toml dependencies
- [ ] Remove `DiscoverError` from brink-db (now in brink-driver)
- [ ] Verify: full workspace builds, all tests pass

### Step 4.2: Fix resolve_include_path in brink-db internals
- [ ] Replace `std::path::Path`-based `resolve_include_path` in `db.rs` (used by `set_file`/`update_file`/`rebuild_include_graph`) with the string-based version from brink-driver (or import it)
- [ ] Remove `use std::path::Path` from brink-db
- [ ] Verify: episode corpus unchanged, `cargo check --target wasm32-unknown-unknown -p brink-db`

---

## Phase 5: Create brink-ide

Reference: [brink-ide-spec § API surface](brink-ide-spec.md#api-surface), [§ Domain types](brink-ide-spec.md#domain-types)

### Step 5.1: Scaffold crate
- [ ] Create `crates/internal/brink-ide/` with Cargo.toml (deps: brink-syntax, brink-ir, brink-analyzer, brink-fmt, bitflags, rowan)
- [ ] Add to workspace Cargo.toml
- [ ] Create `src/lib.rs`
- [ ] Verify: `cargo check -p brink-ide`, `cargo check --target wasm32-unknown-unknown -p brink-ide`

### Step 5.2: Move LineIndex
- [ ] Move `LineIndex` from `brink-lsp/src/convert.rs` to `brink-ide`
- [ ] Move `to_lsp_range` / `to_text_size` helpers — keep LSP-specific wrappers in brink-lsp that delegate
- [ ] Update brink-lsp to import from brink-ide
- [ ] Move LineIndex tests
- [ ] Verify: `cargo test -p brink-ide`, `cargo test -p brink-lsp`

### Step 5.3: Move text utilities
- [ ] Define domain types: `TextEdit` (with `TextRange`)
- [ ] Move `word_at_offset`, `word_range_at_offset`, `builtin_hover_text`
- [ ] Move `diff_to_edits` (return `TextEdit` with `TextRange`, not LSP `Range`)
- [ ] Move `find_call_context` (signature help helper)
- [ ] Move `detect_completion_context`, `cursor_scope`, `is_visible_in_context`
- [ ] Move `find_def_at_offset` core logic
- [ ] Update brink-lsp to import from brink-ide
- [ ] Add tests for moved functions
- [ ] Verify: `cargo test -p brink-ide`, `cargo test -p brink-lsp`

---

## Phase 6: Migrate query functions

Reference: [brink-ide-spec § Migration plan, Phase 3](brink-ide-spec.md#phase-3-migrate-query-functions-one-at-a-time)

Each migration follows the same pattern:
1. Define domain result type in brink-ide
2. Extract core logic from LSP handler into brink-ide function
3. Update LSP handler to: call brink-ide → convert domain result to LSP type
4. Add unit tests in brink-ide
5. Commit

### Step 6.1: Formatting
- [ ] `format_document`, `format_region`
- [ ] `sort_knots`, `sort_stitches`

### Step 6.2: Document structure
- [ ] `document_symbols` (domain `DocumentSymbol` type)
- [ ] `folding_ranges` (domain `FoldRange` type)

### Step 6.3: Semantic tokens
- [ ] `TokenType` enum, `TokenModifiers` bitflags
- [ ] `SemanticToken` domain type
- [ ] Move classification logic from `semantic_tokens.rs`
- [ ] `semantic_tokens`, `semantic_tokens_range`, `delta_encode`
- [ ] `token_type_names`, `token_modifier_names`

### Step 6.4: Completion
- [ ] `CompletionContext`, `CompletionItem`, `CursorScope` domain types
- [ ] `completions` function

### Step 6.5: Hover
- [ ] `HoverInfo` domain type
- [ ] `hover` function

### Step 6.6: Signature help
- [ ] `SignatureInfo`, `ParamLabel` domain types
- [ ] `signature_help` function

### Step 6.7: Inlay hints
- [ ] `InlayHint`, `InlayHintKind` domain types
- [ ] `inlay_hints` function

### Step 6.8: Navigation
- [ ] `LocationResult` domain type
- [ ] `goto_definition`, `find_references`

### Step 6.9: Rename
- [ ] `RenameResult`, `FileEdit` domain types
- [ ] `prepare_rename`, `rename`

### Step 6.10: Code actions
- [ ] `CodeAction`, `CodeActionKind`, `CodeActionData`, `CodeActionEdit` domain types
- [ ] `code_actions`, `resolve_code_action`

### Step 6.11: Workspace symbols
- [ ] `WorkspaceSymbol` domain type
- [ ] `workspace_symbols`

---

## Phase 7: Build brink-web

Out of scope for this plan. Covered in a future `brink-web-spec.md`.
