---
"@brink-lang/studio": patch
---

Hot reload (W15/#3308, spec §F8 REVISED). Edits during play reach the
running Player: on every successful compile the live session migrates —
journal replay when it lands cleanly (exact position and transcript
survive), and the W14 checkpoint road (snapshot → fresh session →
loadState → divert to the recorded knot) when replay diverges, fails,
throws, or reports "clean" while regressing the turn count (the
journal-bypass reality of debug-driven sessions, #3335). Globals, visit
counts, and the turn index survive the edit; a lossy migration surfaces
the LoadReport as a "Reloaded — …" transcript notice; the status chip
flashes a brief "Reloaded". Degraded mode is demoted to the fallback
(failing compile keeps the old program; the supersession is recorded in
live-inspector-spec §5).
