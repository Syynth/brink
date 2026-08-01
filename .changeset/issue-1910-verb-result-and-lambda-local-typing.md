---
"@brink-lang/web": patch
---

#1910: under `types = strict`, a pure verb call
(`map`/`filter`/`fold`/`filter_map`/`map_each`) with an inline lambda
callback, and a lambda literal bound straight to a local, both used to
escape strict inference as `Unknown` even when the callback's own body
unambiguously pinned the type — `map(items, |x| x * 2)` over `Array<int>`
reported `E065` on its result, and `let f = |x| x + 1;` reported `E065` on
`f` itself, unless an enclosing annotation happened to ascribe the same
type from outside.

`InferPass::infer_lambda` used to rebuild a lambda's own `fn(T…): R` type
from written annotations alone once its body walk finished, discarding
everything the walk itself had learned — the same mono-HM narrowing a
top-level `fn`'s own params/return already get. Fixed by reading that
narrowing back (param types from `self.locals`, the return type from the
tail/`return` statements), shadowed by param name for the walk's duration so
it neither reads nor leaks through an enclosing same-named local. `fold`'s
own typing rule also now prefers the seed's type over a callback whose
return is merely `Unknown` (never over one that is `Conflicted`, which is
real information).

Reachable through any `@brink-lang/web` session compiling a `.brink` file
under `types = strict` (a `brink.toml` `[project] types = "strict"`, or an
explicit `--types strict`/`AnalysisOptions` request) that calls a pure verb
with an inline lambda, or binds a lambda literal to a `let`.
