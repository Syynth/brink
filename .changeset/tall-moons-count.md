---
"@brink-lang/studio": patch
---

Add Continuous view, the third editor view: every file in the project stacked
in binder order as one manuscript, with a heading between each and a single
scroller carrying you across file boundaries. Files are stacked as separate
editors rather than concatenated into one document, so each keeps its own
wasm document handle and diagnostics, tokens and completion stay per-file and
correct. Order comes from the same `.binder.json` sidecar the Binder tree
uses, so the manuscript reads in exactly the order the Binder shows.
Selectable from Settings or the "View: Continuous" command.
