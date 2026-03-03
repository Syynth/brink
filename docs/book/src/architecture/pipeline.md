# Compilation Pipeline

The compiler transforms `.ink` source files into bytecode through six passes:

```
Pass 1: Parse          (brink-syntax)     per-file       → AST
Pass 2: Lower          (brink-hir)        per-file       → HIR + SymbolManifest
Pass 3: Merge/Resolve  (brink-analyzer)   cross-file     → unified SymbolIndex
Pass 4: Type-check     (brink-analyzer)   cross-file     → type annotations
Pass 5: Validate       (brink-analyzer)   cross-file     → diagnostics
Pass 6: Codegen        (brink-compiler)   per-container  → bytecode + tables
```

The LSP runs passes 1-5. The compiler runs all 6.

## Pass 1: Parse

<!-- TODO: explain brink-syntax
  - Lossless CST via rowan (every byte preserved)
  - Error recovery — always produces output
  - ~230 SyntaxKind variants
  - 140+ typed AST wrappers
  - Pratt expression parser with 10 precedence levels
-->

## Pass 2: Lower

<!-- TODO: explain brink-hir
  - Weave folding: flat choices/gathers → container tree
  - Implicit structure (root container, auto-enter first stitch)
  - Per-file scope — no cross-file context needed
-->

## Passes 3-5: Analyze

<!-- TODO: explain brink-analyzer
  - Cross-file symbol merging and name resolution
  - Type checking and validation
  - Project database for incremental updates (LSP)
-->

## Pass 6: Codegen

<!-- TODO: explain brink-compiler codegen
  - Per-container bytecode emission
  - Expression → stack ops + jumps
  - Text decomposition → line templates
  - All references use DefinitionId (resolved at link time, not compile time)
-->
