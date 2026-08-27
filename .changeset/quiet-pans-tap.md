---
"@brink-lang/studio": patch
---

Quick open (⌘P) no longer lists symbols from mounted `std/` library files or
from `brink.toml` — the same set the Binder tree and Continuous view show,
since those aren't places you navigate to while writing. Symbol entries are
also keyed by span, so two knots declaring the same stitch name can't collide
on one React key (which silently dropped or duplicated rows).
