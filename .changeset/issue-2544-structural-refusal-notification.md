---
"@brink-lang/studio": patch
---

A refused reorder/move/promote/demote now tells you why, instead of vanishing
(issue #2544). `dispatchSymbolAction`'s seven structural-op branches
(`reorderStitch`, `reorderKnot`, `reorderStitches`, `reorderKnots`,
`moveStitch`, `promoteStitch`, `demoteKnot`) ended with
`if (result.ok && result.path) { await applyMoveResult(...) }` and no `else`
— a refused `StructuralResult` (`ok: false`) applied nothing, correctly, but
also raised nothing, so the user had no way to tell a refusal apart from a
no-op. Each refusal now raises an error-severity notification tagged
`binder`, through the same `notifyStructuralRefusal` helper the rename
surfaces already use (#2528/#2543) — one reporting contract, not a second
style.

`performSymbolRename`'s `!session` early return had the same gap in a worse
shape: it returned `{ applied: false, diagnostics: [] }` with neither
`applied` nor `error` set, so `SymbolRenamePrompt` fell into its breakage-
report branch with an EMPTY report — rendering "would break 0 places" with a
live **Force rename** button whose retry hit the same branch again. It now
sets `error` too, so the prompt closes and the same error notification
fires, instead of asserting a rename is unsafe when in truth no session was
ever bound.

Refused ops still push no undo entry (unchanged — nothing was written).
