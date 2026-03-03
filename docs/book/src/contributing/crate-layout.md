# Crate Layout

brink is organized as a Cargo workspace with strict dependency rules. The central design principle is the **firewall**: `brink-format` is the only crate shared between the compiler and runtime.

## Published crates

| Crate | Path | Purpose |
|-------|------|---------|
| `brink` | `crates/brink/` | Public API — re-exports from compiler and runtime |
| `brink-compiler` | `crates/brink-compiler/` | Pipeline driver + codegen backends |
| `brink-runtime` | `crates/brink-runtime/` | Bytecode VM for executing compiled stories |
| `brink-cli` | `crates/brink-cli/` | CLI for compiling and running ink stories |
| `brink-lsp` | `crates/brink-lsp/` | Language server for ink files |
| `brink-intl` | `crates/brink-intl/` | Batteries-included plural resolution (ICU4X baked data) |

## Internal crates

| Crate | Path | Purpose |
|-------|------|---------|
| `brink-syntax` | `crates/internal/brink-syntax/` | Lexer, parser, lossless CST, typed AST |
| `brink-hir` | `crates/internal/brink-hir/` | HIR types + per-file lowering from AST |
| `brink-analyzer` | `crates/internal/brink-analyzer/` | Cross-file semantic analysis, project database |
| `brink-format` | `crates/internal/brink-format/` | Binary interface between compiler and runtime |
| `brink-json` | `crates/internal/brink-json/` | Parser for inklecate `.ink.json` output format |
| `brink-converter` | `crates/internal/brink-converter/` | Converts `.ink.json` to `.inkb` (bootstraps runtime testing) |

Internal crates have `publish = false` and are not published to crates.io.

## Key dependency rules

1. **`brink-runtime`** depends ONLY on `brink-format`
2. **`brink-lsp`** depends on `brink-analyzer`, NOT on `brink-compiler`
3. **`brink-format`** has no brink-internal dependencies
4. **`brink-intl`** depends ONLY on `brink-format`

<!-- TODO: explain _why_ these rules exist:
  - Runtime isolation — keeps the runtime minimal and embeddable
  - Compile-time isolation — LSP doesn't need codegen
  - Format as firewall — source-level concepts never leak into the runtime
  - Hot-reload — runtime can load new bytecode without compiler present
-->

## Workspace conventions

- **Dependencies** are declared in `[workspace.dependencies]` in the root `Cargo.toml` and referenced via `dep.workspace = true` in each crate
- **Lints** are configured in `[workspace.lints]` and inherited via `[lints] workspace = true`
- **Edition, license, repository** are set in `[workspace.package]` and inherited with `field.workspace = true`
