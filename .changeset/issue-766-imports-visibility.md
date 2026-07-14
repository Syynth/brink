---
"@brink-lang/web": patch
---

Added the M-2 module imports + visibility surface (docs/modules-spec.md
§2/§4/§7), building on M-1's module name model.

- **`IMPORT` grammar** — both forms: bare `IMPORT { a, b AS c } FROM mod`
  and qualified `IMPORT mod`. `FROM`/`AS` stay contextual soft keywords;
  only `IMPORT` is reserved. Superset-parsed always; the brink-dialect gate
  rejects `IMPORT` under strict-ink (E051-class), like `#@module`.
- **`#@private` / `#@public` visibility** on every importable definition
  (knot, function, VAR, CONST, LIST, STRUCT). Effective visibility follows
  declaration-flips-default: a declared module defaults private, an
  undeclared stem-module defaults public, and the per-definition directive
  overrides that.
- **Diagnostics** (§7): private-cross-module reference (E087), unresolved
  import (E088), duplicate import (E089), self-import (E090), qualified
  ambiguity code reserved (E091), redundant-override warning (E092), and
  conflicting visibility directives (E093). `#@private`/`#@public` are
  brink-dialect-gated under strict-ink (E051).

Compat: purely additive and brink-gated. The entire pre-modules world keeps
visibility public and stays in the permeable flat namespace, so no existing
story's resolution changes.
