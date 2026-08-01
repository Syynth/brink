---
"@brink-lang/web": patch
---

Direct-call arguments are now type-checked against the callee's declared
parameter types under `types = strict` (#1864): `h("hi")` with `fn h(x:
int)` now reports `E063` instead of compiling with zero diagnostics. This
was a pre-existing hole — a call through a function *value* (T1c) was
already checked; an ordinary direct call resolving straight to a known
knot/stitch was not, which made `content`'s (#1846) "never coerces to or
from string" invariant inert in practice at call sites (`take(mk())` with
`fn take(x: content)` and `mk()` returning `string`).

Scoped to arguments `structs::classify_expr_ty`'s existing inference
substrate can statically classify (literals, call results, index
expressions, global `VAR`/`CONST` references) and deliberately excludes a
`~ temp`/param argument whose own type this same call's `observe` join
already drives to `Ty::Conflicted` — that case already reports `E066`
separately, so this check never double-reports it. Strict-mode-only;
`types = gradual` is unaffected and keeps deferring to the existing
runtime type-mismatch fault. A call through a function value (UFCS or
otherwise) and a call to an `EXTERNAL` binding are unaffected — those are
`strict::check_value_calls`'s and `external_check`'s own domains
respectively.
