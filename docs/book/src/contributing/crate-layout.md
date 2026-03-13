# Crate Layout

brink is organized as a Cargo workspace with strict dependency rules. The central design principle is the **firewall**: `brink-format` is the only crate shared between the compiler and runtime.

## Published crates

| Crate | Path | Purpose |
|-------|------|---------|
| `brink` | `crates/brink/` | Public API -- re-exports from compiler and runtime |
| `brink-compiler` | `crates/brink-compiler/` | Pipeline driver: `.ink` to `StoryData` |
| `brink-runtime` | `crates/brink-runtime/` | Bytecode VM for executing compiled stories |
| `brink-cli` | `crates/brink-cli/` | CLI tool: compile, convert, play |
| `brink-lsp` | `crates/brink-lsp/` | Language server for ink files |

## Internal crates

| Crate | Path | Purpose |
|-------|------|---------|
| `brink-syntax` | `crates/internal/brink-syntax/` | Lexer, parser, lossless CST, typed AST |
| `brink-ir` | `crates/internal/brink-ir/` | HIR + LIR intermediate representations, lowering |
| `brink-analyzer` | `crates/internal/brink-analyzer/` | Cross-file semantic analysis, symbol resolution |
| `brink-codegen-inkb` | `crates/internal/brink-codegen-inkb/` | Bytecode codegen: LIR to `StoryData` |
| `brink-codegen-json` | `crates/internal/brink-codegen-json/` | JSON codegen: LIR to `.ink.json` (for diffing) |
| `brink-format` | `crates/internal/brink-format/` | Binary interface between compiler and runtime |
| `brink-db` | `crates/internal/brink-db/` | Incremental project database, file discovery |
| `brink-json` | `crates/internal/brink-json/` | Parser for inklecate `.ink.json` output |
| `brink-converter` | `crates/internal/brink-converter/` | Reference pipeline: `.ink.json` to `StoryData` |
| `brink-test-harness` | `crates/internal/brink-test-harness/` | Episode-based behavioral testing |

Internal crates have `publish = false` and are not published to crates.io.

## Editor plugins

| Crate | Path | Purpose |
|-------|------|---------|
| `zed-brink` | `crates/zed-brink/` | Zed editor extension |

## Key dependency rules

1. **`brink-runtime`** depends ONLY on `brink-format` -- keeps the runtime minimal and embeddable
2. **`brink-lsp`** depends on `brink-analyzer`, NOT on `brink-compiler` -- the LSP needs parse through validation, not codegen
3. **`brink-format`** has no brink-internal dependencies -- it is the stable interface layer
4. **`brink-format`** is the firewall -- source-level concepts never leak into the runtime

These rules enable hot-reload (runtime loads new bytecode without the compiler), compile-time isolation (changing compiler internals doesn't rebuild the runtime), and small runtime binaries for embedding.

## Workspace conventions

- **Dependencies** are declared in `[workspace.dependencies]` in the root `Cargo.toml` and referenced via `dep.workspace = true` in each crate
- **Lints** are configured in `[workspace.lints]` and inherited via `[lints] workspace = true`
- **Edition, license, repository** are set in `[workspace.package]` and inherited with `field.workspace = true`
