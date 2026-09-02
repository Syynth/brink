---
"@brink-lang/dialect": minor
"@brink-lang/web": patch
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

`@brink-lang/dialect` (RULED 2026-08-30, "Engines consume the RESOLVED dialect as a compile output"): the dialogue-dialect artifact, validator, `ResolvedDialect`, `DialectParser`, `extendDialect`, `detectCast` and the `runsOf` run rule move into a pure-TypeScript package with no runtime dependencies, so a game engine can read its project's conventions without depending on the editor. `@brink-lang/editor` re-exports the whole surface unchanged (its one editor-coupled helper is now `convertibleShapesOf`). `brink compile` writes the project's resolved dialect as `<story>.dialect.json` beside the compiled story when `brink.toml` declares `[dialogue]`, and the desktop app's Export Story does the same. Book: *Conventions for Your Engine*.
