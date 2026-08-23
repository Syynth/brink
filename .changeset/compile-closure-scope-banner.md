---
"@brink-lang/web": minor
"@brink-lang/studio": minor
---

Out-of-scope editor banner (#3017): the compile closure is now surfaced
through the wasm boundary — `EditorSession.compilation_closure()` /
`EditorSessionHandle.getCompilationClosure()` return the project-relative
paths of the exact file set the latest compile built from (empty before
any compile; read-only). The studio renders a banner above the editor of
any source file outside that closure ("Not included in the project —
nothing INCLUDEs this file, so it is not analyzed"), with a one-click
"Add INCLUDE to <entry>" action for the ink flow, plus a "— file not
analyzed" status-bar note. Absent diagnostics look identical to clean
diagnostics; this makes the difference visible.
