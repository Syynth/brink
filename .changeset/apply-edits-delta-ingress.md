---
"@brink-lang/web": patch
"@brink-lang/editor": patch
---

Bounded-edit ingress (#3064 C1): `applyEditsDocument(doc, edits)` applies a CM6 change list Rust-side — the full document no longer crosses the wasm boundary on every keystroke, and the write is source-only: the fused eager whole-project analysis that `updateDocument` forced per keystroke (and that nothing on the keystroke path consumed — diagnostics are debounced-compile-driven) is no longer computed until something actually pulls it. The editor's element-type field uses the delta path automatically for single-range edits on file handles, falling back to the full push for multi-cursor batches, fragment views, and older wasm builds/mocks. `updateDocument` is unchanged for compatibility.
