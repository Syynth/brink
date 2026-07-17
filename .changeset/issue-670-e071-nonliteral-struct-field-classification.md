---
"@brink-lang/web": patch
---

Analyzer: `E071` (mistyped struct construction field, strict mode) now
classifies variable-, call-, and index-valued initializers, not only
literal-shaped ones (issue #670).

`STRUCT` construction-literal type checking previously only classified
literal-shaped field initializers (scalars, arrays, maps, nested struct
literals) — a variable, function call, or indexing expression stayed
silently unchecked, deferring entirely to the runtime fault. `E071` now also
consults the whole-project inference substrate (`BodyTypes::locals` for a
param/temp, the declaration-derived type for a global `VAR`/`CONST`, the
resolved callee's `InferredSig::return_ty` for a call, and the base's
classified element/value type for an index) when the initializer's own
shape isn't literal. Whenever that resolution lands on `Unknown` or
`Conflicted` — unresolved, unannotated, or genuinely contradictory — the
field stays silently unchecked, same "Unknown never disagrees" posture as
every other gradual-mode-aware check in this analyzer.
