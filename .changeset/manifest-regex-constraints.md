---
"@brink-lang/web": patch
---

Host-manifest regex constraints are now checked (#3683). A string literal
that doesn't match its type's pattern raises E042, as enum and range
constraints already did; the pattern must match the whole string. A pattern
that doesn't compile is refused when the manifest is loaded, naming the
pattern, instead of letting every value through.
