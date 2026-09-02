---
"@brink-lang/studio": minor
"@brink-lang/editor": patch
---

Settings → Conventions is now the teach-by-example editor (#3411, ruled
2026-09-02): pull a passage through the Player launcher's knot/stitch
typeahead (or paste lines), mark each line — Cue, Dialogue, Action,
Narration, Aside — and the studio shows the rules it learned in plain
words with the lines that support each, what it could not settle, and
how the passage reads in the Player under those rules. "Use these rules"
writes the `[dialogue]` section of `brink.toml` (the `at-cue` recipe plus
your rows when that fits, otherwise a `dialect.json` the section points
at) and asks before replacing a section it did not write.

`@brink-lang/editor` re-exports the inference and `[dialogue]`-table
helpers from `@brink-lang/dialect` (`inferDialect`, `dialectFromConfig`,
`toDialogueConfig`, …).
