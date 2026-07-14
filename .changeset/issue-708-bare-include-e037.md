---
"@brink-lang/web": patch
---

Fix #708: a bare `INCLUDE` (no path) no longer aborts compilation with a
raw I/O error. Discovery now skips the empty include path the parser
already flagged, so the parser's `E037` ("expected file path")
diagnostic reaches the caller. Observable through `@brink-lang/web`:
`compile_project`/`compile_fragment`/editor compiles on a project
containing a bare `INCLUDE` now return `ok: false` with an `E037`
warning entry (placed on the offending line) instead of a generic
`error: "I/O error: file not found: …"` string with no source location.
