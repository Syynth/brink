---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

`brink.toml` is no longer inert (issue #2324). `EditorSessionHandle.applyProjectConfig`/`discoverProjectConfig`
(#1005, #1414) were exposed and unit-tested but nothing outside test files ever called either, so every
`[project]`/`[lints]` key in a mounted project's `brink.toml` was silently ignored end to end.

`ProjectSession` (`@brink-lang/editor`) now calls `discoverProjectConfig` — chosen over `applyProjectConfig`
because it walks the session's own already-loaded documents, so no host-specific directory-walk/read code is
needed — once during `initialize()` (before the first analysis) and again whenever a `brink.toml` anywhere in
the session is created, edited, renamed into/out of, or externally rewritten. A new optional
`ProjectSessionOptions.onProjectConfigWarnings` callback forwards the unrecognized-key/lint-code warnings from
each call.

`mountStudio` (`@brink-lang/studio`) wires that callback into the Output tool window, so a typo'd or
unrecognized `brink.toml` key is now visible instead of silently dropped. `[project] entry` is one such key:
`brink_project_config::ProjectConfig` has no field for it at all (verified against
`crates/internal/brink-project-config/src/lib.rs`), so it always reports as an unrecognized key — `mountStudio`'s
explicit `entryFile` argument remains the only thing that decides the compiled entry file; there was nothing at
the wasm-session layer for it to conflict with.
