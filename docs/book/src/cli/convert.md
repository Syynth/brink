# brink convert

Convert between ink formats. This uses the **converter** pipeline (`brink-converter`), which processes inklecate's JSON output rather than compiling from `.ink` source. Use `brink compile` for native compilation.

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
| `.ink.json` | inklecate JSON | Output from the reference ink compiler |
| `.inkb` | Binary bytecode | Brink's native binary format |
| `.inkt` | Textual bytecode | Human-readable disassembly |

## Examples

```sh
# Disassemble ink.json to readable bytecode (stdout)
brink convert story.ink.json

# Convert ink.json to binary
brink convert story.ink.json -o story.inkb

# Disassemble binary to text
brink convert story.inkb -o story.inkt
```
