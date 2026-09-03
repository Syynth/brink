---
"@brink-lang/studio": patch
---

Prose checking no longer freezes the editor. The studio's `ProseChecker`
now runs the `brink-prose` wasm module inside a Web Worker, which lazily
imports it there on the first check — so an embedder that never checks
prose still downloads nothing, and a check no longer blocks input for
the length of the document. A check superseded by a newer edit is
dropped before it is posted rather than queued behind the one in flight.
Environments with no `Worker` (jsdom, a bundler that leaves the
`new URL(..., import.meta.url)` shape alone) and a crashed worker fall
back to the previous in-process road, so checking degrades in speed
rather than stopping (#3491).
