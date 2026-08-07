---
"@brink-lang/studio": minor
---

`StudioApi` gains `getStoryBytes()` — the latest successful compile's story
bytes, or `null` when the latest compile failed (issue #2391, "Export Story
(.inkb)"). Pull-on-demand, like `getFiles()`/`getDirtyFiles()`: bytes are
big and change on every compile, so they stay out of `StudioPublicState`. A
host drives `dispatch("compile.run")` first (the same surface the Player's
Run button uses) to get a fresh compile, then reads this to get the
artifact. Purely additive — no existing `StudioApi` behavior changes.
