# Containers & DefinitionId

## DefinitionId

All named things in brink use a single `DefinitionId(u64)` type. The high 8 bits are a type tag; the low 56 bits are a hash of the fully qualified ink path.

```
DefinitionId (u64):
+-----------+------------------------------------------------------+
| tag (8)   |                    hash (56)                          |
+-----------+------------------------------------------------------+
```

Serialized as `$tt_hhhhhhhhhhhhhh` (tag hex + underscore + 56-bit hash hex).

### Definition tags

| Tag | Kind | Description |
|-----|------|-------------|
| `0x01` | Address | Knot, stitch, gather, or intra-container label |
| `0x02` | Global variable | Name, type, default value |
| `0x03` | List definition | Enum-like type with named items |
| `0x04` | List item | Individual member of a list definition |
| `0x05` | External function | Host-provided function binding |
| `0x07` | Local variable | Temp/param (not serialized, compile-time only) |

The uniform ID scheme provides stability across recompilation (same ink path always produces the same ID), a simple linker (all references are ID lookups), and save file portability (IDs don't depend on compilation order).

## Containers

Containers are the fundamental unit of bytecode execution. At the source level, ink has knots, stitches, gathers, and labeled choice targets. At the bytecode level, these are all **containers**: a `DefinitionId`, a block of bytecode, and metadata.

```rust,ignore
struct ContainerDef {
    id: DefinitionId,
    bytecode: Vec<u8>,
    content_hash: u64,
    counting_flags: CountingFlags,
    path_hash: i32,               // Seed for shuffle RNG
}
```

### Container hierarchy

Containers form a logical hierarchy that mirrors the ink source structure:

- The **root container** holds the top-level flow (content before the first knot).
- **Knots** are top-level containers.
- **Stitches** may be sub-containers within a knot, or addresses within the knot's bytecode.
- **Gathers** and **labeled choice targets** may become addresses within their parent container.

The compiler decides which source constructs become their own container vs. being inlined as addresses within a parent container. This is determined during the LIR planning phase.

## Addresses

An `AddressDef` names a location within a container:

```rust,ignore
struct AddressDef {
    id: DefinitionId,              // The address's own ID
    container_id: DefinitionId,    // Which container it lives in
    byte_offset: u32,              // Position within the container's bytecode
}
```

The **primary address** of a container has `byte_offset == 0` and `id == container_id` -- it is the container's entry point. Non-primary addresses (stitches within a knot, gathers, labels) have distinct IDs and non-zero offsets.

## Counting flags

`CountingFlags` is a bitfield (`u8`) that controls visit and turn tracking for a container:

```rust,ignore
bitflags! {
    pub struct CountingFlags: u8 {
        const VISITS          = 0x01;
        const TURNS           = 0x02;
        const COUNT_START_ONLY = 0x04;
    }
}
```

- **VISITS** (`0x01`) -- the VM increments a counter each time the container is entered. Used by `VISITS()` and conditional logic that depends on how many times content has been seen.
- **TURNS** (`0x02`) -- the VM records the turn number when the container is entered. Used by `TURNS_SINCE()`.
- **COUNT_START_ONLY** (`0x04`) -- only count the visit/turn when the container is entered at its first instruction (byte offset 0), not when re-entered mid-way via a divert.

These flags are set by the compiler based on whether the ink source uses `VISITS()`, `TURNS_SINCE()`, or similar intrinsics that reference the container.

## What is NOT a definition

Several important types are scoped more narrowly and do not get `DefinitionId`s in the binary format:

- **Temp variables** -- identified by slot index (`u16`) within a call frame. The `LocalVar` tag (`0x07`) exists for compiler-internal use but temps are not serialized as definitions in the bytecode.
- **`NameId`** -- a `u16` index into the story's name table. Stores human-readable names for variables, list items, and externals. Names are for display and host binding only; the runtime identifies definitions by `DefinitionId`, not by name.
- **`LineId`** -- a `(container: DefinitionId, index: u16)` pair that references a specific line entry within a container's line table. Lines hold output text and are emitted by `EmitLine(index)`, not addressed as definitions.
