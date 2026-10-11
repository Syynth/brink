---
"@brink-lang/web": patch
---

A glue in the middle of a line, after an interpolation that renders empty, is no longer dropped by the compiler. `a` / `{e}<> b`, with `e` empty, prints `a b` as ink does (#3695).
