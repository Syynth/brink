# @brink-lang/dialect

## 0.18.0

### Minor Changes

- 12f08ab: Rule inference for the teach-by-example Conventions editor (#3409):
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

  - `run-ends-at`), `emittedForAffix`/`affixElement` (mirror the Rust
    builders), and `toDialogueConfig` — the verified projection of a dialect
    back to the table form, `null` when only the file form can hold it.

- 3729f92: `@brink-lang/dialect` (RULED 2026-08-30, "Engines consume the RESOLVED dialect as a compile output"): the dialogue-dialect artifact, validator, `ResolvedDialect`, `DialectParser`, `extendDialect`, `detectCast` and the `runsOf` run rule move into a pure-TypeScript package with no runtime dependencies, so a game engine can read its project's conventions without depending on the editor. `@brink-lang/editor` re-exports the whole surface unchanged (its one editor-coupled helper is now `convertibleShapesOf`). `brink compile` writes the project's resolved dialect as `<story>.dialect.json` beside the compiled story when `brink.toml` declares `[dialogue]`, and the desktop app's Export Story does the same. Book: _Conventions for Your Engine_.
