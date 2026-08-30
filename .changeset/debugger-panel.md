---
"@brink-lang/studio": patch
---

The Debugger panel (W8/#3301) — the StateView replacement (RULED:
redesign, not extension), in StateView's strip slot with a transport
mirror in its header so stepping works with the Player hidden. Sections:
Flows (the open-flows list lives here now — the status bar's
SessionPicker retires; selection scopes everything below) · Frames (an
interactive call stack: click selects, scopes the Variables section's
locals, reveals the frame's exact line, and draws the editor's accent
frame band) · Variables (selected frame's locals, then globals with the
step-diff highlight) · Breakpoints (checkbox enable/disable,
click-to-reveal, remove, disable-all/clear-all) · Story (the old
StateView's inspection content, collapsed). Placeholders keep the old
honesty: no session → start; no debug info → names the App setting.
