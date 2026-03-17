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

## Phase 1: Create brink-driver + migrate compiler ✅

Reference: [brink-driver-spec § API surface](brink-driver-spec.md#api-surface)

**Done** in commit `79cec0f`. Phases 1 and 2 were implemented together since Driver methods delegate to ProjectDb and the compiler was the natural first consumer.

What was done:
- [x] `brink-driver` crate: `Driver` struct, `discover.rs` (BFS + `DiscoverError`), `diagnostics.rs` (`collect_diagnostics` + `DiagnosticReport`)
- [x] `resolve_include_path` made `pub` in brink-db, changed to string-based `rfind('/')` for WASM compat
- [x] `ProjectDb::find_cycle()` public wrapper added
- [x] Compiler rewritten to use `Driver` (discover → analyze → collect_diagnostics → lir_inputs → lower → emit)
- [x] `CompileError::From<brink_driver::DiscoverError>` replaces `From<brink_db::DiscoverError>`
- [x] 9 unit tests: 5 path resolution, 4 diagnostic collection
- [x] All verification checks pass: workspace clippy, fmt, tests, episode ratchet, wasm target

---

## Phase 3+4: LSP migration + brink-db cleanup ✅

Reference: [brink-driver-spec § Changes to brink-lsp](brink-driver-spec.md#changes-to-brink-lsp), [§ Changes to brink-db](brink-driver-spec.md#changes-to-brink-db)

**Done** in commit `4c1f19a`. Phases 3 and 4 were combined — the LSP's diagnostic code is structurally different from the compiler's (per-project analysis outside the lock with multi-project annotation), so `Driver::collect_diagnostics()` doesn't fit. LSP migration was minimal (add dep only); the real work was the brink-db cleanup.

What was done:
- [x] Removed `ProjectDb::analyze()`, `ProjectDb::discover()`, `analysis` field, all cache invalidation
- [x] Removed `DiscoverError` enum and `pub use brink_analyzer::AnalysisResult` re-export from brink-db
- [x] Removed `brink-analyzer` dependency from brink-db
- [x] Moved analysis cache into `Driver` with auto-invalidation on `db_mut()`
- [x] Fixed diagnostic tests to use `brink_analyzer::analyze()` directly
- [x] Cleaned brink-compiler prod deps: removed `brink-db`, `brink-analyzer`, `brink-syntax`, `rowan` (moved `brink-analyzer`, `brink-syntax` to dev-deps for test file)
- [x] Added `brink-driver` to brink-lsp (dep only, no functional code changes)
- [x] All verification checks pass: workspace clippy, fmt, tests, episode ratchet, wasm target

What was kept on `ProjectDb` (cache accessors used by both Driver and LSP):
- `compute_projects()`, `analysis_inputs()`, `analysis_inputs_for()`, `file_metadata()`, `file_ids_topo()`

Note: Step 4.2 (resolve_include_path) was already done in Phase 1 — `resolve_include_path` in brink-db was already string-based (`rfind('/')`), no `std::path::Path` usage existed.

---

## Phase 5: Create brink-ide ✅

Reference: [brink-ide-spec § API surface](brink-ide-spec.md#api-surface), [§ Domain types](brink-ide-spec.md#domain-types)

**Done.** Crate scaffolded, foundational types and text utilities moved from brink-lsp. Compiles for both native and `wasm32-unknown-unknown`.

What was done:
- [x] Scaffolded `crates/internal/brink-ide/` with Cargo.toml (deps: brink-syntax, brink-ir, brink-analyzer, brink-fmt, rowan). No `bitflags` yet — needed in Phase 6.3 for semantic tokens.
- [x] Workspace auto-includes via `crates/internal/*` glob — no root Cargo.toml edit needed
- [x] Moved `LineIndex` struct + `new()`, `line_col()`, `offset()` from `brink-lsp/src/convert.rs` to `brink-ide/src/line_index.rs`. 5 original tests moved + 1 new roundtrip test added.
- [x] `brink-lsp/src/convert.rs`: replaced struct definition with `pub use brink_ide::LineIndex`. Kept `to_lsp_range`, `to_text_size`, `symbol_kind_to_lsp`, `severity_to_lsp`, `diagnostic_to_lsp` and their tests.
- [x] Moved text utilities to `brink-ide/src/text.rs`: `word_at_offset`, `word_range_at_offset`, `builtin_hover_text`, `find_call_context`
- [x] Moved `diff_to_edits` to `brink-ide/src/text.rs` — rewritten to return `Vec<(TextRange, String)>` instead of LSP `TextEdit`. Added `diff_to_lsp_edits` adapter in backend.rs that converts via `LineIndex`.
- [x] Moved completion helpers to `brink-ide/src/completion.rs`: `CompletionContext` enum, `CursorScope` struct, `detect_completion_context`, `cursor_scope`, `is_visible_in_context` — all made `pub`.
- [x] Moved 14 completion/scope tests from `brink-lsp/src/backend.rs` to `brink-ide/src/completion.rs`
- [x] Added `brink-ide` dependency to `brink-lsp/Cargo.toml`
- [x] Updated `brink-lsp/src/backend.rs`: removed moved functions/types/tests, added imports from `brink_ide`
- [x] 21 brink-ide tests pass, 21 brink-lsp tests pass, full workspace passes
- [x] All verification checks pass: native + wasm compile, clippy, fmt

What was NOT moved (deferred to Phase 6):
- `find_def_at_offset` — depends on LSP-specific `NavigationSnapshot` (Phase 6.8)
- `make_completion_item` — returns `lsp_types::CompletionItem` (Phase 6.4)
- `collect_code_actions`, `format_region`, `sort_knots_in_source`, `sort_stitches_in_knot` — query functions (Phase 6.10, 6.1)
- `collect_param_hints` — query function (Phase 6.7)
- Semantic tokens module — entire file (Phase 6.3)

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
- [ ] `CompletionItem` domain type (distinct from `lsp_types::CompletionItem`)
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
