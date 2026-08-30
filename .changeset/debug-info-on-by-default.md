---
"@brink-lang/web": patch
"@brink-lang/studio": patch
---

Debug info is on by default for studio compiles (W1/#3294, ruled
2026-08-29). A fresh `EditorSession` (and the studio store mirroring it)
now emits the `DebugInfo` section on every compile, so breakpoints bind
and positions resolve from the studio's own bytes with no toggle touched;
`setDebugInfoEnabled(false)` remains the opt-out, now surfaced as an
App-settings "Debugging" section ("Emit debug info in studio compiles")
persisted per machine. Release export and the CLI default are unchanged.
