---
"@brink-lang/web": patch
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Peek: hovering Continue or a choice card in the Player forks the live story (`StorySessionHandle.speculate()`, new, at the exact position), runs one continue call on the fork and highlights what it would hit in the editor with a dashed `peek` bar; `SpeculationHandle.currentPath()` reports the fork's knot. Execution highlights split into tint (state) and bar (attention) channels: `follow`/`hover`/`peek` are bar-only and stack on a tinted line, and the cursor's active line gets its own colour on a tinted line.
