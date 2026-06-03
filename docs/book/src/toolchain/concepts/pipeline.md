# Compilation Pipeline

The compiler transforms `.ink` source files into bytecode through six phases:

```
Phase 1: Discovery + Parse    (brink-db, brink-syntax)    per-file    -> CST -> AST
Phase 2: HIR Lowering          (brink-ir::hir)             per-file    -> HIR
Phase 3: Analysis              (brink-analyzer)            cross-file  -> symbol resolution, types
Phase 4: LIR Lowering          (brink-ir::lir)             cross-file  -> unified LIR program
Phase 5: Bytecode Codegen      (brink-codegen-inkb)        per-container -> StoryData
Phase 6: Output                (brink-format)              -> .inkb / .inkt
```

The LSP runs phases 1-3. The compiler runs all phases.

## Phase 1: Discovery + Parse

`ProjectDb::discover()` finds all `.ink` files starting from the entry point, following `INCLUDE` directives. Each file is parsed by `brink-syntax` into a lossless CST (via rowan) and then into a typed AST. The parser uses error recovery and always produces output, even for malformed input.

## Phase 2: HIR Lowering

The AST is lowered to HIR (High-level Intermediate Representation) per-file. This phase handles weave folding -- converting the flat sequence of choices and gathers in ink source into a container tree. Implicit structure like root containers and auto-entering the first stitch is materialized here.

## Phase 3: Analysis

Cross-file semantic analysis merges per-file symbol manifests into a unified symbol index, resolves names, and performs type checking. The project database (`brink-db`) supports incremental updates for the LSP.

## Phase 4: LIR Lowering

Per-file HIR plus analysis resolutions are lowered into a unified LIR (Low-level IR) program. The LIR is a flat, container-oriented representation ready for bytecode emission. Container planning, label allocation, and instruction selection happen here.

## Phase 5: Bytecode Codegen

`brink-codegen-inkb` walks the LIR and emits bytecode per container, producing the `StoryData` structure: containers with bytecode, line tables, variable definitions, list definitions, address labels, and external function declarations. All cross-definition references use `DefinitionId`, resolved at link time by the runtime.

## Phase 6: Output

`StoryData` can be serialized to `.inkb` (binary format for production) or `.inkt` (human-readable text dump for debugging).

## Entry points

| Function | Description |
|----------|-------------|
| `compile_path(path)` | Full pipeline from a file path |
| `compile(entry, read_file)` | Full pipeline with a custom file reader (for WASM, tests) |
| `compile_to_json(entry, read_file)` | Stop at LIR, emit `.ink.json` format (for diffing against inklecate) |
| `compile_string_to_json(source)` | Quick JSON emit from a source string |

## Converter pipeline

A separate pipeline exists for processing inklecate's output: `.ink.json` -> parse (`brink-json`) -> convert (`brink-converter`) -> `StoryData`. This is the known-good reference used for validating the native compiler's output.
