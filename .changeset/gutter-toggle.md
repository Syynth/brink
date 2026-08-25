---
"@brink-lang/studio": patch
---

Editor gutter visibility toggle: a Settings checkbox and an editor context-menu item ("Hide Gutters" / "Show Gutters") hide all editor gutters (line numbers, structure rails, fold/play markers), persisted with the other editor settings. Besides the visual preference, hiding gutters removes a WebKit per-gutter-element layout cost (#3119), roughly halving felt keystroke latency again on large projects in the desktop app — the interim escape hatch until the structural fix lands.
