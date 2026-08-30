---
"@brink-lang/studio": patch
"@brink-lang/web": patch
---

Player feedback round (RULED 2026-08-30): saves carry the STRUCTURAL transcript (the runtime's part stream as human-readable JSON — `WebSession.exportTranscript`/`renderTranscript`) and loads, forks, and hot-reload migrations re-render it against the CURRENT compile, so an edited line's restored row shows the edited prose; fast-forward is a one-shot ContinueMaximally (run to the next choice/stop, paced per settings, no sticky auto mode); Player toolbar sub-sections collapse one group at a time into a ⋯ overflow menu when the pane is too narrow, with hysteresis on re-expansion.
