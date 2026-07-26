---
"@brink-lang/web": patch
---

Analyzer: E067 void-assignment check extends to inferred-void functions
(issue #1054).

`~ x = f()` / `~ temp x = f()` where `f` resolves to a function with no
explicit `): void ===` annotation, but whose body never carries a
value-returning `return <expr>` (issue #1046's inferred-void reading), now
fires `E067` under `types = strict` — the same "assigning a void call is an
error" diagnostic an explicitly-annotated `void` function already got.
Before this fix `collect_void_defs` only ever consulted the knot's own
`return_type` annotation, so assigning the result of a function that
returns nothing purely by inference was silently accepted.

A function with a *declared*, non-`void` return type whose body never
returns a value is unaffected by this change — that shape is the
`E150` checker error (issue #1551, "declares a return type but its body
never returns a value"), not void, and is deliberately excluded from this
check.
