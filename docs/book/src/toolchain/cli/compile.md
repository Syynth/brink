# brink compile

Compile `.ink` source files to bytecode. The input file is the story's entry point; `INCLUDE` directives are resolved automatically.

```sh
brink compile <INPUT> [--output <OUTPUT>] [--dialect <strict-ink|brink>] [--types <gradual|strict>]
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--output <FILE>` / `-o` | stdout | Output file path. Format inferred from extension. |
| `--dialect <DIALECT>` | `strict-ink` (or a discovered [`brink.toml`](../project-config.md)) | `strict-ink` rejects [brink-dialect extension syntax](../dialect/index.md) (`~ { … }` blocks, `#[…]`/`#{…}` literals, indexing) with a targeted diagnostic; `brink` accepts it. Mount-time only — never embedded in the compiled output. |
| `--types <POLICY>` | `gradual` (or a discovered [`brink.toml`](../project-config.md)) | `gradual` is today's behavior; `strict` requires `--dialect brink` and makes `Unknown`/`Conflicted`-escaping inference a compile error. Mount-time only. |

`--dialect`/`--types` are the highest-priority source: if you pass one, it wins over a project's `brink.toml`, which in turn wins over the plain defaults above. See [Project Settings](../project-config.md) for the file's discovery rule and precedence.

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
