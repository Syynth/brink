# brink play

Play an ink story interactively in the terminal.

```sh
brink play [OPTIONS] <FILE>
```

Accepts `.inkb`, `.ink.json`, or `.inkt` files.

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--speed <N>` / `-s` | `30` | Typewriter speed in characters per second (0 = instant) |
| `--input <FILE>` / `-i` | -- | Read choice inputs from a file (batch mode) |

## Interactive mode

When run in a terminal, `brink play` launches a TUI with typewriter text reveal and arrow-key choice selection.

### Key bindings

| Key | Story panel | Choice panel |
|-----|------------|--------------|
| `Space` | Skip typewriter | Skip typewriter |
| `Up/Down` | Scroll history | Select choice |
| `Enter` | -- | Confirm choice |
| `Tab` | Focus choices | Focus story |
| `q` | Quit | Quit |

## Batch mode

When stdin is piped or `--input` is provided, the TUI is bypassed and choices are read as line-delimited 1-indexed integers.

```sh
# Pipe choices
printf "1\n3\n" | brink play story.inkb

# Read choices from a file
brink play story.inkb -i choices.txt
```

In batch mode, story text and choices are printed to stdout as plain text.
