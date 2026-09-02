# Conventions for Your Engine

A project declares its dialogue conventions once, in `brink.toml`:

```toml
[dialogue]
preset = "at-cue"            # the shipped cue preset: `@NAME: <>` + chained dialogue
run-ends-at = ["character", "action"]

[[dialogue.elements]]
kind = "action"
prefix = ">"                 # `> text` is an action paragraph
```

The studio reads that declaration to classify lines while you write and to
fold delivered lines into speaker runs in its Player. Your **engine** can
read the very same conventions — without depending on the editor — because
`brink compile` writes the *resolved* dialect beside the compiled story:

```text
story.inkb
story.dialect.json     # the preset merged, affix sugar expanded
```

`story.dialect.json` is a derived product, like `story.inkb`: never hand-edit
it. The source of truth is `brink.toml`. (The desktop app's *Export Story*
writes the same pair.)

## Reading it in a game

Install the pure-TypeScript package (no runtime dependencies):

```sh
npm install @brink-lang/dialect
```

Then parse each line your story runtime emits and fold the lines into runs:

<!-- ts,no-check: `@brink-lang/dialect` reaches npm with the release that
     ships this chapter; the book's type-check installs PUBLISHED packages, and
     the snippet also stands in `choicesWerePresentedBefore` for the host's own
     turn tracking. -->
```ts,no-check
import { DialectParser, runsOf, type DialogueDialect } from "@brink-lang/dialect";
import raw from "./story.dialect.json";

const dialect = raw as DialogueDialect;
const parser = new DialectParser(dialect);

// `delivered` is whatever your runtime printed, one entry per line.
const lines = delivered.map((text, i) => ({
  segments: parser.parseEmitted(text),
  boundary: choicesWerePresentedBefore(i), // a turn boundary, if you track one
}));

for (const run of runsOf(lines, dialect)) {
  // run.kind   — "character" for a speaker run, "action", or null (narrative)
  // run.attrs  — carried groups, e.g. { speaker: "GRISWOLD" }
  // run.lines  — indices into `delivered` that belong to this run
}
```

`parseEmitted` splits one emitted line into segments (`@GRISWOLD: ` cue,
`(quietly)` parenthetical, remaining text); `runsOf` applies the dialect's
`run_ends_at` rule so a cue-less line after a cue is attributed to the last
speaker until an action, the next cue, or a choice boundary ends the run —
exactly the rule the studio Player uses, from the same file.

The ink text itself is untouched: the dialect never reaches the runtime,
and a project that declares no `[dialogue]` prints plain lines everywhere.
