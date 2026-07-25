# brink compile

Compile `.ink` source files to bytecode. The input file is the story's entry point; `INCLUDE` directives are resolved automatically.

```sh
brink compile <INPUT> [--output <OUTPUT>] [--dialect <strict-ink|brink>] [--types <gradual|strict>] [-D <CODE>]... [--warn <CODE>]... [--allow <CODE>]...
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--output <FILE>` / `-o` | stdout | Output file path. Format inferred from extension. |
| `--dialect <DIALECT>` | `strict-ink` (or a discovered [`brink.toml`](../project-config.md)) | `strict-ink` rejects [brink-dialect extension syntax](../dialect/index.md) (`~ { … }` blocks, `#[…]`/`#{…}` literals, indexing) with a targeted diagnostic; `brink` accepts it. Mount-time only — never embedded in the compiled output. |
| `--types <POLICY>` | `gradual` (or a discovered [`brink.toml`](../project-config.md)) | `gradual` is today's behavior; `strict` requires `--dialect brink` and makes `Unknown`/`Conflicted`-escaping inference a compile error. Mount-time only. |
| `--deny <CODE>` / `-D <CODE>` | — (repeatable) | Promote diagnostic `CODE` to a hard compile error. Only codes whose *default* severity is `Warning` are overridable — see [Lint severity](../project-config.md#lint-severity). The special code `warnings` (`-D warnings`, mirroring `rustc`) is `deny-warnings`: promote every otherwise-`Warning` diagnostic to `Error`. |
| `--warn <CODE>` | — (repeatable) | Force `CODE` to `Warning`, still promotable by `-D warnings`/a project's `deny-warnings`. |
| `--allow <CODE>` | — (repeatable) | Force `CODE` to stay `Warning` even under `-D warnings`/`deny-warnings`. |

`--dialect`/`--types`/`--deny`/`--warn`/`--allow`/`-D warnings` are the highest-priority source: any of these, when actually passed, wins over a project's `brink.toml`, which in turn wins over the plain defaults above. See [Project Settings](../project-config.md) for the file's discovery rule and precedence.

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

# Fail the compile if E014 (an ordinarily-Warning code) fires
brink compile story.ink -D E014

# Fail the compile on ANY diagnostic that would otherwise be a warning
brink compile story.ink -D warnings

# ...but keep E063 as a warning even under -D warnings
brink compile story.ink -D warnings --allow E063
```
