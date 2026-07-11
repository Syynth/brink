---
"@brink-lang/web": patch
---

Added `EditorSessionHandle.setSemanticTypeCheck(level)` (#532), mirroring
`setExternalCheck`. Previously `WasmEditorSession.set_semantic_type_check`
was only reachable on the raw wasm session, which `EditorSessionHandle`
holds in a private field — the `@brink-lang/web` public wrapper had no
method to reach it, so the severity lever was dead code for consumers of
the package. `setSemanticTypeCheck("tolerant" | "error")` now delegates to
the wasm session and bumps the generation counter, same as every other
mutating call on the handle.
