---
"@brink-lang/web": patch
---

Fix (issue #1991): the native `.brink` surface's `~ stmt` content-ground
line escape ("charter §8.2, RULED 2026-07-23: ink's logic line, kept")
used to compile clean with zero diagnostics and silently print as literal
story text, never running the statement — `~ n = 5` printed `~ n = 5` to
the reader and left `n` unchanged. `~ stmt` now parses and lowers to a
real assignment (`=`/`+=`/`-=`) or a bare expression statement (e.g. a
function call), the same as the ink-dialect frontend's own logic line.
Observable through `@brink-lang/web` since the wasm package re-exports the
native compiler/runtime pipeline.
