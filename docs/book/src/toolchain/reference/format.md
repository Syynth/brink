# Binary Format

`brink-format` defines the binary interface between compiler and runtime. It is the ONLY dependency of `brink-runtime`.

## File formats

| Extension | Format | Description |
|-----------|--------|-------------|
| `.inkb` | Binary | Compiled bytecode with definition tables, line tables, and metadata |
| `.inkt` | Textual | Human-readable disassembly (like WAT for WASM) |
| `.inkl` | Locale overlay | Per-scope replacement line tables for a specific locale |

## `.inkb` format

### Header (16 bytes)

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Magic: `INKB` |
| 4 | 2 | Version: u16 LE (currently 1) |
| 6 | 1 | Section count: u8 (10) |
| 7 | 1 | Reserved: 0x00 |
| 8 | 4 | File size: u32 LE |
| 12 | 4 | Content checksum: u32 LE (CRC-32) |

### Offset table

Immediately after the 16-byte preamble. Each entry is 8 bytes:

```text
Offset  Size   Field
------  -----  ------
0       1      SectionKind: u8 tag
1       3      Reserved: 0x00 0x00 0x00
4       4      Offset: u32 LE (byte offset from start of file to section data)
```

With 10 sections, the offset table occupies 80 bytes (10 x 8). The total header size is 96 bytes (16 + 80). Each section's size is computed from the difference between its offset and the next section's offset (or the file size for the last section).

### Sections

| Tag | SectionKind | Contents |
|-----|-------------|----------|
| `0x01` | **NameTable** | Interned name strings. Each entry is a length-prefixed UTF-8 string (`u16` LE byte count + bytes). Referenced by `NameId(u16)` indices throughout other sections. |
| `0x02` | **Variables** | Global variable definitions. Each entry: `DefinitionId` + `NameId` + `ValueType` tag + encoded default value + mutability flag. |
| `0x03` | **ListDefs** | List (enum) type definitions. Each entry: `DefinitionId` + `NameId` + item count + `(NameId, i32 ordinal)` pairs. |
| `0x04` | **ListItems** | Individual list item definitions. Each entry: `DefinitionId` + origin `DefinitionId` + `i32` ordinal + `NameId`. |
| `0x05` | **Externals** | External function declarations. Each entry: `DefinitionId` + `NameId` + `u8` arg count + optional fallback `DefinitionId`. |
| `0x06` | **Containers** | Bytecode containers. Each entry: `DefinitionId` + `u64` content hash + `CountingFlags` byte + `i32` path hash + `u32` bytecode length + raw bytecode bytes. |
| `0x07` | **LineTables** | Per-scope line tables for output text (one per knot/stitch/root). Each scope's table: `DefinitionId` (scope) + line count + encoded line entries (plain strings or interpolation templates). |
| `0x08` | **Labels** | Address definitions (divert targets). Each entry: `DefinitionId` (address) + `DefinitionId` (container) + `u32` byte offset. |
| `0x09` | **ListLiterals** | Pre-computed list literal values used by `PushList` instructions. Each entry: item count + `DefinitionId` items + origin count + `DefinitionId` origins. |
| `0x0A` | **AddressPaths** | Maps qualified author paths (`knot`, `knot.stitch`, `knot.stitch.label`) to `DefinitionId`s, so `Program::find_address` can resolve a name to a starting position. |

### Encoding conventions

- All multi-byte integers are **little-endian**.
- `DefinitionId` values are encoded as raw `u64` LE (8 bytes).
- Strings in the name table are length-prefixed: `u16` LE byte count followed by UTF-8 bytes.
- Sections are self-contained — the runtime can deserialize them independently. The `read_inkb` function parses all sections into a complete `StoryData` for linking.

### Versioning & compatibility

The header carries a `u16` version, and the reader **rejects any version it doesn't recognize** rather than guessing — every change to the byte layout bumps it.

`.inkb` and `.inkl` are **build artifacts**: regenerated from `.ink` on every compile, not meant to be hand-edited or shipped independently of the compiler that produced them. So the toolchain keeps a single current version and **recompiles on mismatch** — there are no multi-version readers. If you bundle compiled bytes with a game, treat them as version-locked to the brink release you built with, and recompile when you upgrade.

This is separate from **save files**, which *are* designed to survive toolchain upgrades: loading a save reports what it couldn't apply (e.g. a variable a newer story removed) rather than failing outright — see the [Runtime API](./runtime-api.md). Program metadata like container layout never affects save compatibility, since saves reference variables and visit counts by definition id, not by byte offset.

## `.inkt` format

The textual format is a human-readable disassembly of `.inkb`. Container paths appear as labels, opcodes as mnemonics with operands. Useful for debugging compiler output and diffing two compilations side-by-side.

```
=== container $01_abcdef1234567 (my_knot) ===
  0000: PushInt 42
  0004: SetGlobal $02_1234567abcdef
  000c: EmitLine 0
  000e: Done
```

## `.inkl` format

Locale overlays replace per-scope line tables without touching bytecode. A
decoded `.inkl` is a `LocaleData`:

- BCP 47 `locale_tag` and the base `.inkb` checksum (`base_checksum`), so a
  mismatched overlay is rejected before it can render garbage.
- `line_tables`: per-scope replacement tables (`LocaleScopeTable`) keyed by
  scope `DefinitionId`.
- Only scopes present in the `.inkl` are replaced; the rest fall back to base
  text under `LocaleMode::Overlay` (or error under `LocaleMode::Strict`).

The runtime applies an overlay with `brink_runtime::apply_locale`. Build `.inkl`
files with `brink compile-locale` (see the Localization section).
