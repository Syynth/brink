---
"@brink-lang/web": patch
---

List values now carry origins the way ink does: a non-empty list's
origins are its items', an empty list's are whatever it was built with
(none for `^`, `LIST_MIN`/`MAX`/`ALL`/`INVERT` and `list + int`, the
left operand's for `+`/`-`, the input's for a non-empty `LIST_RANGE`),
and an empty list assigned to a global or an existing temp takes the
old value's origins. `LIST_ALL`/`LIST_INVERT`/`LIST_COUNT` over emptied
lists match ink (#3532).
