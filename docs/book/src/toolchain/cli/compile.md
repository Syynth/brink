# brink compile

Compile `.ink` source files to bytecode. The input file is the story's entry point; `INCLUDE` directives are resolved automatically.

```sh
brink compile <INPUT> [--output <OUTPUT>]
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--output <FILE>` / `-o` | stdout | Output file path. Format inferred from extension. |

Output format is determined by the file extension:

| Extension | Format |
|-----------|--------|
| `.inkb` | Binary bytecode (production format) |
| `.inkt` | Human-readable text dump (debugging) |

When no `-o` flag is given, `.inkt` is printed to stdout.

## Examples

```sh
# Compile to binary
brink compile story.ink -o story.inkb

# Debug dump to file
brink compile story.ink -o story.inkt

# Debug dump to stdout
brink compile story.ink
```
