---
"@brink-lang/web": patch
---

A played line's `source` (and a choice's) is now where it was emitted,
not where its text first appears. Line-table deduplication shares one
entry across every repeat of a text, so a repeated speaker cue dragged a
line's provenance back to the cue's first use — the Player's Follow and
provenance chips banded the whole scene since then. Resolved through the
compile's debug info; without debug info the old line-table location is
the fallback (#3670).
