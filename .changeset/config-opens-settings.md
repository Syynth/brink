---
"@brink-lang/studio": minor
---

Clicking `brink.toml` in the Binder now opens the **Settings** takeover, in
every editor view. Settings gains a "Project" section carrying the whole
config document — the structured form and the raw text beneath it.

Continuous view renders the project's manuscript and deliberately excludes
`brink.toml` from it, so the config file was simply unreachable there
(#3166). Routing to Settings answers that once for every view rather than
per-view.
