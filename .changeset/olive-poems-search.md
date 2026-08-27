---
"@brink-lang/studio": patch
---

Navigation works in Continuous view: it scrolls. Clicking a file in the
Binder, a search result, a Problem, or a go-to-definition now moves the
manuscript to the target line, clear of the sticky heading, instead of doing
nothing visible. Clicking a knot or stitch in the Binder's structure mode
works too — those name a symbol, which this view resolves to a position
inside the file's section rather than to a separate document it does not
render. Re-navigating to somewhere in the file you are already in scrolls as
well.
