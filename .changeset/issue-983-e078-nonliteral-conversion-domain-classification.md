---
"@brink-lang/web": patch
---

Analyzer: strict-mode `E078` (`int()`/`float()` out-of-domain argument)
no longer classifies only literal-shaped arguments (issue #983, sibling of
#670).

`conversions::check`'s domain check previously only recognized a
divert-target expression, a LIST literal, or a `#[...]`/`#{...}`/`Name#{...}`
collection/struct literal passed *directly* as the `int(x)`/`float(x)`
argument — a variable-, call-, or index-valued argument with a statically
provable out-of-domain type slipped through uncaught at compile time (still
caught at runtime by the `InvalidConversionDomain` fault, but only strict
mode's compile-time convenience was missing it).

`conversions::check` now reuses `structs::classify_expr_ty`/
`structs::MistypeCtx` verbatim — the exact inference-substrate
classification issue #670 added for `structs::check`'s own `E071` — to
resolve a `Path` (param/temp via `BodyTypes::locals`, or global `VAR`/CONST
via its declaration-derived type), a `Call` (the resolved callee's
`InferredSig::return_ty`), or an `Index` expression (its base's classified
array-element/map-value type) to a concrete `Ty`, then checks whether that
`Ty` falls outside the permitted `int`/`float`/`bool`/`string` domain.
Whenever the resolution lands on `Unknown` or `Conflicted`, the argument
stays silently unchecked — the same gradual-mode conservatism the literal
check already had.
