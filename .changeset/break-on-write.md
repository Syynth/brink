---
"@brink-lang/studio": patch
"@brink-lang/web": patch
---

Break-on-write data breakpoints (W18, spec §F6 RULED): right-click a global in the Debugger panel's Variables section → "Break on write" — a write to the watched global pauses the run AND Continue tiers at the writing instruction, with the watchpoint named in the stop reason (the Player chip reads "Paused on write — gold"). Armed watchpoints are listed in the Breakpoints section with the diamond glyph (`◆ gold — on write`), checkbox enable/disable and remove like position breakpoints, stored by author name so they survive hot reloads. `WebSession` gains `debugWatchpointAdd`/`debugWatchpointRemove`/`debugWatchpoints`; the watchpoint stop reason now carries the global's `name`.
