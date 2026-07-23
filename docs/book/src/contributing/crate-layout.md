# Crate Layout

brink is organized as a Cargo workspace with strict dependency rules. The central design principle is the **firewall**: `brink-format` is the only crate shared between the compiler and runtime.

## Published crates

| Crate | Path | Purpose |
|-------|------|---------|
| `brink-compiler` | `crates/brink-compiler/` | Pipeline driver: `.ink` to `StoryData` |
| `brink-runtime` | `crates/brink-runtime/` | Bytecode VM for executing compiled stories |
| `brink-cli` | `crates/brink-cli/` | CLI tool: compile, convert, play, replay, ide, export-xliff, compile-locale, regenerate-xliff, fmt |
| `brink-lsp` | `crates/brink-lsp/` | Language server for ink files |
| `brink-web` | `crates/brink-web/` | WASM bindings for the IDE + runtime; powers the web playground |
| `bevy-brink` | `crates/bevy-brink/` | Bevy 0.19 integration: plugin, assets, components, external-function bindings |

## Internal crates

| Crate | Path | Purpose |
|-------|------|---------|
| `brink-syntax` | `crates/internal/brink-syntax/` | Lexer, parser, lossless CST, typed AST |
| `brink-ir` | `crates/internal/brink-ir/` | HIR + LIR intermediate representations, lowering |
| `brink-analyzer` | `crates/internal/brink-analyzer/` | Cross-file semantic analysis, symbol resolution |
| `brink-driver` | `crates/internal/brink-driver/` | Pipeline orchestration: file discovery + cross-file analysis |
| `brink-codegen-inkb` | `crates/internal/brink-codegen-inkb/` | Bytecode codegen: LIR to `StoryData` |
| `brink-format` | `crates/internal/brink-format/` | Binary interface between compiler and runtime |
| `brink-db` | `crates/internal/brink-db/` | Incremental project database, file discovery |
| `brink-source-tree` | `crates/internal/brink-source-tree/` | `SourceTree` trait: host-agnostic seam for enumerating/reading `.brink` source files |
| `brink-fmt` | `crates/internal/brink-fmt/` | `.ink` source formatter (powers `brink fmt`) |
| `brink-intl` | `crates/internal/brink-intl/` | Internationalization tooling: line export, XLIFF round-trip, `.inkl` compile, ICU plurals |
| `xliff2` | `crates/internal/xliff2/` | General-purpose XLIFF 2.0 read/write library |
| `brink-ide` | `crates/internal/brink-ide/` | Protocol-agnostic IDE query library (shared by the LSP/web) |
| `bevy-brink-derive` | `crates/internal/bevy-brink-derive/` | Derive macros for `bevy-brink` (`#[derive(BrinkCommand)]`) |
| `brink-test-harness` | `crates/internal/brink-test-harness/` | Episode-based behavioral testing (oracle corpus) |

Internal crates have `publish = false` and are not published to crates.io.

`crates/brink/` is an empty umbrella crate — it holds the `brink` name on
crates.io and ships no code. There is no facade re-exporting the compiler and
runtime; depend on `brink-compiler` and `brink-runtime` directly.

## Editor plugins

| Crate | Path | Purpose |
|-------|------|---------|
| `zed-brink` | `crates/zed-brink/` | Zed editor extension |

## Key dependency rules

1. **`brink-runtime`** depends ONLY on `brink-format` — keeps the runtime minimal and embeddable
2. **`brink-lsp`** depends on `brink-analyzer`, NOT on `brink-compiler` — the LSP needs parse through validation, not codegen
3. **`brink-format`** has no brink-internal dependencies — it is the stable interface layer
4. **`brink-format`** is the firewall — source-level concepts never leak into the runtime

These rules enable hot-reload (runtime loads new bytecode without the compiler), compile-time isolation (changing compiler internals doesn't rebuild the runtime), and small runtime binaries for embedding.

## Workspace conventions

- **Dependencies** are declared in `[workspace.dependencies]` in the root `Cargo.toml` and referenced via `dep.workspace = true` in each crate
- **Lints** are configured in `[workspace.lints]` and inherited via `[lints] workspace = true`
- **Edition, license, repository** are set in `[workspace.package]` and inherited with `field.workspace = true`
