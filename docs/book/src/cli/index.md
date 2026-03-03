# The CLI

`brink-cli` provides commands for compiling and playing ink stories.

<!-- TODO: global flags, help output -->

```sh
brink --help
```

## Commands

| Command | Description |
|---------|-------------|
| [`convert`](./convert.md) | Convert between ink formats (`.ink.json`, `.inkb`, `.inkt`) |
| [`play`](./play.md) | Play an ink story interactively |

<!-- TODO: future commands from the spec:
  - `compile` — compile .ink source to .inkb (requires brink-compiler)
  - `generate-locale` — extract translatable lines to XLIFF
  - `compile-locale` — compile translated XLIFF to .inkl overlay
-->
