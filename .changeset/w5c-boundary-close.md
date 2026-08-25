---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Worker architecture W5c close-out (`docs/editor-worker-spec.md` §14): the deferred-refresh rebuilds read **worker-fed stashes** instead of pulling analysis on the main thread — `DocHandle` gains per-surface stashes (projection, hints, widgets, folds; dirty-bit guarded so a stash is never served across an edit it predates) and a refined-token worker plane (`refreshRefined`: replica manifest + changed slices only, assembled synchronously at rebuild time); the compile-delivery overlay refresh fetches its projection first and dispatches on landing. Desktop export awaits the async compile landing (fixing the W4-era regression where it read story bytes synchronously after `compile.run`). A new lexical boundary guard pins every surviving main-thread analysis call to a documented allowlist — the one-shot family (goto/rename/symbols/search-cards) stays main-side at incremental cost, tracked by #3110. The synchronous session survives as content store + the in-process fallback road (decision log 2026-08-25).
