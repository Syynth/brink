---
"@brink-lang/studio": patch
---

A knot/stitch rename that fails now tells you why (issue #2528). `performSymbolRename`
returned the rename op's error — "symbol not found" when the knot was edited away between
opening the context menu and confirming, "file not loaded", "cannot rename this symbol" —
and `SymbolRenamePrompt` closed on it exactly as it closed on success. Nothing else read
that error, so a failed rename was indistinguishable from a successful one: the prompt
disappeared and nothing was renamed. The failure now raises an error-severity notification
tagged `binder`, the same surface the file rename's failure path already uses and the same
source tag the rename's own success toast carries.
