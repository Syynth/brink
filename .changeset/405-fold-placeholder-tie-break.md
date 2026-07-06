---
"@brink-lang/editor": patch
---

Fix #405: `preparePlaceholder` now breaks an exact-span tie between a structural fold and a machinery/narrative fold deliberately (structural wins), instead of relying on the accidental push order of `getFoldingRanges()`. No visible behavior change for the ordering hosts already ship (structural is pushed first in `folding_ranges_impl`), but the precedence is now pinned and covered by a test that constructs the tie with the ranges pushed in the opposite order.
