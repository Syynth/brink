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
mounted at all, so a symbol declared in a mounted stdlib module (e.g.
`std/conventions/screenplay.brink`) was absent from the project-wide
symbol index in the playground/studio editor, unlike a real compile.
Note: `std::`-qualified paths (`use std::conventions::screenplay`) are not
yet resolvable at all — that needs #1582's pub marker and #2167's
closure-scoped confinement, neither of which has landed — so this mount
does not yet make hover/completion/goto-definition resolve *through* a
`std::` path; it only makes the mounted symbols visible to the same
project-wide indexing every other file gets. Mounted stdlib files are also
excluded from client-facing listings (`list_files`/`project_outline`/
`story_graph`) so they don't appear as phantom rows in the Binder or
project-wide search.
