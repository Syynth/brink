---
"@brink-lang/web": minor
---

`brink.toml` — the project settings file for dialect + type policy (#1005).

New API surface: `EditorSessionHandle.applyProjectConfig(toml: string): string[]`.
Parses a `brink.toml`'s `[project] dialect`/`types` and applies it to the
session (dialect/type-policy warnings for unrecognized keys are returned,
never thrown). Call it once at session construction, before any explicit
`setLanguageDialect`/`setTypePolicy` — those calls always override the file
(the file supplies the default; explicit calls win), matching the new
`brink compile`/`brink ide` behavior: both now discover a `brink.toml`
(walking up from the entry file to the nearest ancestor) and apply its
`[project] dialect`/`types`, with `--dialect`/`--types` overriding the file
when actually passed. A missing `brink.toml` changes nothing — no
regression for existing consumers that don't ship one.
