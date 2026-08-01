---
"@brink-lang/web": patch
---

Native bare-name fn values (#1862): in a `.brink` source, a statically-named
function used in expression position is now a **function value** —
`map(items, double)`, no sigil — while a call still keeps its parentheses
(`double(4)`), so reference-vs-call stays unambiguous. This is the 2026-08-01
ruling; `#fn(…)` is deliberately *not* given a native spelling, because `#`
already opens a tag in native content position.

This fixes a silent mis-compile, not just a missing feature. Until now the
same bare name lowered to the knot's **visit count**, so `map(items, double)`
compiled clean and reached the runtime as `map` over an `int`, failing with
"callback must be a function value `fn(T): U`, got int". Web consumers
compiling a `.brink` entry (`compileFragment`/session compiles) therefore see
both a behavior change — such a reference now produces a callable value — and
one new compile error: a target with a `ref` parameter can never be referenced
by bare name, because a bare name binds no arguments and every `ref` parameter
must be bound at creation (E080 at the reference site). The partial-application
form `#fn(f, a)` keeps no native spelling at all and stays ink-only.

Ink is untouched: a bare function-knot name in `.ink` source is still a visit
count, `#fn(…)` remains ink's only fn-value spelling, and the oracle corpus is
unchanged. Respelling ink into native (`brink-respell`) follows the same split
— a zero-bound `#fn(f)` now emits as the bare name `f`; the binding form still
refuses loudly rather than emitting a lambda, whose by-value capture would
silently differ from a `ref` binding.
