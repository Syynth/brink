---
"@brink-lang/web": patch
---

Fixed (#1168): a user-defined function's `Option[T]`-shaped return no
longer escapes strict inference as `Option[Unknown]` when the wrapped
value is a param/temp passed straight through with no other body
evidence — `~ return some(x)` for an annotated `x`, `get(m, k)` on an
annotated map param, and a `for` loop over an annotated `array<T>` param
now infer their concrete element type instead of tripping a false `E065`
(brink dialect only; observable through `@brink-lang/web`'s diagnostics
since the checker's inference walk changed). Body-derived evidence that
genuinely disagrees with an annotation (e.g. a param annotated `string`
but only ever compared against an `int` literal) is unaffected — that
case still infers from usage, unchanged.
