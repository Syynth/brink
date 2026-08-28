---
"@brink-lang/web": patch
"@brink-lang/studio": patch
---

Add a per-session debug-info compile toggle (#3229).

`EditorSessionHandle.setDebugInfoEnabled(enabled)` / `debugInfoEnabled()`
control whether this session's compiles emit the D6 `DebugInfo` section.
Off by default, matching the ship policy; a host turns it on for the
session it is about to debug and off when that session ends.

This is what makes the debugger reachable at all: without the section, the
runtime position, locals table and program→source resolver landed by
D4/D6/D7/D9 resolve to nothing, because the studio's live session runs on
exactly the bytes the editor session compiles.

The caller must recompile for the flag to take effect — it governs what the
next compile emits. Toggling bumps the session generation, so the next
compile is a real one. The studio store exposes it as `setDebugInfoEnabled`,
which recompiles for you.
