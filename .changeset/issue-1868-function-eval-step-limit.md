---
"@brink-lang/web": patch
---

Runtime function evaluation (`begin_function_eval`/`resume_function_eval`)
now honors a caller-supplied VM step budget instead of always sharing the
hardcoded 1,000,000-step production ceiling (#1868).

`WebSpeculation::eval_function`/`resume_function_eval` already parsed a
`steps` option (`speculate(options)`'s `steps` field, marshaled into
`Budget.steps`) but silently ignored it for function evaluation — only
`advance()` honored it. A tiny `steps` budget passed to `speculate()` now
also caps a runaway (e.g. infinitely recursive) `eval_function`/
`resume_function_eval` call with the expected step-limit error, instead of
burning the full production budget before giving up.

**Consumer-breaking note:** `Budget::default().steps` is 100,000, and
`speculate()` already fills an unset `steps` option from that default. Before
this fix, an unset `steps` was silently never consulted by
`eval_function`/`resume_function_eval`, which instead ran under the runtime's
hardcoded 1,000,000-step production ceiling. After this fix, a JS caller
using `speculate({})` (or any `speculate(options)` call that omits `steps`)
followed by `evalFunction`/`resumeFunctionEval` now gets the 100,000-step
default applied — a 10x tightening. A legitimately expensive function that
completed under the old 1,000,000-step ceiling can now fail with a
step-limit error unless the caller passes an explicit, larger `steps` value
to `speculate()`.

Vanilla-ink stories are unaffected; the oracle corpus is byte-identical.
