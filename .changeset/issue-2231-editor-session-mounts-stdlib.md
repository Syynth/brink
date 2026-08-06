---
"@brink-lang/web": patch
---

`EditorSession` (the wasm/studio editor session `@brink-lang/web` exposes)
now mounts the shared stdlib — the third producer named by issue #2231, the
`brink-web` sibling of #2198/#2225 (`brink-cli`/`brink-lsp`). Every
`(root-relative key, source text)` pair from
`brink_environment::stdlib_sources()` is fed through `IdeSession::
update_source` at construction, before any dialect/type-policy setter runs,
mirroring `Project::ide_session()`'s ordering precedent. Previously
`EditorSession::new()` built a bare `IdeSession::new()` with no stdlib
mounted at all, so a project opened in the playground/studio editor
disagreed with a real compile the moment it referenced anything under
`std::` — hover, completions, and goto-definition on stdlib symbols now
resolve.
