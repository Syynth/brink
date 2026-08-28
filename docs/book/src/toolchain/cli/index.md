# The CLI

`brink-cli` (the `brink` binary) provides commands for compiling, playing,
localizing, and formatting ink stories.

```sh
brink --help
```

## Commands

| Command | Description |
|---------|-------------|
| [`compile`](./compile.md) | Compile `.ink` source to `.inkb` or `.inkt` |
| [`convert`](./convert.md) | Convert between ink formats (`.inkb`, `.inkt`) |
| [`play`](./play.md) | Play an ink story interactively or in batch mode |
| [`debug`](./debug.md) | Step through a story: breakpoints, stepping, locals, call stack |
| [`ide`](./ide.md) | Scriptable IDE queries & refactors (navigation, references, rename, structural refactors) |
| [`export-xliff`](../localization/xliff.md) | Export a story's line tables as an XLIFF 2.0 file for translation |
| [`compile-locale`](../localization/xliff.md) | Compile a translated XLIFF into a `.inkl` locale overlay |
| [`regenerate-xliff`](../localization/xliff.md) | Update an XLIFF after recompilation, preserving translations |
| `fmt` | Format `.ink` source files (`--check`, `--stdin`) |
| `replay` | Re-render a saved `.brkt` transcript against a story (optionally a locale) |
