---
"@brink-lang/web": patch
---

A `shuffle once` with text around it keeps that text after its alternatives run out. `A {shuffle once:c|d} Z` prints `A Z` on every later visit, as ink does, where it used to print nothing at all (#3413). The order of the draws is unchanged.
