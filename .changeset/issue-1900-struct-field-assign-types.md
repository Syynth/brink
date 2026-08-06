---
"@brink-lang/web": patch
---

A plain struct-field assignment (`~ p.x = expr`) is now type-checked
against the field's declared type under `types = strict` (#1900, split from
#1864/#1877): the field's declared type was never checked before, on either
of the two root sources #1899 covers for a bare assignment target.

```ink
STRUCT Point = #{x: float, y: float}
VAR p: Point = Point#{x: 0.0, y: 0.0}
~ p.x = "wrong"
-> DONE
```

used to compile with zero diagnostics under `types = strict`; it now
reports `E063`. Covers a `VAR`/`CONST` root and an annotated `~ temp`
root, mirroring #1899's own two root sources. A field name the struct
shape doesn't declare, or an unresolvable root type, stays silently
unchecked ("Unknown never disagrees" — not this check's job). Strict-mode-
only; `types = gradual` is unaffected and keeps deferring to the existing
runtime type-mismatch fault. Scoped to a plain (non-`ref`) assignment
target only — a `ref`-mediated field write is a different aliasing
channel this check does not reach.
