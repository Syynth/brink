---
"@brink-lang/studio": patch
---

A hover card and a diagnostic on the same symbol now read as one panel
rather than a block bolted underneath.

They were already one tooltip with two sections, and both sections were
transparent — what made the diagnostic read as a different window was a 3px
severity rail no other row had, and a different padding to accommodate it.
The rail is gone and the padding matches the card's rows exactly, so
`warning` sits in the same column as `knot` and `effects`.

Severity is carried by the label word instead, which is what it was added
for: it survives a colourblind reader and a screenshot pasted into an issue,
which a rail never did. The lint panel keeps its rails — there is no label
there, and rows are scanned as a list.
