---
"@brink-lang/editor": patch
---

Fix gutter clicks below the fold after the detached-gutters change (#3119): the container was pinned `bottom: 0`, capping its box at one viewport height while CodeMirror keeps positioning markers from the document top — so every gutter marker below the fold fell outside its own container's box and silently refused clicks (the ▶ play affordance, fold arrows, host gutter markers) while still painting normally. The box now grows to contain its markers.
