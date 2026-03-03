# Binary Format

`brink-format` defines the binary interface between compiler and runtime. It is the ONLY dependency of `brink-runtime`.

## File formats

| Extension | Format | Description |
|-----------|--------|-------------|
| `.inkb` | Binary | Definition tables, name table, metadata. All references are symbolic (`DefinitionId`). |
| `.inkl` | Locale overlay | Per-container replacement line tables and audio mappings for a specific locale. |
| `.inkt` | Textual | Human-readable bytecode disassembly, like WAT is to WASM. |

## `.inkb` sections

<!-- TODO: detail each section:
  - Header (magic, format version, section offsets, checksum)
  - Container section (per container: DefinitionId + bytecode + content hash + counting flags + line sub-table)
  - Variable section
  - List definition section
  - List item section
  - External function section
  - Name table
  - Debug info (strippable)
-->

## `.inkl` sections

<!-- TODO: detail locale overlay format:
  - Header: magic, format version, BCP 47 locale tag, base .inkb checksum
  - Per-container line tables
  - Audio table (LineId → audio asset reference)
-->

## `.inkt` format

<!-- TODO: explain the textual disassembly format — container paths as labels,
     opcodes as mnemonics, useful for debugging and diffing -->
