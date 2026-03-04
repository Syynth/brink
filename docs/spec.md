# brink specification

brink is the ink compiler and bytecode runtime for s92-studio, extracted into its own repository to simplify context management for agents. All s92 runtime requirements carry over — this is an organizational separation, not a functional one.

## Crate layout

### Published crates

| Crate | Path | Purpose |
|-------|------|---------|
| `brink` | `crates/brink/` | Public API — re-exports from compiler and runtime |
| `brink-compiler` | `crates/brink-compiler/` | Pipeline driver + codegen |
| `brink-runtime` | `crates/brink-runtime/` | Bytecode VM for executing compiled stories |
| `brink-cli` | `crates/brink-cli/` | CLI for compiling and running ink stories |
| `brink-lsp` | `crates/brink-lsp/` | Language server for ink files |
| `brink-intl` | `crates/brink-intl/` | Batteries-included plural resolution (ICU4X baked data) |

### Internal crates

| Crate | Path | Purpose |
|-------|------|---------|
| `brink-syntax` | `crates/internal/brink-syntax/` | Lexer, parser, lossless CST, typed AST |
| `brink-hir` | `crates/internal/brink-hir/` | HIR types, per-file lowering from AST |
| `brink-analyzer` | `crates/internal/brink-analyzer/` | Cross-file semantic analysis, project database |
| `brink-format` | `crates/internal/brink-format/` | Binary interface between compiler and runtime |

Whether `brink-format` needs to be published is deferred.

### Dependency graph

```mermaid
graph TD
    cli[brink-cli] --> compiler[brink-compiler]
    cli --> lsp[brink-lsp]
    cli --> runtime[brink-runtime]

    compiler --> analyzer[brink-analyzer]
    compiler --> hir[brink-hir]
    compiler --> syntax[brink-syntax]
    compiler --> format[brink-format]

    lsp --> analyzer

    analyzer --> hir
    hir --> syntax

    runtime --> format

    intl[brink-intl] --> format

    style format fill:#f9f,stroke:#333,stroke-width:3px
```

**Dependency rules:**

1. `brink-runtime` depends ONLY on `brink-format` — nothing else from the brink crate family.
2. `brink-lsp` depends on `brink-analyzer`, NOT on `brink-compiler`.
3. `brink-compiler` depends on `brink-format` (writes) and `brink-analyzer`/`brink-hir`/`brink-syntax` (reads).
4. `brink-format` has no brink-internal dependencies.
5. `brink-intl` depends ONLY on `brink-format` (for `PluralCategory` and `PluralResolver` trait).

## Compilation pipeline

The pipeline is organized as a sequence of passes:

```
Pass 1: Parse          (brink-syntax)     per-file       → AST
Pass 2: Lower          (brink-hir)        per-file       → HIR + SymbolManifest + diagnostics
Pass 3: Merge/Resolve  (brink-analyzer)   cross-file     → unified SymbolIndex + diagnostics
Pass 4: Type-check     (brink-analyzer)   cross-file     → type annotations + diagnostics
Pass 5: Validate       (brink-analyzer)   cross-file     → dead code, unused vars, etc.
Pass 6: Codegen        (brink-compiler)   per-container  → bytecode + tables
```

The LSP runs passes 1–5. The compiler runs all 6.

### Pass 1: Parse (brink-syntax) — COMPLETE

- **Input:** `.ink` source text
- **Output:** `Parse` — lossless CST (rowan green/red tree) + `Vec<ParseError>`
- **Properties:**
  - Every byte of source appears in exactly one token (lossless roundtrip)
  - Error recovery via `ERROR` nodes — parser never panics, always produces output
  - ~230 `SyntaxKind` variants (tokens + nodes)
  - Typed AST layer with 140+ zero-cost newtype wrappers over CST nodes
  - Pratt expression parser with 10 precedence levels
  - String interpolation with nesting depth tracking

Covers all ink constructs: knots, stitches, choices, gathers, diverts, tunnels, threads, variables, lists, externals, inline logic, sequences, tags, content extensions markup.

### Pass 2: Lower (brink-hir)

- **Input:** `ast::SourceFile` from brink-syntax
- **Output:** `(HirTree, SymbolManifest, Vec<Diagnostic>)`
- **Responsibilities:**
  - Weave folding: flat choices/gathers (identified by bullet/dash count) → container tree with proper nesting and scope. Must recurse into conditional blocks to find choices that participate in the outer weave (see "Choices inside conditional blocks" under Ink semantics).
  - Implicit structure: top-level content before first knot → root container
  - INCLUDE content merging: top-level content from included files is inlined at the INCLUDE location; knots are separated and appended to the story. Note: the actual merge is cross-file and happens in brink-analyzer; brink-hir records INCLUDE sites and exports `fold_weave` which the analyzer calls on the merged content.
  - First-stitch auto-enter: the first stitch in a knot is entered via implicit divert; other stitches require explicit diverts
  - Strip trivia and syntactic sugar
  - Collect declarations into symbol manifest (knots, stitches, variables, lists, externals, unresolved references)
  - Emit structural diagnostics (malformed weave nesting, orphaned gathers, etc.)
- **Scope:** Per-file. Does not require cross-file context.

### Pass 3–5: Analyze (brink-analyzer)

- **Input:** `Vec<(FileId, HirTree, SymbolManifest)>` from all files
- **Output:** `(SymbolIndex, Vec<Diagnostic>)`
- **Responsibilities:**
  - Merge per-file symbol manifests into a unified symbol table
  - Resolve INCLUDE file graph
  - Name resolution: paths → concrete symbols (DefinitionIds)
  - Scope analysis: temp is function-scoped, VAR/CONST are global
  - Type checking: expression types, assignment compatibility
  - Validation: undefined targets, duplicate declarations, dead code, unused variables
  - Circular include detection

The analyzer also owns the **project database** — the stateful, long-lived cache of parsed trees and analysis results. Both the compiler and LSP interact with this:

- **Compiler:** creates a project database, loads all files, runs passes 1–5, feeds results to codegen
- **LSP:** holds a long-lived project database, updates incrementally on file edits, serves queries against cached results

### Pass 6: Codegen (brink-compiler)

- **Input:** HIR trees + resolved `SymbolIndex`
- **Output:** bag of `ContainerBytecode` blobs (each with its line sub-table) + metadata (written to `.inkb`)
- **Responsibilities:**
  - Per-container bytecode emission
  - Expression lowering → stack ops + jumps
  - Choice lowering → choice point opcodes (see [Choice text decomposition](#choice-text-decomposition))
  - Sequence lowering → sequence opcodes
  - Divert/tunnel/thread lowering → control flow opcodes
  - Implicit diverts: end-of-root-story gets implicit gather + `-> DONE`
  - Text decomposition: static text blocks → line templates with slot placeholders (see [Text decomposition](#text-decomposition))
  - Per-container line table building (each line entry has content + source text content hash)
  - All cross-definition references use `DefinitionId` — no resolved indices in the output

## Definitions and DefinitionId

All named things in the format — containers, global variables, list definitions, list items, and external functions — use a single `DefinitionId(u64)` type. The high 8 bits are a type tag identifying which table the definition belongs to; the low 56 bits are a hash of the fully qualified name/path.

```
DefinitionId (u64):
┌──────────┬──────────────────────────────────────────────────┐
│ tag (8)  │                  hash (56)                       │
└──────────┴──────────────────────────────────────────────────┘
```

The linker resolves all `DefinitionId` references uniformly to compact runtime indices. The runtime never sees `DefinitionId` on the hot path — they're resolved at link time. Persistent state (save files, visit counts) stores `DefinitionId` for stability across recompilation.

### Definition tags

| Tag | Kind | Payload |
|-----|------|---------|
| `0x01` | Container | Bytecode blob, content hash, counting flags |
| `0x02` | Global variable | Name, value type, default value, mutable flag |
| `0x03` | List definition | Name, items (name + ordinal each) |
| `0x04` | List item | Origin list `DefinitionId`, ordinal |
| `0x05` | External function | Name, arg count, optional fallback `DefinitionId` |

### Containers (tag `0x01`)

Containers are the fundamental compilation and runtime unit, analogous to functions in a normal programming language. At the source level, ink has knots, stitches, gathers, and labeled choice targets. At the bytecode level, these are all **containers** — there is no distinction. This matches the reference ink runtime, which has a single `Container` type.

Each container definition has:

- **`DefinitionId`** — `0x01` tag + hash of fully qualified path (e.g., `hash("my_knot.my_stitch")`). Stable across recompilation as long as the path doesn't change.
- **Bytecode** — its own instruction stream
- **Content hash** — fingerprint of the bytecode, used during hot-reload to detect whether a container's implementation changed
- **Counting flags** (bitmask):
  - Bit 0: `visits_should_be_counted` — track visit count
  - Bit 1: `turn_index_should_be_counted` — record which turn it was visited on
  - Bit 2: `counting_at_start_only` — only count when entering at the start, not when re-entering mid-container

#### Container hierarchy

```
Root container
├── [top-level content]
├── Knot A (container)
│   ├── [knot content before first stitch]
│   ├── Stitch X (container)
│   │   ├── [stitch content]
│   │   └── Gather (container, may be labeled)
│   └── Stitch Y (container)
└── Knot B (container)
```

- The first stitch in a knot is auto-entered via an implicit divert. Other stitches require explicit `-> stitch_name`.
- Stitches do NOT fall through to each other.
- The root story container gets an implicit final gather + `-> DONE` appended by the compiler.

#### Execution model: call frames and container positions

The VM distinguishes two kinds of entry into a container:

- **Flow entry** — moving into a child container (stitch, gather, choice branch). Pushes a container position onto the current call frame's position stack. Does NOT create a new call frame. The child shares the parent's temp variable slots.
- **Call entry** — function call or tunnel. Pushes a new call frame with a fresh position stack and fresh temp slots. The callee cannot access the caller's temps.

Each call frame contains:

```
CallFrame {
    frame_type: CallFrameType,
    return_address: (DefinitionId, offset),
    temps: Vec<Value>,                        // frame-local temp variable slots
    container_stack: Vec<(DefinitionId, offset)>,  // flow positions within this call
}
```

#### Call frame types

| Type | Created by | Behavior |
|------|-----------|----------|
| `Root` | Story startup | Main flow entry. Empty → yield Done. |
| `Function` | `Call` opcode | Output captured as return value. Empty → pop frame, push captured text. |
| `Tunnel` | `TunnelCall` opcode | Non-capturing call. Yields for pending choices. |
| `Thread` | `ThreadCall` opcode | Boundary frame — thread won't unwind into inherited frames below. |
| `External` | `CallExternal` opcode | Marker frame for pending external function resolution. Contains no bytecode — the frame exists to track state on the call stack. See [External function handling](#external-function-handling). |

The "current container" is always the top of `call_stack.top().container_stack`. Finishing a container (reaching end of its bytecode) pops from the container stack and resumes the parent. Returning from a function/tunnel pops the entire call frame. Diverts replace the current container position. Threads fork the entire call stack (call frames + their container stacks).

### Global variables (tag `0x02`)

Each variable definition has:

- **`DefinitionId`** — `0x02` tag + hash of variable name
- **Name** — `NameId` (for debugging/inspection and host binding)
- **Value type** — the type of the default value
- **Default value** — `Value` (same type as the VM stack)
- **Mutable** — `bool` (`true` for `VAR`, `false` for `CONST`)

`VAR` declarations are mutable globals. `CONST` declarations are immutable globals — they always exist in the format (visible, inspectable, debuggable). The compiler may inline CONST values as a build-time optimization controlled by a compiler flag, but the definition is always present. Attempting to `SetGlobal` on an immutable variable is a runtime error.

Temporary variables (`temp`) have no format-level definition. They are call-frame-local — created by a `DeclareTemp` opcode during execution, stored in the current call frame's temp slot array, and discarded when the frame pops. Temp slot indices are assigned by the compiler/converter across the entire knot/function scope (including all child containers reached by flow entry), not per-container.

#### Bytecode instructions for variables

```
GetGlobal(DefinitionId)     // push global variable value
SetGlobal(DefinitionId)     // pop stack → assign to global (runtime error if immutable)
DeclareTemp(u16)            // declare temp at local slot index in current frame
GetTemp(u16)                // push temp value from frame slot
SetTemp(u16)                // pop stack → assign to frame slot
```

Globals use `DefinitionId` (resolved by linker to fast runtime index). Temps use call-frame-local slot indices assigned by the compiler across the entire knot/function scope — no `DefinitionId`, no linker involvement. Child containers reached by flow entry share the parent's call frame and use the same slot namespace.

### List definitions (tag `0x03`)

Each list definition has:

- **`DefinitionId`** — `0x03` tag + hash of list name
- **Name** — `NameId`
- **Items** — `Vec<(NameId, i32)>` (item name + ordinal)

Ordinals can be non-contiguous and negative (e.g., `LIST foo = (Z = -1), (A = 2), (B = 3), (C = 5)`). The linker builds efficient runtime representations (bitset mappings, lookup tables) from this.

### List items (tag `0x04`)

Each list item is an independent definition, because bare item names are implicitly global in ink — `happy` resolves to a single-element list value `{Emotion.happy: 1}`.

- **`DefinitionId`** — `0x04` tag + hash of qualified name (e.g., `hash("Emotion.happy")`)
- **Origin** — `DefinitionId` of the parent list definition
- **Ordinal** — `i32`

#### List values

A list value (for variable defaults and as literals in bytecode) is a set of items, potentially from multiple origin definitions:

```
ListValue {
    items: Vec<DefinitionId>      // list item DefinitionIds that are "set"
    origins: Vec<DefinitionId>    // list definition DefinitionIds (for typed empties)
}
```

The `origins` field preserves type information for empty lists — needed for `LIST_ALL` and `LIST_INVERT` to know the full universe of possible items.

### External functions (tag `0x05`)

Each external function definition has:

- **`DefinitionId`** — `0x05` tag + hash of function name
- **Name** — `NameId`
- **Arg count** — `u8`
- **Fallback** — `Option<DefinitionId>` pointing to a container (tag `0x01`) with the ink-defined fallback body

External function resolution is a **runtime** concern, not a link-time concern. The linker indexes external definitions (assigns runtime indices, builds lookup tables) but does not resolve them to host bindings or fallbacks. Resolution happens per-flow at execution time — see [External function handling](#external-function-handling). The separate tag gives better diagnostics and makes externals visually distinct in `.inkt` debug output.

### What is NOT a definition

- **Temporary variables** — stack-frame-local, created/destroyed per execution. No `DefinitionId`.
- **Names** — internal interned strings (variable names, list names, debug labels). Indexed by `NameId(u16)`. Not localizable.
- **Lines** — text output content, scoped to containers. Identified by `LineId = (DefinitionId, u16)` — the container's DefinitionId + a local index within that container. Each line carries its content (plain text or template) and a content hash of the source text for locale change tracking.

## Bytecode VM

The runtime is a stack-based bytecode VM.

### Design properties

- Stack-based: operands on value stack
- Jump offsets within a container are container-relative
- Cross-definition references use `DefinitionId` in the file format, resolved to compact runtime indices at load time
- Short-circuit `and`/`or` handled by compiler (emits conditional jumps), not VM

### Value type

```
Int(i32) | Float(f32) | Bool(bool) | String | List | DivertTarget | Null
```

`DivertTarget` holds a `DefinitionId` pointing to a container — used for variable divert targets (`VAR x = -> some_knot`).

### Opcode categories

The instruction set covers:

- **Stack & literals:** push int/float/bool/string/list/divert-target/null, pop, duplicate
- **Arithmetic:** add (including string concat), sub, mul, div, mod, negate
- **Comparison & logic:** equal, not-equal, greater, less, etc., not, and, or
- **Global variables:** get global (`DefinitionId`), set global (`DefinitionId`)
- **Temp variables:** declare temp (slot), get temp (slot), set temp (slot)
- **Control flow:** jump, conditional jump, divert (`DefinitionId` — goto, replaces current position), conditional divert, variable divert
- **Container flow:** enter container (`DefinitionId` — push position, resume at caller when child ends), exit container (pop position)
- **Functions & tunnels:** call (push call frame + enter), return (pop call frame), tunnel call, tunnel return
- **Threads:** thread start (fork entire call stack), thread done
- **Output:** emit line (`u16` local line index), eval line (`u16` local line index — same as emit line but pushes to value stack instead of output buffer), emit value (stringify + emit top of stack), emit newline, glue, emit tag
- **Choices:** begin/end choice set, begin/end choice (`ChoiceFlags` + `DefinitionId` target), choice output (`u16` local line index)
- **Sequences:** sequence (with kind: cycle/stopping/once-only/shuffle), sequence branch
- **Intrinsics:** visit count, turns since, turn index, choice count, random, seed random
- **External functions:** call external (`DefinitionId` + arg count)
- **List operations:** contains, not-contains, intersect, union, except, all, invert, count, min, max, value, range, list-from-int, random
- **Lifecycle:** done (pause, can resume), end (permanent finish)
- **Debug:** source location mapping (strippable)

The exact opcode encoding is defined in `brink-format`.

## Format (brink-format)

`brink-format` defines the binary interface between compiler and runtime. It is the ONLY dependency of `brink-runtime`.

### Contents

- `DefinitionId(u64)` — tagged definition identity type (8-bit type tag + 56-bit name hash)
- `NameId(u16)` — index into the name table (internal strings, not localizable)
- `LineId = (DefinitionId, u16)` — container-scoped line identity (all user-visible text output)
- Opcode definitions and encoding
- Definition payloads for each tag type (container, variable, list def, list item, external fn)
- `Value` type and encoding (int, float, bool, string, list, divert target, null)
- Line template types: `LineTemplate`, `LinePart`, `SelectKey`, `PluralCategory`
- `PluralResolver` trait (implemented by host or `brink-intl`)
- Serialization/deserialization for `.inkb`, `.inkl`, and `.inkt`

### File formats

- **`.inkb`** — binary format. Definition tables (containers with per-container line sub-tables, variables, lists, externals), name table, and metadata. All cross-definition references are symbolic (`DefinitionId`). No resolved indices.
- **`.inkl`** — locale overlay. Per-container replacement line tables and audio mappings for a specific locale. Keyed by `LineId = (DefinitionId, u16)` for stability across recompilation.
- **`.inkt`** — textual format. Human-readable representation of the bytecode, like WAT is to WASM. Container paths as labels, opcodes as mnemonics. For debugging, inspection, and diffing.

### `.inkb` sections

- Header (magic, format version, section offsets, checksum)
- Container section (per container: `DefinitionId` + bytecode blob + content hash + counting flags + line sub-table)
  - Line sub-table: per line entry: content (plain text or `LineTemplate`) + source text content hash
- Variable section (`DefinitionId` + `NameId` + type + default + mutable per entry)
- List definition section (`DefinitionId` + `NameId` + items per entry)
- List item section (`DefinitionId` + origin + ordinal per entry)
- External function section (`DefinitionId` + `NameId` + arg count + optional fallback per entry)
- Name table (`NameId` → text, for internal strings: definition names, debug labels)
- Debug info (strippable, source maps)

### `.inkl` sections

- Header: magic `b"INKL"`, format version, BCP 47 locale tag, base `.inkb` checksum (must match)
- Per-container line tables (keyed by container `DefinitionId`, each entry: local line index → localized content)
- Audio table (`LineId` → audio asset reference)

### Line template types

```
LineTemplate = Vec<LinePart>

enum LinePart {
    Literal(String),
    Slot(u8),
    Select {
        slot: u8,
        variants: Vec<(SelectKey, Vec<LinePart>)>,
        default: Vec<LinePart>,
    },
}

enum SelectKey {
    Cardinal(PluralCategory),    // CLDR cardinal: zero, one, two, few, many, other
    Ordinal(PluralCategory),     // CLDR ordinal: zero, one, two, few, many, other
    Exact(i32),                  // exact numeric match
    Keyword(String),             // for gender, custom categories
}

enum PluralCategory { Zero, One, Two, Few, Many, Other }
```

A line's content is either plain text (a single `Literal`) or a `LineTemplate` with slots and selectors. The runtime's line resolver walks the `LinePart` tree, reads slot values from the VM stack, picks select variants (using the `PluralResolver` trait for plural categories), and appends formatted text to the output buffer.

### Plural resolution

The runtime defines a `PluralResolver` trait:

```
trait PluralResolver {
    fn cardinal(&self, number: i64, fraction: Option<&str>) -> PluralCategory;
    fn ordinal(&self, number: i64) -> PluralCategory;
}
```

The runtime ships no locale data. Consumers provide a resolver via:

- **`brink-intl`** — batteries-included crate backed by ICU4X baked data, pruned at build time to only the locales the consumer specifies.
- **Custom implementation** — game engines with their own i18n system implement the trait directly.
- **No resolver** — stories without localization don't need one. Fallback: everything maps to `Other`.

## Runtime (brink-runtime)

### Core requirements

- **Bytecode VM:** stack-based execution of compiled stories
- **Multi-instance:** one linked program (immutable, shareable), many story instances with isolated per-instance state
- **Hot-reload:** safe recompilation without invalidating running state
- **Deterministic RNG:** per-instance seed/state for reproducible shuffle sequences

### Two-layer architecture

The runtime maintains two layers:

- **Unlinked layer:** the raw definition tables with symbolic `DefinitionId` references. This is the source of truth, populated from `.inkb`.
- **Linked layer:** the resolved `Program` with fast internal indices. Built by the linker step.

Loading, hot-reload, and patching all flow through the same linker step:

1. **Normal startup:** load `.inkb` → optionally overlay `.inkl` → populate unlinked layer → link → run
2. **Hot-reload (full):** replace entire unlinked layer → re-link → reconcile instances
3. **Hot-reload (patch):** update changed definitions in unlinked layer → re-link → reconcile instances
4. **Locale switch:** swap per-container line tables + audio table from a different `.inkl` → re-link

### Linker step

The linker reads all definitions from the unlinked layer and:

1. For each `DefinitionId`, reads the tag and dispatches to the appropriate table
2. Assigns each definition a fast runtime index within its table
3. Builds resolution tables: `DefinitionId → runtime index` (one per tag type)
4. Resolves all `DefinitionId` references in bytecode to runtime indices
5. Indexes external function definitions (assigns runtime indices, builds name lookup tables). Resolution to host bindings or ink fallbacks is a runtime concern, not a link-time concern.
6. Initializes global variables from their default values
7. Builds per-container line tables, name table, and other content structures
8. Produces an immutable, shareable `Program`

One codepath processes all definition types uniformly. The tag determines which table, but the resolution mechanism is the same.

### Three-part state model

Runtime state is split into three parts with distinct ownership and lifecycle:

#### Flow (isolated execution state)

A Flow is a fully isolated execution context — analogous to a separate conversation or narrative track. Each flow owns:

- **Threads / call stack** — `Vec<Thread>`, each thread containing a `Vec<CallFrame>` (return address, temp slots, container position stack). The current position is `call_stack.top().container_stack.top()`.
- **Value stack** — operand stack for bytecode evaluation
- **Output buffer** — accumulated text with per-line structure and tag association (see [Output buffer](#output-buffer))
- **Pending choices** — choices awaiting player selection, each with a thread fork snapshot
- **Tag state** — current tags, in-tag flag

All persistent positions within a flow use `(DefinitionId, offset)` for recompilation stability.

#### Context (game state / save state)

A Context holds the narrative and game state that is meaningful to save, load, and synchronize:

- **Globals** — global variable values
- **Visit counts** — per container `DefinitionId`
- **Turn counts** — which turn each container was last visited
- **Turn index** — current turn number
- **RNG seed + state** — for deterministic randomness

Context is the natural serialization boundary — saving a story means serializing its Context (plus Flow state for mid-passage saves). Contexts can be cloned for speculative execution ("what happens if the player picks choice 2?") and diffed to see what changed.

#### Program (immutable, shared)

The linked program — containers, bytecode, line tables, definitions, name table. Loaded once, shared across all story instances and flows. Never mutated after linking.

### Execution model

The execution model is layered: a dumb per-instruction VM at the bottom, with progressively higher-level APIs built on top. Each layer adds intelligence; lower layers know nothing about the layers above.

#### Layer 0: VM step (per-instruction)

The VM processes a single bytecode instruction and reports what happened:

```
vm::step(flow: &mut Flow, context: &mut Context, program: &Program) -> Result<Stepped>
```

The VM is maximally dumb — it decodes one opcode, executes it, and returns. It does not loop, does not make decisions about when to stop, and does not know about lines, passages, or the Story.

```
enum Stepped {
    Continue,                                  // opcode executed, nothing special
    ExternalCall,                                      // hit external fn — name, args, fallback all on the External frame
    ThreadCompleted,                           // current thread exhausted, switched to next
    Done,                                      // hit Done opcode
    Ended,                                     // hit End opcode
}
```

All runtime errors (type errors, stack underflow, unresolved external calls, etc.) are returned via `Result::Err(RuntimeError)`. Error variants should be detailed enough for the caller to decide recoverability — e.g., a type error includes the types involved, an unresolved external includes the function name. If the VM is in an unrecoverable state, subsequent `step` calls will continue returning the same error; it is the caller's responsibility to detect this and stop.

When the VM yields `ExternalCall`, it has popped the arguments from the value stack and pushed an `External` call frame. The caller must resolve the external call before the next `step` — see [External function handling](#external-function-handling).

#### Layer 1: Line-level continuation

Loops the VM step until a complete line of dialogue is ready:

- Detects newline boundaries in the output buffer (a newline followed by non-glue content confirms the line)
- Handles glue lookahead (a newline is tentative until the next step confirms it wasn't consumed by glue)
- Resolves external function calls via registered handlers
- Returns one line of text with its associated tags

This is equivalent to the reference ink runtime's `Continue()`.

#### Layer 2: Passage-level continuation

Loops line-level continuation until a yield point (choices available, done, or ended). Returns all accumulated lines. This is equivalent to the reference ink runtime's `ContinueMaximally()` and the current behavior of `step()`.

#### Layer 3: Story orchestrator

The `Story` manages one or more flows and their contexts, providing the convenient public API:

- **Single-flow** (common case): one flow, one context. API behaves like `ContinueMaximally` — step and get text + choices.
- **External function binding**: register handlers at the Story level. Line/passage-level continuation resolves external calls transparently.
- **Choice selection**: `choose(index)` is a flow-level operation — restores the thread fork, sets execution position, clears pending choices.

#### Flows and instancing

Every flow in the Story is a named **(Flow, Context) pair**. Multi-flow and instanced flows are the same primitive — the difference is usage pattern, not mechanism.

- **Named flows**: the Story manages a collection of named (Flow, Context) pairs. The "default" flow is just the one created at startup. Additional flows can be created with their own entry points and contexts.
- **Instanced flows**: multiple (Flow, Context) pairs can share the same scene template (entry point in the Program). Each instance has a unique identity (e.g., `"shopkeeper:npc_42"`) and fully independent state.

**Variable scoping for instances** uses explicit registration. When setting up an instance template, the game developer declares which globals are **shared** (readable/writable across all instances, backed by a common store). Everything else in the Context is **per-instance by default** — visit counts, turn counts, turn index, RNG, and all unregistered globals get their own copy per instance. The VM sees a flat key-value store; the backing store handles the shared/instance split transparently.

**Lifecycle, persistence, and synchronization** are Story-layer or engine/caller-layer concerns — the Flow and VM know nothing about them. The Story (or the engine above it) decides when to spawn or destroy instances, how to serialize their contexts, and whether/how to propagate state between flows. The primitives (named (Flow, Context) pairs, explicit shared-global registration) are designed to support a range of policies without prescribing one.

Consumers who need maximum control can bypass the Story and work directly with flows, contexts, and `vm::step`. The Story is a convenience layer that does not sacrifice performance or control.

### External function handling

External function resolution uses the call stack itself as the state machine. The `External` call frame type (see [Call frame types](#call-frame-types)) tracks pending external calls with no separate state flags.

#### VM behavior

When the VM hits `CallExternal(fn_id, argc)`:

1. Pop `argc` args from the value stack
2. Push an `External` call frame — return address = current position, the external function's `DefinitionId` for fallback lookup, and the popped args stored in the frame's `temps` (frame-local storage)
3. Yield `Stepped::ExternalCall`

Everything about the pending call — name, args, fallback `DefinitionId` — lives on the `External` frame. The `Stepped` variant is a pure signal with no payload. The caller inspects the flow to get what it needs (e.g., `flow.external_name(program)`, `flow.external_args()`). This keeps the yield minimal and ensures all call state survives serialization, debugging, and save/load.

If `step` is called while an `External` frame is on top of the call stack (i.e., the caller forgot to resolve it), the VM returns `Err(RuntimeError::UnresolvedExternalCall)`. The caller can inspect the frame for details.

#### Caller resolution

The caller resolves the external call via methods on the Flow:

- **Provide a result**: `flow.resolve_external(value)` — pops the `External` frame and pushes the return value onto the value stack. For fire-and-forget calls (e.g., `~ play_sound(...)`), the caller provides `Value::Null` and the next opcode (`Pop`) discards it.
- **Invoke the ink fallback**: `flow.invoke_fallback(program)` — replaces the `External` frame with a `Function` frame pointing at the ink-defined fallback container. The next `step` call executes the fallback using the existing function call machinery (output capture, return value, frame pop). No special-case VM code needed.

#### Story-level integration

Higher-level APIs (line/passage continuation) resolve external calls transparently via registered handlers:

```
story.bind_external("play_sound", |args| Ok(Value::Null));
story.bind_external("get_health", |args| Ok(Value::Int(player.health)));
```

When `continue_line` encounters `Stepped::ExternalCall`:

1. Read name from `flow.external_name(program)`, look up handler
2. **Handler found**: read args from `flow.external_args()`, call handler, use `flow.resolve_external(result)`, continue stepping
3. **No handler, fallback exists**: use `flow.invoke_fallback(program)`, continue stepping
4. **Neither**: return error to caller

External handler functions are fallible (`-> Result<Value, E>`) — a handler that errors propagates through the continuation layer to the caller. The error should be descriptive (handler name, args, underlying cause) so the caller can decide how to respond.

### Output buffer

The output buffer lives in each Flow and tracks per-line structure as content is emitted:

- The VM calls `push_text`, `push_newline`, `push_glue`, `begin_tag`/`end_tag` — it does not know about lines.
- The buffer internally groups text and tags into line segments, separated by newline boundaries.
- Glue resolution collapses line boundaries (a glue followed by a newline merges the adjacent lines).
- Callers query the buffer for structured output: completed lines with their associated tags, whether a partial line is in progress, etc.

This design solves per-line tag association (e.g., the i18n test case where tags must attach to the line they follow, not the entire passage) without adding complexity to the VM.

### Choice forks

When a choice is created (`BeginChoice`), the VM captures a **thread fork** — a snapshot of the current thread's call stack. This fork is stored on the `PendingChoice` and restored when the player selects that choice.

Choice forks capture **Flow state only** (call stack, temps). The Context (globals, visit counts) is **not** captured or rolled back — modifications to globals between fork creation and choice selection remain visible. This matches the reference ink runtime's behavior.

**Guiding invariant:** the multi-flow model must produce identical results to a single-flow story when only one flow is present. Choice forks are a Flow-local operation and do not interact with Context synchronization.

### Hot-reload reconciliation

All persistent references in story instances use `DefinitionId`, not runtime indices. When a new program is linked, the reconciliation is a single pass over the old and new definition sets by `DefinitionId`, regardless of type:

1. For each running instance, check every `(DefinitionId, offset)` position across all call frames and their container position stacks:
   - **Container exists, content hash unchanged** → position is valid, do nothing
   - **Container exists, content hash changed** → reset offset to 0 (container entry)
   - **Container gone** → fall back up the container/call stack to deepest valid position, or reset to entry point
3. Detect renames via content hashing (removed container with same content hash as added container = rename)
4. Visit counts keyed by container `DefinitionId` — retain for containers that still exist, orphan the rest
5. Sequence states keyed by `(DefinitionId, sequence_index)` — invalidate if content hash changed
6. Pending choices — invalidate (the choice set may no longer exist)
7. Reconcile variables: diff old and new variable definitions by `DefinitionId` (keep existing, add new with defaults, flag removed/type-changed)
8. Reconcile list definitions: new items are added, removed items are orphaned
9. Return a `ReconcileReport` with warnings for editor integration

### Multi-instance management

A `NarrativeRuntime` (or equivalent) host interface manages:

- Loading/unloading programs (via the linker step)
- Spawning/destroying story instances (each with its own flows and contexts)
- Stepping instances and collecting results
- Registering external function handlers
- Hot-reloading programs and reconciling instances
- Save/load for contexts and flow state

## Localization

Brink separates executable logic from localizable text. The bytecode is locale-independent — all user-visible text is referenced via `LineId = (DefinitionId, u16)`, a container-scoped local index into the container's line sub-table. Locale-specific content lives in `.inkl` overlay files that replace line content per container.

### Text decomposition

During codegen, the compiler decomposes text into line entries in the container's line sub-table:

- **Plain text** (no interpolation, no inline logic) → a line with a single `Literal`, emitted via `EmitLine(u16)`.
- **Interpolated or structured text** (contains `{variables}`, inline conditionals, or inline sequences) → a line with a `LineTemplate`, emitted via `EmitLine(u16)`. The compiler pushes slot values onto the stack before the emit.

The `u16` is the local line index within the current container. The runtime resolves this to the container's line sub-table entry.

Example: `I found {num_gems} {num_gems > 1: gems | gem} in the {cave_name}.` compiles to:

```
GetLocal(num_gems)          // push slot 0
GetLocal(cave_name)         // push slot 1
EmitLine(2)                 // format line 2's template with 2 slots from stack
```

Line sub-table entry 2:

```
I found {0} {0 -> one: gem | other: gems} in the {1}.
```

The plural logic lives in the line template, not the bytecode. Translators can restructure sentences, reorder slots, and alter plural/gender forms per locale without touching the compiled program.

### Scope of text decomposition

The compiler can only build message templates for **static text blocks** — contiguous text where the full structure is visible at compile time within a single expression or line.

**Can be one line:**

- A single line with interpolation: `Hello, {name}!`
- A single line with inline conditionals: `{flag: yes|no}`
- A single line with inline sequences: `{a|b|c}` (sequence index becomes a slot)
- Statically glued lines (both sides are literals or simple interpolations)
- Choice display / choice output text

**Each fragment is its own line (cannot be merged):**

- Text across container boundaries (diverts, tunnels, function calls, threads)
- Text in dynamically bounded loops
- Text produced by external function calls

The boundary rule: if it crosses a container call, each side is independent.

### Choice text decomposition

Ink's bracket syntax splits choice text into three roles:

```
* Pick up the sword[.] You grab the sword.
```

- Before `[` → appears in both the choice list and the output
- Inside `[...]` → appears only in the choice list
- After `]` → appears only after selection

This three-part split is a source-language authoring convenience. For localization, the compiler decomposes each choice into **two independent lines**:

- **Choice display** — the complete text shown in the choice list (before + inside bracket)
- **Choice output** — the complete text emitted after selection (before + after bracket)

`BeginChoice(flags, target)` always pops the display text from the value stack. The choice target (`DefinitionId`) is encoded directly in the opcode — no separate divert instruction. Two bytecode patterns are supported:

**High-level (static/templated text):** The compiler resolves bracket syntax at compile time and stores both texts as line table entries. `EvalLine` reads a line and pushes it as a String to the value stack (same as `EmitLine` but targeting the value stack instead of the output buffer). `ChoiceOutput` stores a line table reference on the pending choice for emission when the player selects it.

```
EvalLine(5)                   // push display text from line table
BeginChoice(flags, target)    // pop display text, register choice
  ChoiceOutput(6)             // output text line reference
EndChoice
```

Translators localize each line independently with no structural coupling. This allows the target language to use completely different grammatical constructions for the choice prompt vs. the narrative output.

**Low-level (dynamic text):** When choice text contains arbitrary logic that cannot be statically decomposed into a line table entry, `BeginStringEval`/`EndStringEval` captures evaluated text as a String and pushes it to the value stack. The choice target container handles output text directly. This path is also used by the ink.json converter, which does not have access to the original bracket syntax.

```
BeginStringEval
  EnterContainer(choice_text)   // arbitrary code that emits text
EndStringEval                   // capture text, push String to value stack
BeginChoice(flags, target)      // pop display text, register choice
EndChoice                       // no ChoiceOutput — target handles output
```

Both patterns are first-class. `BeginChoice` is agnostic to how the display text was produced — it always pops one String from the value stack.

### Voice acting

Every text emission has a stable `LineId = (DefinitionId, u16)` — its container identity plus local line index. This is the same identity used for localization, so voice acting and text localization share a single addressing scheme.

Audio asset references live in the `.inkl` audio table, keyed by `LineId`. Authors can associate explicit recording IDs with lines via tags (`#voice:blacksmith_greeting_01`).

The runtime's text output includes `LineId` so the host can look up audio:

```
struct TextOutput {
    text: String,
    line_id: LineId,
    audio_ref: Option<AudioRef>,
}
```

The host handles playback — brink provides the mapping.

### Locale overlay loading

At runtime, loading a `.inkl` overlay replaces per-container line content and adds the audio table. The bytecode is unchanged — `EmitLine(2)` still references local line index 2, but the content behind that index is now in the target locale. Since lines are scoped to containers, only containers present in the `.inkl` have their lines replaced; others retain the base locale content.

The `.inkl` header includes the base `.inkb` checksum. The runtime validates this on load — a mismatched `.inkl` (compiled against a different `.inkb` version) is rejected.

### Localization authoring (XLIFF)

Localization source files use **XLIFF 2.0** — one file per locale (e.g., `translations/ja-JP.xlf`). Containers are represented as `<file>` elements within the XLIFF document. Brink-specific metadata (content hashes, audio asset references) uses XLIFF's custom namespace extension mechanism.

Workflow:

1. **Generate:** `brink-cli generate-locale` reads a compiled `.inkb` and produces an XLIFF file with all translatable lines (organized by container), including context annotations for translators.
2. **Translate:** Translators work in the XLIFF file directly or import it into a translation management platform (Lokalise, Crowdin, etc.). Audio asset references are added to the XLIFF via the `brink:audio` extension attribute. Translation state tracking uses XLIFF's built-in `state` attribute (`initial`/`translated`/`reviewed`/`final`).
3. **Compile:** `brink-cli compile-locale` reads the translated XLIFF and produces a binary `.inkl` overlay.
4. **Regenerate (on source changes):** `brink-cli generate-locale` diffs the new `.inkb` against the existing XLIFF by `LineId`, preserving human-edited fields (translations, audio refs), updating machine-managed fields (original text, context), and using the source text content hash to detect changed lines and reset their review status.

XLIFF was chosen because every major translation management platform natively imports/exports it, and the spec requires tools to preserve unknown extensions — brink-specific metadata survives round-trips through external tooling.

## LSP (brink-lsp)

Thin protocol adapter over `brink-analyzer`. Depends on analyzer, NOT on compiler.

Planned features:

- Diagnostics (streamed on every change)
- Go to definition (via SymbolIndex position lookup)
- Find references
- Rename (find references → workspace edit)
- Hover (symbol type, doc comment, usage count)
- Autocomplete (knot/stitch names at diverts, globals, local vars)
- Semantic tokens
- Document/workspace symbols
- Signature help (external function parameters)

## Implementation order

### Vertical spike

The first implementation milestone is a vertical spike: a thin slice through every crate that runs a trivial ink story end-to-end. The spike validates that the crate boundaries and interfaces work together before investing heavily in any single crate.

The spike covers: text output, simple choices, diverts between knots, `-> END`. No weave folding, no variables, no sequences, no cross-file includes.

### Spike deliverables

The spike's real output is **interfaces and tests**, not implementations.

1. **Public API surfaces** — each crate gets its public types and function signatures defined first. These are the stable artifacts that survive rewrites.
2. **Boundary tests** — integration tests at each crate boundary that describe what the pipeline produces for specific inputs (parse snapshots, HIR snapshots, bytecode disassembly, execution output). These are the source of truth.
3. **Minimal implementations** — just enough to make the tests pass. These are explicitly disposable.

### Disposability

Spike implementations are v0. They exist to validate interfaces, not to be the final code. When building out a crate for real, **prefer rewriting over patching** if the existing implementation doesn't match the target design. The tests and public API signatures are what matter; everything behind them is throwaway.

Keep spike implementations tiny — the smaller they are, the less inertia they carry.

### Tiers (post-spike)

- **Tier 1:** Full choice semantics (sticky, once-only, fallback, nesting, conditions), gathers, weave folding, variables (local, temp), arithmetic, conditionals, sequences, glue, tags.
- **Tier 2:** Tunnels, threads, external functions, global variables, visit counts, `TURNS_SINCE`, multi-file (`INCLUDE`).
- **Tier 3:** LIST type (definitions, bitset operations, full set operations).

The analyzer grows with each tier — barely exists during the spike, picks up name resolution in tier 1, and gets the full pass suite by tier 2.

## Deferred

The following are real requirements but deferred to later phases:

- **Step budgeting** — the per-instruction `vm::step` naturally supports cooperative scheduling (the caller controls the loop), but a convenience API like `continue_line_with_budget(n)` that limits the number of instructions per call may be useful for frame-budgeted game loops. Deferred until the layered API is stable.
- **JSON codegen backend** — inklecate-compatible `.ink.json` output for conformance testing. Decision on whether to build this is deferred.
- **Content extensions** — pluggable compile-time text transforms (speaker attribution, parentheticals, styled text). Still wanted, but the architectural integration (opcode reservation, structured `ContentOutput` types) needs more thought before committing.
- **Localization implementation** — the localization architecture is specified (see [Localization](#localization)) but implementation is deferred to post-tier-3. Format types (`LineTemplate`, `PluralCategory`, etc.) land with `brink-format`; the line resolver, `.inkl` loading, XLIFF tooling, and `brink-intl` come later.
- **`no_std` runtime** — desirable for WASM targets but not an immediate constraint.

## Ink semantics (reference behavior)

Key semantics verified against the reference C# ink implementation:

- **INCLUDE with top-level content:** top-level content from included files is merged inline at the INCLUDE location. Knots/stitches are separated and appended to the end of the story.
- **Stitch fall-through:** stitches do NOT fall through. Only the first stitch in a knot is auto-entered (via implicit divert). Other stitches require explicit `-> stitch_name`. Stitches with parameters are never auto-entered.
- **Root entry point:** all top-level content becomes an implicit root container. The compiler appends an implicit gather + `-> DONE` so the story terminates gracefully.
- **Visit counting:** per-container granularity. Any container (knot, stitch, gather, choice target) can independently track visits and turn indices. `countingAtStartOnly` prevents overcounting on mid-container re-entry.
- **Gathers:** not first-class objects. Implemented as unnamed containers that choice branches divert to.
- **Choices inside conditional blocks:** choices (`*`) can appear inside `{ - condition: ... }` multiline conditional blocks and participate in the outer weave structure (not a separate scope). Gathers (`-`) are explicitly forbidden inside conditional blocks — the reference compiler errors with "You can't use a gather (the dashes) within the { curly braces } context." This means conditional blocks are transparent to weave folding: they conditionally include/exclude choices at the current depth, but cannot introduce nested weave structure. The parser enforces this at `StatementLevel.InnerBlock`. **Parser gap:** brink-syntax does not yet parse choices inside multiline branch bodies — `multiline_branch_body` has no `STAR` arm and needs to be updated.

## Test corpus

The repository includes a test corpus at `tests/`:

- `tests/tests_github/` — real-world `.ink` files from open-source projects
- 1,115 `.ink` files, 937 golden `.ink.json` files
- Used for parser smoke tests (zero panics), lossless roundtrip validation, and future conformance testing

Fuzz testing and property-based testing infrastructure exists in `brink-syntax`.
