---
"@brink-lang/web": patch
---

A temp written after `<- thread` now survives the next choice: the call
stack's fork-snapshot cache is invalidated by every mutable frame access,
so a choice's fork always sees the frames as they are (#3528).
