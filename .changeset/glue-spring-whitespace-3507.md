---
"@brink-lang/web": patch
---

Whitespace between an inline construct and glue now renders the way ink
renders it (#3507). `{0} <>` followed by `world` prints `0 world` (it
printed `0world`); the same holds after an inline conditional or
sequence. When the construct renders empty, glue drops the space with the
newline it consumes, so `a` / `{false:x} <>` / `b` prints `ab` — the
runtime's glue resolution now trims whitespace-only output after a
consumed newline exactly as ink's does, which also changes the rare
whitespace-only-text-before-glue shape from `a b` to `ab`.
