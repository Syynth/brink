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

Vanilla-ink stories are unaffected; the oracle corpus is byte-identical.
