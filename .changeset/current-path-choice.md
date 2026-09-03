---
"@brink-lang/web": patch
---

`currentPath()` answers inside a choice body, gather, or sequence branch with the knot or stitch that holds it, instead of `null` — so the first line after a choice is attributed to its knot.
