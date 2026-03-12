# brink format specification

`brink-format` defines the binary interface between compiler and runtime — the types, instruction set, and file formats that bridge compilation and execution. It is the ONLY dependency of `brink-runtime`.

See also: [compiler-spec](compiler-spec.md) (how the compiler produces these types), [runtime-spec](runtime-spec.md) (how the runtime consumes them).

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
| `0x07` | Local variable | Params and temps — scoped to a container, not serialized in bytecode |

Note: tag `0x06` is unassigned.

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
- The root story container gets an implicit final gather + `-> DONE` appended by the compiler.

## Global variables (tag `0x02`)

Each variable definition has:

- **`DefinitionId`** — `0x02` tag + hash of variable name
- **Name** — `NameId` (for debugging/inspection and host binding)
- **Value type** — the type of the default value
- **Default value** — `Value` (same type as the VM stack)
- **Mutable** — `bool` (`true` for `VAR`, `false` for `CONST`)

`VAR` declarations are mutable globals. `CONST` declarations are immutable globals — they always exist in the format (visible, inspectable, debuggable). The compiler may inline CONST values as a build-time optimization controlled by a compiler flag, but the definition is always present. Attempting to `SetGlobal` on an immutable variable is a runtime error.

Temporary variables (`temp`) have no format-level definition. They are call-frame-local — created by a `DeclareTemp` opcode during execution, stored in the current call frame's temp slot array, and discarded when the frame pops. Temp slot indices are assigned by the compiler/converter across the entire knot/function scope (including all child containers reached by flow entry), not per-container.

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
Int(i32) | Float(f32) | Bool(bool) | String(Rc<str>) | List(Rc<ListValue>) | DivertTarget | VariablePointer | TempPointer | Null
```

`String` and `List` are `Rc`-wrapped so that cloning a `Value` is always O(1) — a refcount bump, not a deep copy. This makes call-frame cloning (during `fork_thread`) essentially free.

`DivertTarget` holds a `DefinitionId` pointing to a container — used for variable divert targets (`VAR x = -> some_knot`).

`VariablePointer(DefinitionId)` — a pointer to a global variable, used for `ref` parameters that target globals. The converter emits `PushVarPointer` to create these.

`TempPointer { slot: u16, frame_depth: u16 }` — a runtime-only pointer to a temp variable in a specific call frame, used for `ref` parameters that target temps. The converter emits `PushTempPointer(slot)`, and the runtime resolves it to `TempPointer { slot, frame_depth: current_frame }` at execution time. `TempPointer` never appears in `.inkb` files — it exists only on the value stack and in call-frame temp slots during execution.

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
| `EmitLine` | `u16` line index | Emit line from container's line table to output buffer |
| `EmitValue` | — | Stringify + emit top of stack |
| `EmitNewline` | — | Emit newline to output buffer |
| `Glue` | — | Join adjacent output (suppress whitespace/newline) |
| `BeginTag` | — | Begin a tag annotation on the current output |
| `EndTag` | — | End the current tag annotation |
| `EvalLine` | `u16` line index | Like `EmitLine` but pushes result to value stack instead of output buffer |

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
- `LineId = (DefinitionId, u16)` — container-scoped line identity (all user-visible text output)
- `Opcode` — enum of all bytecode instructions with encode/decode
- `DecodeError` — error type for all format decoding failures
- Definition payloads: `AddressDef`, `ContainerDef`, `GlobalVarDef`, `ListDef`, `ListItemDef`, `ExternalFnDef`
- `CountingFlags` — bitflags for container visit/turn tracking
- `Value` type and `ValueType` discriminant
- `ListValue` — set of active items + origin definitions
- `ChoiceFlags` — 5-bit bitmask for choice properties
- `SequenceKind` — cycle/stopping/once-only/shuffle discriminant
- Line content types: `LineContent`, `LineTemplate`, `LinePart`, `SelectKey`, `PluralCategory`
- `PluralResolver` trait (implemented by host or `brink-intl`)
- Serialization/deserialization for `.inkb`, `.inkl`, and `.inkt`

## File formats

- **`.inkb`** — binary format. Definition tables (containers, addresses, variables, lists, externals), line tables, list literals, name table, and metadata. All cross-definition references are symbolic (`DefinitionId`). No resolved indices.
- **`.inkl`** — locale overlay. Per-container replacement line tables for a specific locale. Each entry contains localized content and an optional audio ref. Keyed by container `DefinitionId` + local line index for stability across recompilation.
- **`.inkt`** — textual format. Human-readable representation of the bytecode, like WAT is to WASM. Container paths as labels, opcodes as mnemonics. For debugging, inspection, and diffing.

### `.inkb` layout

#### Header

```text
Offset  Size   Field
------  -----  ------
0       4      Magic: b"INKB"
4       2      Version: u16 LE (= 1)
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
| `0x06` | Containers | Per container: `DefinitionId` + bytecode blob + content hash + counting flags + path hash |
| `0x07` | Line tables | Per container: `DefinitionId` + line entries (content + source hash each) |
| `0x08` | Labels | Per entry: address `DefinitionId` + container `DefinitionId` + byte offset |
| `0x09` | List literals | Per entry: `ListValue` (items + origins) |

#### Value type tags in `.inkb`

| Tag | Type |
|-----|------|
| `0x00` | Int |
| `0x01` | Float |
| `0x02` | Bool |
| `0x03` | String |
| `0x04` | List |
| `0x05` | DivertTarget |
| `0x06` | Null |
| `0x07` | VariablePointer |

`TempPointer` is never serialized — it is runtime-only. During `.inkb` encoding, a `TempPointer` value is written as `Null`.

### `.inkl` sections

- Header: magic `b"INKL"`, format version, BCP 47 locale tag, base `.inkb` checksum (must match)
- Per-container line tables (keyed by container `DefinitionId`, each entry: local line index, localized content, optional audio ref)

There is no separate audio table. Audio refs are stored per-line alongside content in the per-container line tables.

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
}

enum SelectKey {
    Cardinal(PluralCategory),    // CLDR cardinal: zero, one, two, few, many, other
    Ordinal(PluralCategory),     // CLDR ordinal: zero, one, two, few, many, other
    Exact(i32),                  // exact numeric match
    Keyword(String),             // for gender, custom categories
}

enum PluralCategory { Zero, One, Two, Few, Many, Other }
```

A line's content is either plain text (`Plain`) or a `LineTemplate` with slots and selectors. The runtime's line resolver walks the `LinePart` tree, reads slot values from the VM stack, picks select variants (using the `PluralResolver` trait for plural categories), and appends formatted text to the output buffer. Select variants and defaults are flat `String` values — not nested `LinePart` trees.

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
