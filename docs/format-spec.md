# brink format specification

`brink-format` defines the binary interface between compiler and runtime — the types, instruction set, and file formats that bridge compilation and execution. It is the ONLY dependency of `brink-runtime`.

See also: [compiler-spec](compiler-spec.md) (how the compiler produces these types), [runtime-spec](runtime-spec.md) (how the runtime consumes them).

## Versioning

The `.inkb` and `.inkl` formats carry a `(MAGIC, VERSION)` header; the reader **hard-rejects** any version it doesn't recognize (`UnsupportedVersion`). **Every change to the on-the-wire layout bumps `VERSION`.**

Today these are **regenerable build artifacts** — produced from `.ink` on every compile, never decoupled from the compiler that made them — so the policy is deliberately simple: **regenerate on mismatch; no multi-version readers.** Maintaining back-compat parsers for bytes nobody persists would be premature complexity.

**`.inkb` version history:** v2 added `ContainerDef::param_count`; v3 added the `local` scope bit to variables and containers; **v4** added the collection value tags `Array`/`Map` (tree encoding) and froze the reserved Tier-1 value-tag/section/opcode surface in a single planned bump (the §9 one-bump rule of `docs/value-model-spec.md`; RFC `docs/format-v4-rfc.md`). Per that rule, the remaining Tier-1 milestones (function values, handles, projections) add data using encodings already specified at v4 and do **not** bump the version — the writer and reader evolve together within VERSION 4 as each milestone's compiler surface begins emitting the reserved tags. (The optional `Visibility` section, M-2b, also landed on the v4 line — it is omitted entirely when empty, so it needed no bump of its own.) **v5** added the mandatory `AliasTable` section (M-3, `docs/modules-spec.md` §5): unlike `Visibility`, it is always present (possibly empty) and was not part of the v4 RFC's pre-reserved inventory, so its introduction is its own one-bump event. **v6** added the `PART_SPAN` `LinePart` tag (#1716, inline markup spans — `docs/prose-dialect-spec.md` §4.4/§4.5): a `LinePart` tag was never part of the v4 RFC's pre-reserved inventory either (that inventory reserved *value* tags and whole sections, not `LinePart` variants), so — like `AliasTable` — this is its own one-bump event, not a free ride on the `Array`/`Map`-style no-bump precedent. Ruled directly by issue #1716 ("`LinePart::Span` is a v6 payload") and coordinated with #1683, the v6 bump manifest for the full markup-wire batch (spans, element data, universal block id, choice captured environment); this PR lands only the `Span` payload, and `VERSION` 6 stays open to absorb #1683's remaining payloads without a further bump, one batched break rather than a bump per payload. `.inkl` bumped its own header version in lockstep (1 → 2) — see `.inkl` header layout below — since `.inkl` shares the same `encode_line_content`/`decode_line_content` dispatch and an old `.inkl` reader is just as unable to decode `PART_SPAN` as an old `.inkb` reader.

This is distinct from the **save format** (`SAVE_FORMAT_VERSION`, `save.rs`), which *is* durable — written by players and expected to survive runtime updates — and is therefore *tolerant* by design (`LoadReport` reports what it couldn't apply rather than failing). Program metadata such as container layout does not affect save compatibility: saves reference visit counts and variables by `DefinitionId`, not by container byte layout.

**When to revisit:** the single-version policy holds until a compiled artifact is first **shipped or cached decoupled from its compiler** — e.g. a game bundles a prebuilt `.inkb` against an independently-updating runtime, or the studio persists compiled bytes across versions. At that point, prefer making sections **length-framed and append-only (TLV-style)** — new fields always appended, sections self-describing — so an older reader can skip what it doesn't recognize, rather than maintaining N full parsers. (The container section's length-prefixed bytecode is already partway there.) Make that call when the first durable consumer appears, not before.

`.brkt` (the runtime transcript format, `crates/brink-runtime/src/transcript.rs`) is such a durable consumer already — it tolerates old files by design, unlike `.inkb`'s hard-reject-and-regenerate policy above. `docs/brkt-trailing-section-findings.md` traces exactly what `.brkt`'s current positional trailing-section probe would break for the `.inkb` v6 bump's new sections, and catalogs this repo's existing self-describing-layout precedents (the section offset table above, section-local version bytes) as prior art for that gap.

## Definitions and DefinitionId

All named things in the format — addresses (containers + intra-container labels), global variables, list definitions, list items, external functions, and local variables — use a single `DefinitionId(u64)` type. The high 8 bits are a type tag identifying which table the definition belongs to; the low 56 bits are a hash of the fully qualified name/path.

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
| `0x01` | Address | Container `DefinitionId` + byte offset (see [Addresses](#addresses-tag-0x01)) |
| `0x02` | Global variable | Name, value type, default value, mutable flag |
| `0x03` | List definition | Name, items (name + ordinal each) |
| `0x04` | List item | Origin list `DefinitionId`, ordinal, name |
| `0x05` | External function | Name, arg count, optional fallback `DefinitionId` |
| `0x06` | Struct declaration (TM-4b) | Compiler-side only — `brink-analyzer`'s `SymbolIndex` bookkeeping for a `STRUCT` name (duplicate detection, goto-def, resolution). Never serialized to `.inkb`; the runtime-facing shape identity is the separate `ShapeId`/`StructShapes` space (§ StructShapes section, tag `0x0C`), populated once TM-4c's codegen lowers struct constructs. |
| `0x07` | Local variable | Params and temps — scoped to a container, not serialized in bytecode |

## Addresses (tag `0x01`)

Addresses are the unified mechanism for referring to positions in bytecode. An address points to a specific byte offset within a container. There is no separate "Container" or "Label" tag — both are addresses.

Each address definition has:

- **`DefinitionId`** — `0x01` tag + hash of fully qualified path
- **`container_id`** — `DefinitionId` of the container this address lives in
- **`byte_offset`** — `u32` offset within the container's bytecode

A **primary address** has `byte_offset == 0` and `id == container_id` — this is the container's entry point. An **intra-container address** has a non-zero offset and a distinct ID — these are used for labels, gather targets, and other jump destinations within a container.

### Containers

Containers are the fundamental compilation and runtime unit, analogous to functions in a normal programming language. At the source level, ink has knots, stitches, gathers, and labeled choice targets. At the bytecode level, these are all **containers** — there is no distinction. This matches the reference ink runtime, which has a single `Container` type.

Each container has a primary address (tag `0x01`) plus a `ContainerDef` with:

- **Bytecode** — its own instruction stream
- **Content hash** — `u64` fingerprint of the bytecode, used during hot-reload to detect whether a container's implementation changed
- **Counting flags** (bitmask):
  - Bit 0: `VISITS` — track visit count
  - Bit 1: `TURNS` — record which turn it was visited on
  - Bit 2: `COUNT_START_ONLY` — only count when entering at the start, not when re-entering mid-container
- **Path hash** — `i32`, sum of char values from the container's ink path string. Used to seed the RNG for shuffle sequences.
- **Param count** — `u8`, the number of parameters the container declares (a parameterized knot/stitch/function, e.g. `=== call(action, present) ===` has 2; `0` for the vast majority). The prologue binds them with that many leading `DeclareTemp`s. Lets the runtime arity-check a host-directed parameterized entry (`choose_path_string_with_args`) and `call_function`. (Historical: `.inkb` files built by the retired converter reference pipeline left this `0` — inklecate's JSON didn't expose it.)
- **Scope id** — `DefinitionId` of the lexical scope this container belongs to. For knots and stitches, `scope_id == id` (they ARE the scope). For gathers, choice targets, inline sequence wrappers, and other compiler-internal containers, `scope_id` is the enclosing knot or stitch. Used by the linker to associate containers with their scope's line table.

### Container hierarchy

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
- The root story container gets an implicit final gather + `-> DONE` appended by the compiler — once, for the entry file's root weave only. An `INCLUDE`d file's own trailing weave gets none, matching C# ink's `isRootStory` guard, so running off the end of one is still a `RanOutOfContent` fault (issues #1448, #1502).

## Global variables (tag `0x02`)

Each variable definition has:

- **`DefinitionId`** — `0x02` tag + hash of variable name
- **Name** — `NameId` (for debugging/inspection and host binding)
- **Value type** — the type of the default value
- **Default value** — `Value` (same type as the VM stack)
- **Mutable** — `bool` (`true` for `VAR`, `false` for `CONST`)

`VAR` declarations are mutable globals. `CONST` declarations are immutable globals — they always exist in the format (visible, inspectable, debuggable). The compiler may inline CONST values as a build-time optimization controlled by a compiler flag, but the definition is always present. `CONST`-write enforcement is compile-time only: `lir::lower` refuses to lower any write to a `CONST` root (`E187`, issue #2201). The `Mutable` flag above is currently descriptive-only — it is serialized (`brink-format/src/inkb/write.rs`) and printed for inspection (`inkt/write.rs`), but the VM's `SetGlobal` opcode does not read it, so nothing at the runtime layer itself rejects a write to an immutable global's storage cell.

Temporary variables (`temp`) have no format-level definition. They are call-frame-local — created by a `DeclareTemp` opcode during execution, stored in the current call frame's temp slot array, and discarded when the frame pops. Temp slot indices are assigned by the compiler across the entire knot/function scope (including all child containers reached by flow entry), not per-container.

### Bytecode instructions for variables

```
GetGlobal(DefinitionId)      // push global variable value
SetGlobal(DefinitionId)      // pop stack → assign to global (runtime error if immutable)
DeclareTemp(u16)             // declare temp at local slot index in current frame
GetTemp(u16)                 // push temp value (auto-dereferences VariablePointer and TempPointer)
GetTempRaw(u16)              // push raw temp value without dereferencing
SetTemp(u16)                 // pop stack → assign to frame slot (writes through pointers)
PushVarPointer(DefinitionId) // push a VariablePointer referencing a global variable
PushTempPointer(u16)         // push a TempPointer referencing a temp slot in the current frame
```

Globals use `DefinitionId` (resolved by linker to fast runtime index). Temps use call-frame-local slot indices assigned by the compiler across the entire knot/function scope — no `DefinitionId`, no linker involvement. Child containers reached by flow entry share the parent's call frame and use the same slot namespace.

## Local variables (tag `0x07`)

Local variable definitions track params and temps that are scoped to a container. These are not serialized in bytecode — they exist purely in the definition tables for debugging and analysis purposes.

## List definitions (tag `0x03`)

Each list definition has:

- **`DefinitionId`** — `0x03` tag + hash of list name
- **Name** — `NameId`
- **Items** — `Vec<(NameId, i32)>` (item name + ordinal)

Ordinals can be non-contiguous and negative (e.g., `LIST foo = (Z = -1), (A = 2), (B = 3), (C = 5)`). The linker builds efficient runtime representations (bitset mappings, lookup tables) from this.

## List items (tag `0x04`)

Each list item is an independent definition, because bare item names are implicitly global in ink — `happy` resolves to a single-element list value `{Emotion.happy: 1}`.

- **`DefinitionId`** — `0x04` tag + hash of qualified name (e.g., `hash("Emotion.happy")`)
- **Origin** — `DefinitionId` of the parent list definition
- **Ordinal** — `i32`
- **Name** — `NameId`

### List values

A list value (for variable defaults and as literals in bytecode) is a set of items, potentially from multiple origin definitions:

```
ListValue {
    items: Vec<DefinitionId>      // list item DefinitionIds that are "set"
    origins: Vec<DefinitionId>    // list definition DefinitionIds (for typed empties)
}
```

The `origins` field preserves type information for empty lists — needed for `LIST_ALL` and `LIST_INVERT` to know the full universe of possible items.

List literal values are stored in a dedicated list literals table (`.inkb` section `0x09`) and referenced by `PushList(idx)` opcodes.

## External functions (tag `0x05`)

Each external function definition has:

- **`DefinitionId`** — `0x05` tag + hash of function name
- **Name** — `NameId`
- **Arg count** — `u8`
- **Fallback** — `Option<DefinitionId>` pointing to a container (tag `0x01`) with the ink-defined fallback body

External function resolution is a **runtime** concern, not a link-time concern. The linker indexes external definitions (assigns runtime indices, builds lookup tables) but does not resolve them to host bindings or fallbacks. Resolution happens per-flow at execution time — see [runtime-spec: External function handling](runtime-spec.md#external-function-handling). The separate tag gives better diagnostics and makes externals visually distinct in `.inkt` debug output.

## What is NOT a definition

- **Temporary variables** — stack-frame-local, created/destroyed per execution. No `DefinitionId`.
- **Names** — internal interned strings (variable names, list names, debug labels). Indexed by `NameId(u16)`. Not localizable.
- **Lines** — text output content, scoped to lexical scopes (knots/stitches). Identified by `LineId = (DefinitionId, u16)` — the scope's DefinitionId + a local index within that scope. The `DefinitionId` in a `LineId` refers to the lexical scope, not the emitting container. Each line carries its content (plain text or template), a content hash of the source text, optional slot metadata, optional audio ref, and optional source location.

## Bytecode VM

The runtime is a stack-based bytecode VM.

### Design properties

- Stack-based: operands on value stack
- Jump offsets within a container are container-relative
- Cross-definition references use `DefinitionId` in the file format, resolved to compact runtime indices at load time
- Short-circuit `and`/`or` handled by compiler (emits conditional jumps), not VM

### Value type

```
Int(i32) | Float(f32) | Bool(bool) | String(Rc<str>) | List(Rc<ListValue>) | DivertTarget | VariablePointer | TempPointer | Null
```

`String` and `List` are `Rc`-wrapped so that cloning a `Value` is always O(1) — a refcount bump, not a deep copy. This makes call-frame cloning (during `fork_thread`) essentially free.

`DivertTarget` holds a `DefinitionId` pointing to a container — used for variable divert targets (`VAR x = -> some_knot`).

`VariablePointer(DefinitionId)` — a pointer to a global variable, used for `ref` parameters that target globals. The compiler emits `PushVarPointer` to create these.

`TempPointer { slot: u16, frame_depth: u16 }` — a runtime-only pointer to a temp variable in a specific call frame, used for `ref` parameters that target temps. The compiler emits `PushTempPointer(slot)`, and the runtime resolves it to `TempPointer { slot, frame_depth: current_frame }` at execution time. `TempPointer` never appears in `.inkb` files — it exists only on the value stack and in call-frame temp slots during execution.

**Pointer semantics:** When a temp slot holds a `VariablePointer` or `TempPointer`, `SetTemp` writes through to the pointed-to location (global or target frame's temp) and `GetTemp` auto-dereferences to the pointed-to value. `GetTempRaw` pushes the raw value without dereferencing. `PushTempPointer` flattens double-indirection: if the temp already holds a pointer (`VariablePointer` or `TempPointer`), the existing pointer is pushed as-is rather than wrapping it in another `TempPointer`. This ensures nested ref passthrough (e.g., `fn_a(ref x)` calling `fn_b(ref x)`) works correctly.

### Instruction set

The instruction set is organized into categories. Opcode byte values are defined in `brink-format::opcode`.

#### Stack & literals (`0x01`–`0x09`)

| Opcode | Operand | Description |
|--------|---------|-------------|
| `PushInt` | `i32` | Push integer literal |
| `PushFloat` | `f32` | Push float literal |
| `PushBool` | `bool` | Push boolean literal |
| `PushString` | `u16` | Push string by name-table index |
| `PushList` | `u16` | Push list literal by list-literals-table index |
| `PushDivertTarget` | `DefinitionId` | Push a divert target (container address) |
| `PushNull` | — | Push null |
| `Pop` | — | Discard top of stack |
| `Duplicate` | — | Duplicate top of stack |

#### Arithmetic (`0x10`–`0x15`)

| Opcode | Description |
|--------|-------------|
| `Add` | Add (also string concatenation) |
| `Subtract` | Subtract |
| `Multiply` | Multiply |
| `Divide` | Divide |
| `Modulo` | Modulo |
| `Negate` | Unary negate |

#### Comparison (`0x20`–`0x25`)

| Opcode | Description |
|--------|-------------|
| `Equal` | `==` |
| `NotEqual` | `!=` |
| `Greater` | `>` |
| `GreaterOrEqual` | `>=` |
| `Less` | `<` |
| `LessOrEqual` | `<=` |

#### Logic (`0x28`–`0x2A`)

| Opcode | Description |
|--------|-------------|
| `Not` | Logical not |
| `And` | Logical and (note: short-circuit is handled by compiler via `JumpIfFalse`, not by this opcode) |
| `Or` | Logical or (same — short-circuit via compiler jumps) |

#### Global variables (`0x30`–`0x31`)

| Opcode | Operand | Description |
|--------|---------|-------------|
| `GetGlobal` | `DefinitionId` | Push global variable value |
| `SetGlobal` | `DefinitionId` | Pop stack → assign to global (runtime error if immutable) |

#### Temp variables (`0x34`–`0x39`)

| Opcode | Operand | Description |
|--------|---------|-------------|
| `DeclareTemp` | `u16` slot | Declare temp at slot in current frame |
| `GetTemp` | `u16` slot | Push temp value (auto-dereferences pointers) |
| `SetTemp` | `u16` slot | Pop stack → assign to slot (writes through pointers) |
| `GetTempRaw` | `u16` slot | Push raw temp value without dereferencing |
| `PushVarPointer` | `DefinitionId` | Push pointer to a global variable |
| `PushTempPointer` | `u16` slot | Push pointer to a temp slot in current frame |

#### Control flow (`0x40`–`0x44`)

| Opcode | Operand | Description |
|--------|---------|-------------|
| `Jump` | `i32` offset | Unconditional relative jump within container |
| `JumpIfFalse` | `i32` offset | Pop stack; jump if falsy |
| `Goto` | `DefinitionId` | Unconditional divert to address (replaces current position) |
| `GotoIf` | `DefinitionId` | Pop condition; divert if truthy |
| `GotoVariable` | — | Pop `DivertTarget` from stack; divert to it |

#### Container flow (`0x48`–`0x49`)

| Opcode | Operand | Description |
|--------|---------|-------------|
| `EnterContainer` | `DefinitionId` | Push position stack, enter child container |
| `ExitContainer` | — | Pop position stack, resume at caller |

#### Functions & tunnels (`0x50`–`0x55`)

| Opcode | Operand | Description |
|--------|---------|-------------|
| `Call` | `DefinitionId` | Push call frame + enter function |
| `Return` | — | Pop call frame |
| `TunnelCall` | `DefinitionId` | Tunnel call (push return address, enter) |
| `TunnelReturn` | — | Pop tunnel return address |
| `TunnelCallVariable` | — | Pop `DivertTarget` from stack; tunnel call to it |
| `CallVariable` | — | Pop `DivertTarget` from stack; function call to it |

#### Threads (`0x57`–`0x59`)

| Opcode | Operand | Description |
|--------|---------|-------------|
| `ThreadCall` | `DefinitionId` | Fork call stack and begin executing thread at target |
| `ThreadStart` | — | Mark start of a thread's execution |
| `ThreadDone` | — | Mark thread as complete |

#### Output (`0x60`–`0x66`)

| Opcode | Operand | Description |
|--------|---------|-------------|
| `EmitLine` | `u16` line index + `u8` slot count | Emit line from scope's line table. Pops `slot_count` values from stack for template slot resolution. |
| `EmitValue` | — | Stringify + emit top of stack |
| `EmitNewline` | — | Emit newline to output buffer |
| `Glue` | — | Join adjacent output (suppress whitespace/newline) |
| `BeginTag` | — | Begin a tag annotation on the current output |
| `EndTag` | — | End the current tag annotation |
| `EvalLine` | `u16` line index + `u8` slot count | Like `EmitLine` but pushes resolved string to value stack instead of output buffer. Pops `slot_count` values from stack for template slot resolution. |

#### Choices (`0x72`–`0x73`)

| Opcode | Operand | Description |
|--------|---------|-------------|
| `BeginChoice` | `ChoiceFlags` + `DefinitionId` target | Begin a choice with flags and target container |
| `EndChoice` | — | End current choice |

**ChoiceFlags** (5-bit bitmask):

| Bit | Flag | Description |
|-----|------|-------------|
| 0 | `has_condition` | Choice has a condition to evaluate |
| 1 | `has_start_content` | Text before `[` in the original ink choice |
| 2 | `has_choice_only_content` | Text inside `[]` (metadata only under single-pop protocol) |
| 3 | `once_only` | Choice can only be selected once (`*` vs `+`) |
| 4 | `is_invisible_default` | Fallback choice (not displayed to player) |

#### Sequences (`0x78`–`0x79`)

| Opcode | Operand | Description |
|--------|---------|-------------|
| `Sequence` | `SequenceKind` + `u8` branch count | Begin a sequence with N branches |
| `SequenceBranch` | `i32` offset | Jump offset to the next branch |

**SequenceKind**: `Cycle` (0), `Stopping` (1), `OnceOnly` (2), `Shuffle` (3).

#### Intrinsics (`0x80`–`0x86`)

| Opcode | Operand | Description |
|--------|---------|-------------|
| `VisitCount` | — | Pop `DivertTarget` from stack, push its visit count |
| `TurnsSince` | — | Pop `DivertTarget`, push turns since last visit (-1 if never) |
| `TurnIndex` | — | Push current turn number |
| `ChoiceCount` | — | Push number of currently available choices |
| `Random` | — | Pop max, pop min, push random int in [min, max] |
| `SeedRandom` | — | Pop seed value, reseed RNG |
| `CurrentVisitCount` | — | Push visit count of the *current* container (no stack input) |

#### Casts & math (`0x90`–`0x96`)

| Opcode | Description |
|--------|-------------|
| `CastToInt` | Pop value, push as `Int` |
| `CastToFloat` | Pop value, push as `Float` |
| `Floor` | Pop float, push floor as `Int` |
| `Ceiling` | Pop float, push ceiling as `Int` |
| `Pow` | Pop exponent, pop base, push base^exponent |
| `Min` | Pop b, pop a, push min(a, b) |
| `Max` | Pop b, pop a, push max(a, b) |

#### External functions (`0xA0`)

| Opcode | Operand | Description |
|--------|---------|-------------|
| `CallExternal` | `DefinitionId` + `u8` arg count | Call an external function |

#### List operations (`0xB0`–`0xBD`)

| Opcode | Description |
|--------|-------------|
| `ListContains` | Pop item, pop list, push whether list contains item |
| `ListNotContains` | Pop item, pop list, push whether list does NOT contain item |
| `ListIntersect` | Pop b, pop a, push intersection |
| `ListAll` | Pop list, push all items from its origin definitions |
| `ListInvert` | Pop list, push complement relative to origin definitions |
| `ListCount` | Pop list, push item count |
| `ListMin` | Pop list, push minimum item (as single-element list) |
| `ListMax` | Pop list, push maximum item (as single-element list) |
| `ListValue` | Pop list, push integer ordinal value of the single item |
| `ListRange` | Pop max, pop min, pop list, push items in ordinal range |
| `ListFromInt` | Pop ordinal, pop list-def target, push single-item list |
| `ListRandom` | Pop list, push random item from list |

Note: opcodes `0xB3` and `0xB4` are unassigned. List union and except are handled by `Add` and `Subtract` respectively, which are overloaded for list operands.

#### Collection operations (`0xBE`–`0xC9`, reserved)

Named in `docs/format-v4-rfc.md` §3 "Collections (T1a)"; numeric assignments
are frozen by the §9 one-bump rule, contiguous and adjacent to List
operations above. Not `Opcode` variants yet — there is no decode match arm
for these bytes, so the strict reader rejects them (`UnknownOpcode`) until
the T1a compiler surface begins emitting them.

| Opcode | Description |
|--------|-------------|
| `0xBE` `ArrayNew(n)` | Pop `n` values, push a new array |
| `0xBF` `MapNew(n)` | Pop `2n` values (key/value pairs), push a new map |
| `0xC0` `IndexGet` | Pop index, pop collection, push element |
| `0xC1` `IndexSet` | Pop value, pop index, pop collection, push updated collection |
| `0xC2` `Len` | Pop collection, push length |
| `0xC3` `MapGet` | Pop key, pop map, push value |
| `0xC4` `MapInsert` | Pop value, pop key, pop map, push updated map |
| `0xC5` `MapRemove` | Pop key, pop map, push updated map |
| `0xC6` `MapContains` | Pop key, pop map, push whether map contains key |
| `0xC7` `Keys` | Pop map, push array of keys |
| `0xC8` `Values` | Pop map, push array of values |
| `0xC9` `PushLiteral(u32)` | Push pooled literal by `LiteralPool` index; absorbs `PushList`/`ListLiterals` |

Sharing-discipline ops (`TakeVar`, `StoreVarIfNew`, `EqVars` —
`docs/value-model-spec.md` §6) and later Tier-1 opcode groups (functions,
handles, projections, records — RFC §3) are named in the RFC but out of this
reservation; each gets its own contiguous block, numbered when its own
milestone lands.

#### Sharing-discipline operations (`0xCA`–`0xCD`)

Named in `docs/format-v4-rfc.md` §3 "Sharing discipline (T1a)"; semantics in
`docs/value-model-spec.md` §5, §6, and §9. Numeric assignments are frozen by
the §9 one-bump rule, contiguous and adjacent to the collection block above.
`TakeGlobal`/`TakeTemp` are live as of T1b-4 (#576) — the RFC's generic
`TakeVar(slot)` split into its two concrete slot kinds (a global
`DefinitionId` and a temp `u16` don't share an operand encoding, so one
opcode can't cover both). `StoreVarIfNew`/`EqVars` remain reserved (no
`Opcode` variant — `decode`'s catch-all still rejects `0xCB`/`0xCC` as
`UnknownOpcode`) until their own milestone.

| Opcode | Description |
|--------|-------------|
| `0xCA` `TakeGlobal(DefinitionId)` | Move the value out of the named global, leaving `Null` behind — the take-half of the take → `make_mut` → write-back RMW discipline that closes the indexed-write COW cliff (value-model spec §5). No auto-dereference (mirrors `GetGlobal`/`SetGlobal` — a `ref`-param pointer lives in a temp, never a global). |
| `0xCB` `StoreVarIfNew` (reserved) | Optional store-time keep-old-Arc cutoff: skip the write if the new value is structurally equal to the existing one (value-model spec §6) |
| `0xCC` `EqVars(a, b)` (reserved) | Fused compare of two variable slots (peephole over `LoadVar a; LoadVar b; Eq`), with optional ref-collapse on equality (value-model spec §6) |
| `0xCD` `TakeTemp(slot)` | Move the value out of the temp slot, leaving `Null` behind — `TakeGlobal`'s temp-slot counterpart. Auto-dereferences like `GetTemp`: if the slot holds a `VariablePointer`/`TempPointer` (a `ref` parameter), the *pointed-to* location is taken and left `Null`, while the pointer itself stays in the slot untouched. |

`StoreVarIfNew`/`EqVars` are optional peephole/sharing optimizations, never
required for correctness — v1 ships with just the `ptr_eq` equality fast
path (spec §6). `TakeGlobal`/`TakeTemp` are load-bearing for the loop-append
performance claim in spec §5 but never required for *correctness* either —
the compiler's fallback (ordinary `GetGlobal`/`GetTemp` clone-based RMW,
still used for chained indexed assignment, T1b-4 PR description) produces
identical observable results, just without the O(1)-amortized guarantee.
Later Tier-1 opcode groups (functions, handles, projections, records — RFC
§3) remain named in the RFC but out of this reservation; each gets its own
contiguous block, numbered when its own milestone lands.

#### String eval (`0xE0`–`0xE1`)

| Opcode | Description |
|--------|-------------|
| `BeginStringEval` | Begin inline string evaluation (output goes to stack, not output buffer) |
| `EndStringEval` | End string evaluation, push concatenated result as `String` value |

#### Lifecycle (`0xF0`–`0xF2`)

| Opcode | Description |
|--------|-------------|
| `Done` | Pause execution (can resume — end of a passage/turn) |
| `End` | Permanent finish — story is over |
| `Nop` | No operation (used for alignment/padding) |

#### Debug (`0xFE`)

| Opcode | Operand | Description |
|--------|---------|-------------|
| `SourceLocation` | `u32` line + `u32` col | Source location mapping (strippable) |

## Format contents

`brink-format` provides:

- `DefinitionId(u64)` — tagged definition identity type (8-bit type tag + 56-bit name hash)
- `DefinitionTag` — enum of tag discriminants (`Address`, `GlobalVar`, `ListDef`, `ListItem`, `ExternalFn`, `LocalVar`)
- `NameId(u16)` — index into the name table (internal strings, not localizable)
- `LineId = (DefinitionId, u16)` — scope-scoped line identity (lexical scope = knot or stitch; all user-visible text output). The `DefinitionId` refers to the lexical scope, not the emitting container.
- `Opcode` — enum of all bytecode instructions with encode/decode
- `DecodeError` — error type for all format decoding failures
- Definition payloads: `AddressDef`, `ContainerDef`, `GlobalVarDef`, `ListDef`, `ListItemDef`, `ExternalFnDef`
- `CountingFlags` — bitflags for container visit/turn tracking
- `Value` type and `ValueType` discriminant
- `ListValue` — set of active items + origin definitions
- `ChoiceFlags` — 5-bit bitmask for choice properties
- `SequenceKind` — cycle/stopping/once-only/shuffle discriminant
- Line content types: `LineEntry`, `LineContent`, `LineTemplate`, `LinePart`, `SelectKey`, `PluralCategory`, `SlotInfo`, `SourceLocation`
- `PluralResolver` trait (implemented by host or `brink-intl`)
- Serialization/deserialization for `.inkb`, `.inkl`, and `.inkt`

## File formats

- **`.inkb`** — binary format. Definition tables (containers, addresses, variables, lists, externals), per-scope line tables, list literals, name table, and metadata. All cross-definition references are symbolic (`DefinitionId`). No resolved indices. Line tables are keyed by lexical scope `DefinitionId`, not by container.
- **`.inkl`** — locale overlay. Per-scope replacement line tables for a specific locale. Each entry contains localized content and an optional audio ref. Keyed by scope `DefinitionId` + local line index for stability across recompilation.
- **`.inkt`** — textual format. Human-readable representation of the bytecode, like WAT is to WASM. Container paths as labels, opcodes as mnemonics. For debugging, inspection, and diffing.

### `.inkb` layout

#### Header

```text
Offset  Size   Field
------  -----  ------
0       4      Magic: b"INKB"
4       2      Version: u16 LE (= 4)
6       1      Section count: u8 (N entries in offset table)
7       1      Reserved: 0x00
8       4      File size: u32 LE (total bytes)
12      4      Content checksum: u32 LE (CRC-32 of all bytes after header)
16      N*8    Offset table entries
```

Each offset table entry (8 bytes):

```text
0       1      SectionKind: u8 tag
1       3      Reserved: 3 bytes of 0x00
4       4      Offset: u32 LE (byte offset from start of file)
```

#### Sections

| Tag | Section | Contents |
|-----|---------|----------|
| `0x01` | Name table | `NameId` → text, for internal strings: definition names, debug labels |
| `0x02` | Variables | Per entry: `DefinitionId` + `NameId` + type + default + mutable |
| `0x03` | List definitions | Per entry: `DefinitionId` + `NameId` + items |
| `0x04` | List items | Per entry: `DefinitionId` + origin + ordinal + name |
| `0x05` | Externals | Per entry: `DefinitionId` + `NameId` + arg count + optional fallback |
| `0x06` | Containers | Per container: `DefinitionId` + bytecode blob + content hash + counting flags + path hash + scope id |
| `0x07` | Line tables | Per scope: `DefinitionId` + line entries (content + source hash + slot info + audio ref + source location each). The `DefinitionId` here is the lexical scope (knot/stitch), not a container. |
| `0x08` | Labels | Per entry: address `DefinitionId` + container `DefinitionId` + byte offset |
| `0x09` | List literals | Per entry: `ListValue` (items + origins) |
| `0x0A` | Address paths | Per entry: qualified-path hash → target `DefinitionId` |
| `0x0E` | Visibility (M-2b) | Per entry: the `DefinitionId` of a `#@private` definition, sorted ascending. **Optional** — omitted when empty. See below. |
| `0x0F` | Alias table (M-3) | Section-local version byte, then per entry: old `DefinitionId` → new `DefinitionId`, sorted by old. Always present (possibly empty). See below. |
| `0x10` | Frame shapes (FS-3) | Section-local version byte, then per `await` site: the site's stable `DefinitionId` (the synthesized continuation container) + its name-keyed crossing-local slots (`NameId`s), sorted by site. **Optional** — omitted when empty. See below. |

**Reserved v4 sections** — numeric assignments are frozen by the §9 one-bump
rule (`docs/format-v4-rfc.md` §2 "Sections") but not `SectionKind` variants
yet — the strict reader rejects an offset-table entry tagged with one of
these (`InvalidSectionKind`) until each section's milestone lands and adds a
real variant + `from_u8` arm, the same discipline the reserved v4 value tags
below already follow. `0x0B` `LiteralPool` (T1a; content-hash-deduplicated
constant pool, absorbs `List literals` — `PushList` retires in favor of
`PushLiteral(idx)` when the collection opcodes land), `0x0C` `StructShapes`
(reserved, count always 0 in 4.0), `0x0D` `EffectRows` (reserved, count
always 0 in 4.0, section-locally versioned so T2 can define the row encoding
without another format bump).

**`Visibility` section (`0x0E`, M-2b)** — carries per-definition visibility
(`docs/modules-spec.md` §4): a `u32` count followed by the `DefinitionId` of
every `#@private` definition, sorted ascending. It is the complement encoding
— public is the default, private names are enumerated — mirroring how
`#@local` scope defaults are carried. **The writer omits the section entirely
when there are no private definitions**, so the whole all-public / pre-modules
world stays byte-identical and this section needs **no VERSION bump**: it is
purely additive and self-framed in the offset table (a reader tolerates its
absence, decoding to an empty set — the append-only-section evolution this
spec recommends). The runtime builds a lookup set from it to refuse host
*semantic* access (variable get/set, entry lookup, function eval) to private
defs; host *persistence* (save/load/journal/replay) never consults it and sees
everything (§4 boundary rule 2). `0x0D` is reserved for `EffectRows`, so this
takes the next free tag, `0x0E`.

**`AliasTable` section (`0x0F`, M-3)** — carries old→new `DefinitionId` rename
records (`docs/modules-spec.md` §5) emitted from `#@was(old_name)`
directives: a one-byte section-local version, then a `u32` count, then that
many `(old DefinitionId, new DefinitionId)` pairs sorted by `old` for the
runtime's binary-search miss-path lookup. Unlike `Visibility`, this section
is **mandatory** — always present (possibly empty) — because it is a
brand-new section not part of the v4 RFC's pre-reserved inventory, so its
introduction is its own one-bump event: `.inkb` format version 4 → 5 (the
row encoding itself stays section-locally versioned so it can still evolve
without a further whole-file bump). `0x0E` is taken by `Visibility`, so this
takes the next free tag, `0x0F`.

A `#@was` on a knot or stitch mints **one entry per descendant** whose
qualified name (and so `DefinitionId`) changed too — every stitch and label
beneath a renamed knot, every label beneath a renamed stitch — not just one
entry for the renamed declaration itself (issue #1671): a rename's own id
is recoverable only from the declared `#@was`, but a descendant's stale id
can never be derived at load time (a `DefinitionId` is a hash; no path can
be recovered from one), so the compiler must materialize the whole bridge
set while it still knows every descendant's path. Table growth is therefore
bounded by the renamed container's subtree size, not by 1 per `#@was`
directive — a knot with many stitches and labels renamed once produces one
entry per descendant, additively (a stitch renamed *simultaneously* with
its own enclosing knot produces one edge per level, not a doubly-old
composite path).

**`FrameShapes` section (`0x10`, FS-3)** — carries per-`await`-site frame
shapes (`docs/flow-suspension-spec.md` §4/§11): a one-byte section-local
version, then a `u32` count, then that many entries, each the site's stable
`DefinitionId` (the synthesized **continuation container** the runtime enters
on resume — its identity is `module + enclosing def + site index`, never an
instruction offset) followed by a `u32` slot count and that many `NameId`
slots — the name-keyed locals that cross the park (what the runtime spills on
park / restores on wake). Like `Visibility`, it is **optional** — omitted
entirely when empty — so its introduction needed **no** `VERSION` bump, and
every existing story stays byte-identical. Behind the E052 `await` lowering
fence (FS-3c) no `await` compiles, so this section is empty for every story
produced today; its first non-empty emission rides the continuation-splitting
codegen when the fence drops (FS-3r), the same reserved-then-materialized
discipline `StructShapes` followed. `0x0F` is taken by `AliasTable`, so this
takes the next free tag, `0x10`.

The synthesized continuation containers this section names are **invisible**
(`docs/flow-suspension-spec.md` §11.2): marked with the `CountingFlags`
`INVISIBLE` bit (`0x08`), they carry no visit counts, are not valid divert
targets, and are hidden from IDE navigation/completion (debug views such as
the `.inkt` dump excepted). The flag rides the container's existing counting
byte — no new field, no layout change.

#### Value type tags in `.inkb`

| Tag | Type | Encoding |
|-----|------|----------|
| `0x00` | Int | `i32` LE |
| `0x01` | Float | `f32` LE |
| `0x02` | Bool | `u8` (0/1) |
| `0x03` | String | length-prefixed UTF-8 |
| `0x04` | List | item `DefinitionId`s + origin `DefinitionId`s |
| `0x05` | DivertTarget | `DefinitionId` |
| `0x06` | Null | (none) |
| `0x07` | VariablePointer | `DefinitionId` |
| `0x08` | FragmentRef | `u32` fragment index |
| `0x09` | Array | `u32` len, then that many recursively-encoded values (v4) |
| `0x0A` | Map | `u32` len, then that many `(key, value)` pairs **in insertion order** (v4) |

**v4 collections** (`Array`/`Map`) use a plain tree encoding — a length prefix
followed by recursively-encoded children. `Arc` sharing is **not** preserved on
the wire (`docs/value-model-spec.md` §5); a snapshot serializes as a plain
nested tree. Map keys are restricted to the scalar domain `int`/`string`/`bool`
and are written with the corresponding scalar tag (`0x00`/`0x03`/`0x02`); the
strict reader rejects any other key tag. Insertion order is semantic. The same
tag surface is shared by the runtime transcript (`.brkt`) value encoding.

**Reserved v4 value tags** — numeric assignments are frozen by the §9 one-bump
rule but emitted by nothing in 4.0 (each is materialized when its Tier-1
milestone lands, still under VERSION 4). The strict reader rejects them until
then. `0x0B` `FnRef` (T1c), `0x0C` `Closure` (T1c), `0x0D` `Handle` (T1d),
`0x0E` `Projection` (T1e), `0x0F` `Record` (reserved, typed-dialect era). See
`docs/format-v4-rfc.md` §1 for their encodings.

`TempPointer` is never serialized — it is runtime-only. During `.inkb` encoding, a `TempPointer` value is written as `Null`.

### `.inkl` sections

- Header: magic `b"INKL"`, format version, BCP 47 locale tag, base `.inkb` checksum (must match)
- Per-scope line tables (keyed by scope `DefinitionId`, each entry: local line index, localized content, optional audio ref)

There is no separate audio table. Audio refs are stored per-line alongside content in the per-scope line tables.

### Line entry structure

Each line entry in a scope's line table:

```
LineEntry {
    content: LineContent,                  // Plain(String) or Template(LineTemplate)
    source_hash: u64,                      // hash of original ink source text
    audio_ref: Option<String>,             // audio asset identifier, if any
    slot_info: Vec<SlotInfo>,              // metadata per slot index (empty for Plain)
    source_location: Option<SourceLocation>,  // where in the .ink source this line came from
}

SlotInfo {
    index: u8,                             // matches Slot(u8) index in the template
    name: String,                          // source expression text, e.g. "player_name"
}

SourceLocation {
    file: String,                          // source file path (relative to project root)
    range: (u32, u32),                     // byte offset start, end in the source file
}
```

`slot_info` and `source_location` are metadata for tooling. They are serialized into the `.inkb` line tables section but are not loaded by the runtime's fast path. The `.inkl` overlay format carries `content` and `audio_ref` but NOT slot info or source location — those are source-language concerns, not translation concerns.

### Line template types

```
LineContent = Plain(String) | Template(LineTemplate)

LineTemplate = Vec<LinePart>

enum LinePart {
    Literal(String),
    Slot(u8),
    Select {
        slot: u8,
        variants: Vec<(SelectKey, String)>,
        default: String,
    },
    // `<name attr="v">…</name>` — an inline markup span (#1716,
    // `docs/prose-dialect-spec.md` §4.4). Genuinely nested: `children` is
    // itself a `Vec<LinePart>`, so a span can contain literals, slots,
    // other spans, or (structurally, though the compiler never emits it —
    // §4.4/§4.5) a select. Empty `children` is the self-closing /
    // point-marker shape (`<pause/>`, `<sfx name="bell"/>`, §8b.11).
    // Hash-transparent: `name`/`attrs` never contribute to `source_hash`,
    // only `children`'s own text/slots do, recursively.
    Span {
        name: String,
        attrs: Vec<(String, String)>,
        children: Vec<LinePart>,
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

A line's content is either plain text (`Plain`) or a `LineTemplate` with slots, selectors, and spans. The runtime's line resolver walks the `LinePart` tree, reads slot values from the VM stack, picks select variants (using the `PluralResolver` trait for plural categories), recurses into `Span` children the same way, and appends formatted text to the output buffer. Select variants and defaults are flat `String` values — not nested `LinePart` trees. `Span` is the one variant that *is* nested (`children: Vec<LinePart>`) — see its doc comment above.

#### `LinePart` wire tags

| Tag | Variant | Encoding |
|-----|---------|----------|
| `0x00` | `Literal` | length-prefixed UTF-8 string |
| `0x01` | `Slot` | `u8` slot index |
| `0x02` | `Select` | `u8` slot, then variant count + `(SelectKey, String)` pairs, then default `String` |
| `0x03` | `Span` | name `String`, attr count + `(String, String)` pairs, then child count + that many recursively-encoded `LinePart`s (v6, #1716) |

`Span` was introduced at `.inkb` `VERSION` 6 / `.inkl` version 2 (see Versioning above) — unlike a *reserved* tag materializing an already-frozen slot, `0x03` was a genuinely new tag on the `LinePart` dispatch, so its addition is its own one-bump event, not a free ride on the v4 reserved-tag precedent. An old reader hard-rejects `0x03` via the same unrecognized-tag path every out-of-range tag already takes (`InvalidLinePart`).

### Plural resolution

The runtime defines a `PluralResolver` trait:

```
trait PluralResolver {
    fn cardinal(&self, n: i64, locale_override: Option<&str>) -> PluralCategory;
    fn ordinal(&self, n: i64) -> PluralCategory;
}
```

The `locale_override` parameter allows overriding the resolver's default locale for a specific resolution call.

The runtime ships no locale data. Consumers provide a resolver via:

- **`brink-intl`** — batteries-included crate backed by ICU4X baked data, pruned at build time to only the locales the consumer specifies.
- **Custom implementation** — game engines with their own i18n system implement the trait directly.
- **No resolver** — stories without localization don't need one. Fallback: everything maps to `Other`.
