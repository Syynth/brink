# brink format specification

`brink-format` defines the binary interface between compiler and runtime — the types, instruction set, and file formats that bridge compilation and execution. It is the ONLY dependency of `brink-runtime`.

See also: [compiler-spec](compiler-spec.md) (how the compiler produces these types), [runtime-spec](runtime-spec.md) (how the runtime consumes them).

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

## Containers (tag `0x01`)

Containers are the fundamental compilation and runtime unit, analogous to functions in a normal programming language. At the source level, ink has knots, stitches, gathers, and labeled choice targets. At the bytecode level, these are all **containers** — there is no distinction. This matches the reference ink runtime, which has a single `Container` type.

Each container definition has:

- **`DefinitionId`** — `0x01` tag + hash of fully qualified path (e.g., `hash("my_knot.my_stitch")`). Stable across recompilation as long as the path doesn't change.
- **Bytecode** — its own instruction stream
- **Content hash** — fingerprint of the bytecode, used during hot-reload to detect whether a container's implementation changed
- **Counting flags** (bitmask):
  - Bit 0: `visits_should_be_counted` — track visit count
  - Bit 1: `turn_index_should_be_counted` — record which turn it was visited on
  - Bit 2: `counting_at_start_only` — only count when entering at the start, not when re-entering mid-container

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
GetGlobal(DefinitionId)     // push global variable value
SetGlobal(DefinitionId)     // pop stack → assign to global (runtime error if immutable)
DeclareTemp(u16)            // declare temp at local slot index in current frame
GetTemp(u16)                // push temp value from frame slot
SetTemp(u16)                // pop stack → assign to frame slot
```

Globals use `DefinitionId` (resolved by linker to fast runtime index). Temps use call-frame-local slot indices assigned by the compiler across the entire knot/function scope — no `DefinitionId`, no linker involvement. Child containers reached by flow entry share the parent's call frame and use the same slot namespace.

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

### List values

A list value (for variable defaults and as literals in bytecode) is a set of items, potentially from multiple origin definitions:

```
ListValue {
    items: Vec<DefinitionId>      // list item DefinitionIds that are "set"
    origins: Vec<DefinitionId>    // list definition DefinitionIds (for typed empties)
}
```

The `origins` field preserves type information for empty lists — needed for `LIST_ALL` and `LIST_INVERT` to know the full universe of possible items.

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
- **Sequences:** sequence (with type bitmask: stopping/cycle/once/shuffle and valid combinations), sequence branch
- **Intrinsics:** visit count, turns since, turn index, choice count, random, seed random
- **External functions:** call external (`DefinitionId` + arg count)
- **List operations:** contains, not-contains, intersect, union, except, all, invert, count, min, max, value, range, list-from-int, random
- **Lifecycle:** done (pause, can resume), end (permanent finish)
- **Debug:** source location mapping (strippable)

The exact opcode encoding is defined in `brink-format`.

## Format contents

`brink-format` provides:

- `DefinitionId(u64)` — tagged definition identity type (8-bit type tag + 56-bit name hash)
- `NameId(u16)` — index into the name table (internal strings, not localizable)
- `LineId = (DefinitionId, u16)` — container-scoped line identity (all user-visible text output)
- Opcode definitions and encoding
- Definition payloads for each tag type (container, variable, list def, list item, external fn)
- `Value` type and encoding (int, float, bool, string, list, divert target, null)
- Line template types: `LineTemplate`, `LinePart`, `SelectKey`, `PluralCategory`
- `PluralResolver` trait (implemented by host or `brink-intl`)
- Serialization/deserialization for `.inkb`, `.inkl`, and `.inkt`

## File formats

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
