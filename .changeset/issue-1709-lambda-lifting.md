---
"@brink-lang/web": patch
---

Lambda lifting (#1709): a `|x| …` lambda in a `.brink` source now compiles
and runs. #1685 landed lambdas as far as HIR, after which LIR lowering
raised a targeted E052 ("no runtime representation yet"), so compiling any
source containing a lambda still failed. Lowering now lifts the lambda body
into a synthesized top-level function and creates an ordinary function value
over it — `PushFnRef` when the lambda captures nothing, `MakeClosure` (the
existing `VAL_CLOSURE` `{name, is_ref, payload}` environment) when it does.
Capture is by value always, per the 2026-07-19 ruling: each capture is
evaluated once at the point the lambda value is made, so a later write to
the enclosing local is not visible through the value, and no capture is ever
a `ref`. Both ruled body spellings work — the single expression and the
braced block whose trailing expression is the value.

The practical consequence is that a lambda literal is now a legal callback
for the pure verb trio `map`/`filter`/`fold` (#1679); before this, `#fn(named
function)` was the only fn-value spelling those verbs could be handed. Note
that "pure-required" still cannot be checked through a lambda callback:
`Ty::Fn` carries no effect rows (#1680), so the E119 gate continues to judge
only inline `#fn(target)` callbacks, and the dev-mode world-write guard
remains the runtime residual. Ink sources are entirely unaffected — ink's
grammar cannot spell a lambda — and the oracle corpus is unchanged.
