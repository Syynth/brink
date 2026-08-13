---
"@brink-lang/studio": patch
---

`file.save`/`file.saveAll` no longer re-baseline a path against content a
host write never persisted (issue #2426). Both commands snapshot the
content they're about to write and, once the host save resolves, only mark
a path clean if its current session content still matches the snapshot. A
path edited while its host write was still in flight stays dirty and
surfaces a "…changed while saving — still unsaved" warning instead of a
false "Saved" notice; `file.saveAll`'s "Saved N files" count and
`api.getDirtyFiles()`/`StudioPublicState.dirtyFiles` reflect only the
verified subset.
