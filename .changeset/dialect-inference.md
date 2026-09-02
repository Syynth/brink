---
"@brink-lang/dialect": minor
---

Rule inference for the teach-by-example Conventions editor (#3409):
`inferDialect(markedLines)` proposes a dialect from lines the author has
marked (cue / dialogue / action / narration / parenthetical) using a
small, fixed set of shapes — an affix marker with `<>` glue, the ink
docs' `Name: text` line, an all-caps screenplay cue — rejects any shape a
differently-marked line also matches, verifies by re-parsing every line,
and reports plain-language `learned` rules with supporting line indices
plus `decisions` for what the shapes cannot settle. Builds on the
shipped `at-cue` preset whenever the result is affix-expressible.

Also the `[dialogue]` table as TypeScript: `DialogueConfig`,
`dialectFromConfig` (mirrors the compiler's resolver, presets + overlays
+ `run-ends-at`), `emittedForAffix`/`affixElement` (mirror the Rust
builders), and `toDialogueConfig` — the verified projection of a dialect
back to the table form, `null` when only the file form can hold it.
