# Architecture & the Firewall

brink is a workspace of focused crates with strict dependency rules. The central
design principle is the **firewall**: `brink-format` is the only crate shared
between the compiler and the runtime, so the runtime has zero knowledge of
source-level concepts.

## The firewall principle

The crate graph is split into two halves by `brink-format`:

- **Compiler side** — every crate that understands ink source code (syntax, IR,
  analysis, codegen). Internal; may change without notice.
- **Runtime side** — `brink-runtime`, which only understands compiled bytecode.
  It depends exclusively on `brink-format`.

This split has real, practical consequences:

- **The runtime never links the compiler.** Shipping `brink-runtime` does not
  pull in the parser, analyzer, or codegen — the embeddable binary stays small.
- **`brink-format` defines everything that crosses the boundary** — `StoryData`,
  `ContainerDef`, `AddressDef`, `Opcode`, `Value`, `DefinitionId`, and the rest.
  Source-level types (AST, HIR, symbols) never leak through it.
- **Hot-reload works** — because the runtime loads bytecode without the compiler
  present, new `StoryData` can be swapped in at runtime.
- **Save-file portability** — the runtime speaks only in `DefinitionId`s (stable
  hashes), so save state isn't tied to a particular compilation.

The same firewall is why the LSP depends on the analyzer but **not** the
compiler: editor features need parse-through-analysis, not codegen.

> For the full crate inventory, paths, and the exact dependency rules, see
> [Contributing › Crate Layout](../../contributing/crate-layout.md). For how
> source becomes bytecode, see [The Compilation Pipeline](./pipeline.md).
