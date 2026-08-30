---
"@brink-lang/studio": patch
---

Player appearance settings (W13/#3306, RULED). Settings → Player gains a
font-size knob for the Player's prose — its own `--bs-player-font-size`
variable on the `--bs-editor-font-size` precedent (the reading surface's
size is not the UI's size), falling back to the app type scale at the
default 0. Stepping below the readable floor resets to follow-scale
rather than sticking at a clamp. Persisted with the paced-reveal
setting; room to grow (line spacing, face) without re-ruling.
