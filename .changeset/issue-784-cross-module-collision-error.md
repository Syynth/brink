---
"@brink-lang/web": patch
---

M-2c stopgap: cross-**declared**-module same-name duplicate definitions
are now a hard error under `dialect = brink` (issue #784).

- **`E096`** — two *declared* modules (`#@module(name)`, different names)
  each defining a same-name, same-kind symbol (a knot, stitch, VAR/CONST,
  LIST, STRUCT, EXTERNAL, or label) is now a compile error, reported at
  *both* definitions' spans. Flat resolution (unchanged by this stopgap —
  true import-scoped resolution is tracked separately, #790) binds a bare
  name to whichever declared-module definition merge happens to see first,
  so two declared modules sharing a name silently made that binding
  order-dependent. Escalating to a hard error makes flat resolution correct
  by construction until scoping lands.
- A duplicate *within* one declared module (same module name on both
  files), or involving any undeclared/legacy file, keeps the existing
  `E022`/`E023`/`E026` warning — unchanged.
- Gated to `dialect = brink` only: under `strict-ink` (the default), this
  code never fires — the compat/oracle corpus is untouched.
