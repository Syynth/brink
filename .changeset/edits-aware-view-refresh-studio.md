---
"@brink-lang/studio": patch
---

Applying a Safe fix from the Problems row no longer scrolls the editor away
from the edit (#3496): `applyMoveResult` threads a single `Fix`'s own
precise `edits` through to the document layer's new
`applyEditsToViews`, so the mounted view gets a minimal change instead of a
whole-document reload. A structural op (rename/move/promote/demote/reorder)
has no such precise edit list and still benefits from the document layer's
own minimal-diff fallback.
