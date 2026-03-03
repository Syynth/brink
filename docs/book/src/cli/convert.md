# brink convert

Convert between ink formats. Input format is inferred from the file extension; output defaults to `.inkt` (textual bytecode) on stdout.

```sh
brink convert <INPUT> [--output <OUTPUT>]
```

## Supported formats

| Extension | Format | Description |
|-----------|--------|-------------|
| `.ink.json` | inklecate JSON | Output from the reference ink compiler |
| `.inkb` | Binary bytecode | Brink's native binary format |
| `.inkt` | Textual bytecode | Human-readable disassembly (like WAT for WASM) |

## Examples

<!-- TODO: show conversion between all format pairs -->
<!-- TODO: explain the .ink.json → .inkb bootstrap path (via brink-converter) -->

```sh
# Disassemble an ink.json to readable bytecode
brink convert story.ink.json

# Convert ink.json to binary
brink convert story.ink.json --output story.inkb

# Disassemble binary to text
brink convert story.inkb --output story.inkt
```
