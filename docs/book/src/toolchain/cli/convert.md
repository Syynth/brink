# brink convert

Convert a compiled story between brink's own formats — binary (`.inkb`) and
textual disassembly (`.inkt`). It also accepts raw `.ink` source, which is
compiled in-memory first (equivalent to `brink compile`).

Input format is inferred from the file extension; output defaults to `.inkt` on stdout.

```sh
brink convert <INPUT> [--output <OUTPUT>]
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--output <FILE>` / `-o` | stdout (.inkt) | Output file path. Format inferred from extension. |

## Supported formats

| Extension | Format | Description |
|-----------|--------|-------------|
| `.ink` | ink source | Compiled in-memory via the native pipeline (input only) |
| `.inkb` | Binary bytecode | brink's native binary format |
| `.inkt` | Textual bytecode | Human-readable disassembly |

## Examples

```sh
# Disassemble binary to readable bytecode (stdout)
brink convert story.inkb

# Round-trip textual bytecode back to binary
brink convert story.inkt -o story.inkb

# Disassemble binary to text
brink convert story.inkb -o story.inkt
```
