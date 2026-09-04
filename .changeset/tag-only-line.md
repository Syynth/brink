---
"@brink-lang/web": patch
---

A tag-only line (`# tag` on a line of its own) no longer produces a
blank line: its tags attach to the next delivered line, as in ink
(#3534). Previously the tags were lost.
