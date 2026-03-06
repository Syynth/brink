# Compiler pipeline audit findings

Audit date: 2026-03-05. Covers `compiler-spec.md` vs implementation across `brink-syntax`, `brink-hir`, `brink-analyzer`, `brink-format`, and `brink-compiler`.

## Crate status summary

| Crate | Status | Notes |
|-------|--------|-------|
| `brink-syntax` | **Complete** | 159 SyntaxKinds, full Pratt parser (10 precedence levels), all ink constructs, error recovery, lossless CST |
| `brink-hir` | **Complete** | Full type model (50+ types), lowering, weave folding, symbol manifests, 21 diagnostic codes, no stubs |
| `brink-format` | **~95%** | All 75+ opcodes, all definition types, `.inkb`/`.inkt` serialization. `.inkl` locale overlay not implemented |
| `brink-analyzer` | **~10%** | Manifest merging + DefinitionId hashing only. No resolution, type-check, or validation |
| `brink-compiler` | **Stub** | Calls analyzer, returns empty `StoryData`. No codegen |

## 1. Spec describes it, not implemented

### 1.1 brink-analyzer (passes 3-5)

Almost entirely unimplemented. The crate has a working skeleton that merges per-file symbol manifests into a `SymbolIndex`, but all semantic analysis is stubbed.

**Name resolution** — spec says paths are resolved to concrete `DefinitionId`s. `SymbolIndex::resolve()` returns `None` with comment "Stub -- real resolution is a future pass." Unresolved references from `SymbolManifest.unresolved` are completely ignored.

**INCLUDE resolution** — spec says the analyzer resolves the INCLUDE file graph and calls `fold_weave` on merged top-level content. Not implemented. `IncludeSite` structs from HIR are ignored.

**Circular include detection** — spec lists it. Not implemented.

**Scope analysis** — spec says "temp is function-scoped, VAR/CONST are global." `Scope` struct exists with `knot`/`stitch` fields but is never populated or queried.

**Type checking** — spec says "expression types, assignment compatibility." Not implemented.

**Validation** — spec says "undefined targets, duplicate declarations, dead code, unused variables." Not implemented. Analyzer returns empty diagnostics always.

**Duplicate detection** — comment in `manifest.rs` says "No duplicate detection or validation yet."

**Project database** — spec says "the analyzer also owns the project database -- the stateful, long-lived cache." Not implemented. `analyze()` is a stateless function taking all files at once.

**Incremental analysis** — spec says analyzer caches HIR per knot, uses rowan green node identity. Not implemented.

### 1.2 brink-compiler (pass 6)

Entirely unimplemented. `compile()` in `lib.rs` calls the analyzer and returns an empty `StoryData`.

**Expression lowering** — spec says "expression trees -> stack ops + jumps." Not implemented.

**Choice lowering** — spec describes dual-path bytecode model (high-level `EvalLine`/`ChoiceOutput` vs low-level `BeginStringEval`). Not implemented.

**Sequence lowering** — spec describes `Sequence`/`SequenceBranch` opcodes. Not implemented.

**Divert/tunnel/thread lowering** — spec describes control flow opcodes (`Goto`, `Call`, `TunnelCall`, `ThreadCall`, etc.). Not implemented.

**Text decomposition** — spec describes line template building with slot assignment and `EmitLine`. Not implemented.

**Per-container line table building** — spec describes content + source text content hash per line entry. Not implemented.

**Implicit diverts** — spec says "end-of-root-story gets implicit gather + `-> DONE`." HIR has first-stitch auto-enter but codegen doesn't exist to emit the root DONE.

**Container boundary decisions** — spec says "codegen decides which HIR nodes become bytecode containers." Not implemented.

### 1.3 brink-format

**`.inkl` locale overlay** — spec describes format (magic `b"INKL"`, BCP 47 locale, per-container line tables, audio table). No code exists -- no module, no read/write functions.

## 2. Spec/code mismatches

### 2.1 PluralResolver signature

- **Spec:** `fn cardinal(&self, number: i64, fraction: Option<&str>)` -- second param is a fractional part
- **Code:** `fn cardinal(&self, n: i64, locale_override: Option<&str>)` -- second param is a locale override
- Different semantics. Needs reconciliation.

### 2.2 LinePart::Select structure

- **Spec:** `variants: Vec<(SelectKey, Vec<LinePart>)>` -- recursive nesting (selects inside selects)
- **Code:** `variants: Vec<(SelectKey, String)>` -- flat strings
- Flat strings prevent nesting a plural inside a gender select. May be an intentional simplification but limits template expressiveness.

### 2.3 Debug info representation

- **Spec:** `.inkb` has a "Debug info (strippable, source maps)" section
- **Code:** source location is embedded as `SourceLocation` opcodes inline in bytecode
- Both approaches work. Inline opcodes are strippable (just remove them) but the spec should match the implementation.

### 2.4 Inflated counts in spec

- **Spec says:** "~230 `SyntaxKind` variants" -- **actual:** 159
- **Spec says:** "140+ zero-cost newtype wrappers" -- **actual:** ~55 AST node types with ~100 accessor methods

### 2.5 Undocumented StoryData fields

`StoryData` has `list_literals: Vec<ListValue>` and `labels: Vec<LabelDef>` which appear in `.inkb` as optional backwards-compat sections but aren't mentioned in the spec's `.inkb` section description.

## 3. Implemented but not in spec

These exist in code but aren't documented in compiler-spec.md:

- **`Stmt::ExprStmt(Expr)`** in HIR -- bare expression statements (e.g., `~ expr`)
- **`FloatBits`** in HIR -- floats stored as raw `u64` bits for `Eq`/`Hash` derivation
- **`InfixOp::Pow`** in HIR -- `^` means power on numbers but intersection on lists. Potential semantic ambiguity to document.
- **`AssignOp` variants** in HIR -- `Set`, `Add`, `Sub` for `=`, `+=`, `-=`
- **`Param::is_divert`** in HIR -- flag for tunnel return divert target parameters
- **Cast/math opcodes** in format -- `CastToInt`, `CastToFloat`, `Floor`, `Ceiling`, `Pow`, `Min`, `Max` not enumerated in spec opcode categories
- **`CurrentVisitCount`** opcode -- exists in format, not in spec
- **`Nop`** opcode -- exists in format, not in spec
- **`BeginStringEval`/`EndStringEval`** -- mentioned in choice text section but not in opcode categories list

## 4. Design gaps to resolve before implementing

### 4.1 Temp slot assignment scope

Spec says "Temp slot indices are assigned by the compiler/converter across the entire knot/function scope (including all child containers reached by flow entry)." Codegen needs a pre-pass collecting all temp declarations across the knot and its children before emitting bytecode. Algorithm not specified.

### 4.2 Loose end propagation

Spec says "choices inside conditionals stay nested within `Stmt::Conditional`; weave transparency is deferred to codegen/runtime via loose end propagation." References `Weave.cs` `PassLooseEndsToAncestors` but no brink-specific algorithm exists. This is critical for correct choice compilation.

### 4.3 Container hierarchy construction

Spec shows the container tree (root -> knots -> stitches -> gathers) but doesn't specify how codegen walks the HIR to produce this. Key questions:

- Does every `ChoiceSet` gather become a container?
- Every labeled gather?
- Only gathers that are divert targets?
- What about choice branches -- are they containers?

### 4.4 Short-circuit and/or

Spec says "handled by compiler (emits conditional jumps), not VM." The actual jump pattern (`JumpIfFalse` for `and`, jump-if-true for `or`) isn't specified.

### 4.5 VariablePointer / ref parameter design

Format-spec flags this: "**STATUS: needs review** -- the write-through/auto-deref semantics are implemented but have not been validated against the full ref parameter design."

### 4.6 FLOW VAR instance flag

Decision log says `FLOW VAR` scoping propagates through the compiler into `GlobalVarDef` as a single bit. Neither the analyzer nor format have implemented this flag yet.

### 4.7 `^` operator ambiguity

`^` is parsed at the `Intersect` precedence level and means list intersection on lists but power on numbers. The HIR has `InfixOp::Pow` but it's the same source token. Needs clear documentation of how codegen/runtime disambiguates (presumably by runtime type).

## 5. Architectural decision: HIR/LIR split

**Decision (2026-03-05):** Rename `brink-hir` to `brink-ir`. Existing HIR types become `brink_ir::hir`. A new `brink_ir::lir` defines the post-analysis, codegen-ready representation. The analyzer transforms HIR -> LIR. Two codegen backends walk the LIR:

- **`.inkb`** -- brink native bytecode
- **`.ink.json`** -- inklecate-compatible JSON (brink as drop-in replacement compiler)

### Why a separate LIR

The HIR is a rich semantic tree optimized for diagnostics: expressions as trees, nested choice structures, unresolved paths. Codegen needs a different shape: resolved references, decided container boundaries, linearized control flow, loose ends connected, temp slots assigned.

The dual-backend requirement further motivates this -- `.inkb` and `.ink.json` have fundamentally different instruction models. Both need to walk a common resolved representation.

### LIR design considerations

The LIR should capture:

- Resolved container hierarchy (root -> knots -> stitches -> sub-containers)
- Each container's content as a linearized sequence of semantic operations
- All references resolved to `DefinitionId`
- Temp slot indices assigned per knot/function scope
- Loose ends connected (choices without explicit diverts -> gathers)
- Choice text decomposed (display line + output line)
- Sequence state tracked

The LIR sits above both backends -- resolved and linearized, but still in terms of semantic operations (emit text, evaluate expression, begin choice, divert to container) rather than target-specific instructions.

### What diverges per backend

- **`.inkb`**: expressions -> stack opcodes, control flow -> jumps, text -> line table entries with `EmitLine`
- **`.ink.json`**: expressions -> `ev`/value/`/ev` inline arrays, control flow -> named container nesting, text -> inline string values with `^\n`

## 6. Crate layout and dependency graph

```
brink-syntax  (no brink deps)
     │
brink-ir  (depends on brink-syntax, brink-format)
  ├── hir/         HIR types + AST→HIR lowering
  ├── symbols/     SymbolManifest + SymbolIndex types
  └── lir/         LIR types + HIR→LIR lowering (consumes SymbolIndex)
     │
brink-analyzer  (depends on brink-ir)
  └── takes SymbolManifests → populates SymbolIndex
     │
brink-compiler  (depends on brink-ir, brink-analyzer, brink-syntax, brink-format)
  ├── driver       INCLUDE discovery, parse/lower orchestration
  └── backend/     inkb (LIR → StoryData), inkjson (LIR → .ink.json)
```

brink-ir module structure:

```
crates/internal/brink-ir/src/
├── lib.rs
├── hir/
│   ├── types.rs        HirFile, Block, Stmt, Expr, ...
│   └── lower.rs        AST → HIR + SymbolManifest
├── lir/
│   ├── types.rs        LirContainer, LirOp, ...
│   ├── lower.rs        HIR + SymbolIndex → LIR
│   └── resolve.rs      reference resolution pass
└── symbols/
    ├── manifest.rs     SymbolManifest, DeclaredSymbol, UnresolvedRef
    └── index.rs        SymbolIndex, SymbolInfo, SymbolKind, Scope
```

brink-compiler module structure:

```
crates/brink-compiler/src/
├── lib.rs              compile(), compile_path()
├── driver.rs           INCLUDE discovery, parse/lower orchestration
└── backend/
    ├── mod.rs
    ├── inkb.rs         LIR → StoryData (brink bytecode)
    └── inkjson.rs      LIR → ink.json (inklecate-compatible)
```

## 7. Compilation driver architecture

`brink-compiler` is both the driver and the codegen. It orchestrates the full pipeline — INCLUDE discovery, parsing, lowering, analysis, LIR construction, codegen — but does not perform filesystem I/O directly. File reading is injected by the caller.

```
brink-compiler (driver + codegen, I/O-agnostic)
  ├─ receives entry point + file reader callback
  ├─ parses entry file → AST, scans for INCLUDEs
  ├─ calls back to reader for each included file, recursively
  ├─ lowers each file (brink-ir::hir)
  ├─ feeds all HIR + manifests to brink-analyzer (pure, no I/O)
  ├─ builds LIR from HIR + SymbolIndex (brink-ir::lir)
  └─ runs codegen backend → StoryData

brink-lsp (own driver, no compiler dependency)
  ├─ gets file contents from editor notifications
  ├─ parses / re-lowers incrementally
  └─ feeds brink-analyzer directly
```

The public API re-exported by `brink`:

```
brink::compiler::compile(entry, read_file) → StoryData
brink::runtime::Story::new(story_data)     → playable story
```

`brink-cli` is thin — calls `compile` with `std::fs::read_to_string`, optionally writes `.inkb` to disk. The analyzer is a pure function/database shared by both compiler and LSP, with no filesystem awareness.

## 7. Implementation priorities

### Analyzer (roughly in order)

1. INCLUDE file graph resolution + circular detection
2. Cross-file manifest merging with `fold_weave` on merged top-level content
3. Name resolution (paths -> DefinitionId using scope context)
4. Duplicate declaration detection
5. Type inference/checking for expressions
6. Validation (undefined targets, unused vars, dead code)
7. Project database (for LSP -- can defer if compiler-only for now)

### LIR construction (analyzer output)

1. Container boundary identification (walk HIR, decide what becomes a container)
2. Temp slot assignment across knot/function scope
3. Loose end propagation (connect choices without explicit diverts to gathers)
4. Choice text decomposition (display + output lines)
5. Reference resolution (all paths -> DefinitionId)

### Codegen: `.inkb` backend

1. Expression lowering -> stack ops
2. Text decomposition -> line table entries
3. Control flow lowering (diverts, tunnels, threads -> opcodes)
4. Choice lowering (dual-path opcodes)
5. Sequence lowering -> Sequence/SequenceBranch opcodes
6. Short-circuit and/or -> conditional jumps
7. Implicit root DONE insertion

### Codegen: `.ink.json` backend

1. Container tree serialization (named containers with content arrays)
2. Expression serialization (`ev`/value/`/ev` wrapping)
3. Control flow serialization (divert objects, tunnel markers)
4. Choice serialization (choice point objects)
5. Variable/list/external declaration serialization
