---
"@brink-lang/web": patch
---

`Choice.index` now numbers the visible choices contiguously, matching
ink's `currentChoices` — an invisible fallback (`+ ->`) no longer occupies
an index even when it sits ahead of a visible choice (a thread's fallback
merged before the main flow's choices printed `0, 2` where ink prints
`0, 1`). `choose(index)` takes that same number, as before (#3527).
