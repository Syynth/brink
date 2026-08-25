---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Structural computes ride the async session facade (editor worker architecture W2e, `docs/editor-worker-spec.md`): `ProjectSession` gains `structuralQuery` — an interactive-priority client query for the compute-only structural ops (`renameFile`, `renameDir`, `moveStitch`, `promoteStitch`, `demoteKnot`, `renameSymbol`, `renameSymbolAt`; they return new sources + a breakage report and mutate nothing, so query semantics fit exactly) — and the studio's gated-op runner awaits it, mapping a destroy-during-queue cancellation to the same swallowed result as a destroy-during-defer. The `ProjectSession` file-lifecycle mutations deliberately stay synchronous until the transport flips (sync reads couple to them; recorded in the spec). The paint-path enrolment guard now matches both the facade call shape and the raw wasm call shape, so a raw gated call reappearing anywhere still fails it.
