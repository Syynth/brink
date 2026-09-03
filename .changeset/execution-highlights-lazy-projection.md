---
"@brink-lang/studio": patch
---

The execution-highlight policy no longer pulls the file's HIR projection
unless it can use it. `executionHighlightsFor` now accepts the projection as
a thunk, and only the choice-point branch resolves it — "no session",
"ended", "error", "degraded" and plain "running" all answer without touching
it. The studio passes `() => documents.getHirProjection(path)`, so the
synchronous whole-document `getHirSpansDoc` query that pull entails stops
running on every keystroke of an idle editor.

Passing a plain projection (or `null`) still works exactly as before.

The studio's own wiring of that seam is now the named export
`executionHighlightsHook(getState, getProjection)` instead of an arrow inlined
in `mountStudio` — the eager evaluation was a property of the call site, so
the call site is what had to become testable.
