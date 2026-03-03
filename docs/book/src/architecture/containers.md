# Containers & DefinitionId

## DefinitionId

All named things in brink use a single `DefinitionId(u64)` type. The high 8 bits are a type tag; the low 56 bits are a hash of the fully qualified name.

```
DefinitionId (u64):
┌──────────┬──────────────────────────────────────────────────┐
│ tag (8)  │                  hash (56)                       │
└──────────┴──────────────────────────────────────────────────┘
```

### Definition tags

| Tag | Kind | Description |
|-----|------|-------------|
| `0x01` | Container | Bytecode, content hash, counting flags |
| `0x02` | Global variable | Name, type, default value, mutability |
| `0x03` | List definition | Name, items with ordinals |
| `0x04` | List item | Origin list, ordinal |
| `0x05` | External function | Name, arg count, optional fallback |

<!-- TODO: explain why a uniform ID scheme — stability across recompilation,
     simple linker, save file portability -->

## Containers

Containers are the fundamental compilation and runtime unit. At the source level, ink has knots, stitches, gathers, and labeled choice targets. At the bytecode level, these are all **containers**.

<!-- TODO: container hierarchy diagram -->

<!-- TODO: explain the two entry modes:
  - Flow entry — push position onto current frame's container stack (shares temps)
  - Call entry — push new call frame with fresh temps (function/tunnel)
-->

## Counting flags

<!-- TODO: explain visits_should_be_counted, turn_index_should_be_counted,
     counting_at_start_only -->

## What is NOT a definition

<!-- TODO: explain temps, names (NameId), lines (LineId) -->
