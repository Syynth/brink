# brink play

Play an ink story interactively in the terminal.

```sh
brink play [OPTIONS] <FILE>
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--speed <N>` | `30` | Typewriter speed in characters per second (0 = instant) |
| `--input <FILE>` | — | Read choice inputs from a file (batch mode) |

## Interactive mode

When run in a terminal, `brink play` launches a full-screen TUI with:

<!-- TODO: describe the TUI features
  - Typewriter text reveal for both passage text and choices
  - Arrow-key choice selection with wrap-around
  - Tab to switch focus between story scrollback and choice panel
  - History fades in above the current passage
  - Stable layout (no jumping)
-->

### Key bindings

| Key | Story panel | Choice panel |
|-----|------------|--------------|
| `Space` | Skip typewriter | Skip typewriter |
| `↑/↓` | Scroll history | Select choice |
| `Enter` | — | Confirm choice |
| `Tab` | Focus choices | Focus story |
| `q` | Quit | Quit |

## Batch mode

When stdin is piped or `--input` is provided, the TUI is bypassed and choices are read as line-delimited 1-indexed integers.

```sh
# Pipe choices
printf "1\n3\n" | brink play story.ink.json

# Read choices from a file
brink play story.ink.json --input choices.txt
```

<!-- TODO: explain batch output format -->
