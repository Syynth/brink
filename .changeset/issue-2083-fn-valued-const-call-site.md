---
"@brink-lang/web": patch
---

Analyzer: a fn-valued global `const`'s call site now resolves (issue #2083).

Calling a fn-valued global `const` from anywhere other than its own
declaration — either the bare-name form (`const twice = double`, #1862) or
a lambda-literal decl default (`const twice = |x| x * 2`, #1774) — used to
fail with `E025` ("unresolved variable reference"). RCA found the bug was
never a `brink-db` incremental-resolution gap, despite the issue's own
report suspecting one: `brink_analyzer::resolve::resolve_function`'s
call-site "try variables" lookup searched only `SymbolKind::Variable`,
never `SymbolKind::Constant` — a `var`-bound fn value's call site already
resolved (`resolve_variable`'s own bare-*read* lookup already searches
`[Variable, Constant]` together; the call-site lookup was a one-sided
omission). Fixed by adding `Constant` to that lookup, so both the
`brink-db` db-direct road and the off-db `IdeSnapshot::analyze` road agree.
