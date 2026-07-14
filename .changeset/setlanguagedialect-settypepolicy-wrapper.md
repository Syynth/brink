---
"@brink-lang/web": minor
---

Added `EditorSessionHandle.setLanguageDialect(value)` and
`EditorSessionHandle.setTypePolicy(value)` (#693), mirroring
`setSemanticTypeCheck`/`setExternalCheck`. The raw
`WasmEditorSession.set_language_dialect` (#611) and
`WasmEditorSession.set_type_policy` (#660) wasm levers existed, but
`EditorSessionHandle` — the surface `@brink-lang/web` consumers actually
use — exposed neither, so no JS caller could opt into the brink dialect or
the typed-mode policy at all (every new construct raised `E051` with no
opt-in path). `setLanguageDialect("brink" | "strict-ink")` and
`setTypePolicy("strict" | "gradual")` now delegate to the wasm session and
bump the generation counter, same as every other mutating call on the
handle.
