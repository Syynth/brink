---
"@brink-lang/web": patch
---

A stateful alternative on a line with an inline conditional now advances
once per line view, whichever branch renders (ink's documented
semantics): container ids are stamped before normalization, so the
conditional's cloned branches share the alternative's container instead
of each advancing a private copy.
