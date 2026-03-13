# Architecture Overview

brink is organized as a workspace of focused crates with strict dependency rules. The central design principle is the **firewall**: `brink-format` is the only crate shared between the compiler and runtime, ensuring the runtime has zero knowledge of source-level concepts.

## The firewall principle

The crate graph is split into two halves by `brink-format`:

- **Compiler side** -- all crates that understand ink source code (syntax, IR, analysis, codegen). These are internal and may change without notice.
- **Runtime side** -- `brink-runtime`, which only understands compiled bytecode. It depends exclusively on `brink-format`.

This split has practical consequences:

- The runtime binary never links against the parser, analyzer, or codegen. Shipping `brink-runtime` does not pull in compiler internals.
- `brink-format` defines every type that crosses the boundary: `StoryData`, `ContainerDef`, `AddressDef`, `Opcode`, `Value`, `DefinitionId`, etc.
- Save-file portability: because the runtime speaks only in `DefinitionId`s (stable hashes), save state is not tied to a particular compilation.

## Published crates

| Crate | Purpose |
|-------|---------|
| `brink` | Public API -- re-exports from compiler and runtime |
| `brink-compiler` | Pipeline driver: `.ink` source to `StoryData` |
| `brink-runtime` | Bytecode VM for executing compiled stories |
| `brink-cli` | CLI tool: compile, convert, play |
| `brink-lsp` | Language server for ink files |

## Internal crates

| Crate | Purpose |
|-------|---------|
| `brink-syntax` | Lexer, parser, lossless CST, typed AST |
| `brink-ir` | HIR + LIR intermediate representations, lowering passes |
| `brink-analyzer` | Cross-file semantic analysis, symbol resolution |
| `brink-codegen-inkb` | Bytecode codegen: LIR to `StoryData` |
| `brink-codegen-json` | JSON codegen: LIR to `.ink.json` (for diffing against inklecate) |
| `brink-format` | Binary interface between compiler and runtime |
| `brink-db` | Incremental project database, file discovery |
| `brink-json` | Parser for inklecate `.ink.json` output |
| `brink-converter` | Reference pipeline: `.ink.json` to `StoryData` |
| `brink-test-harness` | Episode-based behavioral testing |

## Editor integration

| Crate | Purpose |
|-------|---------|
| `zed-brink` | Zed editor extension for ink files |

## Key dependency rules

1. **`brink-runtime`** depends ONLY on `brink-format` -- keeps the runtime minimal and embeddable, with no compiler dependencies
2. **`brink-lsp`** depends on `brink-analyzer`, NOT on `brink-compiler` -- the LSP only needs passes 1-5 (parse through validation), not codegen
3. **`brink-format`** has no brink-internal dependencies -- it is the stable interface layer
4. **`brink-format`** is the firewall -- source-level concepts (AST, HIR, symbols) never leak into the runtime

These rules enable the runtime to load new bytecode without the compiler present (hot-reload), keep compile times isolated, and ensure the runtime binary stays small for embedding.
