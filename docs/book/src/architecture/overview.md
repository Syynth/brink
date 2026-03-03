# Architecture Overview

brink is organized as a workspace of focused crates with strict dependency rules. The central design principle is the **firewall**: `brink-format` is the only crate shared between the compiler and runtime, ensuring the runtime has zero knowledge of source-level concepts.

<!-- TODO: include the mermaid dependency graph from spec.md -->

## Published crates

| Crate | Purpose |
|-------|---------|
| `brink` | Public API — re-exports from compiler and runtime |
| `brink-compiler` | Pipeline driver + codegen backends |
| `brink-runtime` | Bytecode VM for executing compiled stories |
| `brink-cli` | CLI for compiling and running ink stories |
| `brink-lsp` | Language server for ink files |
| `brink-intl` | Batteries-included plural resolution (ICU4X baked data) |

## Internal crates

| Crate | Purpose |
|-------|---------|
| `brink-syntax` | Lexer, parser, lossless CST, typed AST |
| `brink-hir` | HIR types + per-file lowering from AST |
| `brink-analyzer` | Cross-file semantic analysis, project database |
| `brink-format` | Binary interface between compiler and runtime |

## Key dependency rules

1. `brink-runtime` depends ONLY on `brink-format`
2. `brink-lsp` depends on `brink-analyzer`, NOT on `brink-compiler`
3. `brink-format` has no brink-internal dependencies
4. `brink-intl` depends ONLY on `brink-format`

<!-- TODO: explain _why_ these rules exist (hot-reload, compile-time isolation, etc.) -->
