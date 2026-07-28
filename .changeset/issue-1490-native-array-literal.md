---
"@brink-lang/web": patch
---

NG-D (ruled 2026-07-27): the native `.brink` surface gains an
array/sequence literal, `[1, 2, 3]` — square brackets, expression position
only, lowering directly to the same `Expr::ArrayLiteral` HIR shape the
brink dialect's `#[…]` sigil literal already produces. The B5-symmetric
`Array { … }` construction-registry spelling was weighed and rejected:
brackets were already lexed and idle in expression position, and the
everyday collection literal deserves the lightest spelling. `[]` (empty)
and nested arrays (`[[1, 2], [3]]`) are both accepted; every existing
dialect-agnostic analyzer pass over `Ty::Array`/`Expr::ArrayLiteral`
(inference, containment, comparator contracts, …) applies unchanged.
Observable through `@brink-lang/web`: a `.brink` project compiled through
the wasm package can now parse and run source using this literal, where it
previously failed with a parse error. Fixes #1490.
