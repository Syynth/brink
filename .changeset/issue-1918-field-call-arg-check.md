---
"@brink-lang/web": patch
---

A UFCS call through a struct's own fn-typed field (`recv.field(args)`,
`UfcsVerdict::FieldCall`) is now argument-checked under `types = strict`,
closing a gap #1914 (issue #1881) deliberately left uncovered: a wrong-arity
or wrong-typed call through a field compiled clean with zero diagnostics.
`E063` now fires for both an arity mismatch and a per-argument type
mismatch, phrased like the existing "call through a value" (T1c) diagnostic
family — matching `strict::check_value_calls`'s own wording, since a field
call is structurally that same "call through a function value" case, just
reached via field access. Gradual mode is unaffected (this class of static
check is strict-only, matching the sibling `FreeFnDesugar`/`FreeFnAutoRef`/
`PreludeDesugar` checks already shipped); a correctly-typed, correct-arity
field call still compiles clean.
