---
"@brink-lang/editor": patch
---

Hover cards are suppressed while an inline rename row is open, and any
open card is dismissed when the rename starts. The reworked rename UI
places the "⚠ breaks N" badge beneath the token — exactly where a hover
tooltip lands when it flips below (viewport top) — so the card sat on
top of the badge and intercepted its clicks (caught by the symbol-rename
e2e; a real-pointer hazard too, since moving toward the badge keeps the
card alive by hovering it).
