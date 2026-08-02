---
"@brink-lang/web": patch
---

Issue #2001 (the tracked remainder of #1995/#1920 after PR #1999): the
`#fn(target, args…)` partial-application creation site now checks its
by-ref bound arguments invariantly too.

`infer_fn_literal` (the `#fn` literal's own bound-argument loop) recorded
call-graph edges, fn-value creation, and `ref`-param write tracking, but
performed **no argument-type check at all** — the exact soundness hole
#1995/#1920 closed for the direct-call and UFCS-desugared sites, in a
third spelling: ink `VAR i = 3` + `~ temp f = #fn(scale, i)` against
`function scale(ref x: float, k: int)` used to yield zero diagnostics.
`#fn`'s own `fn_values::check` (`E080`) only verifies a `ref` position is
bound to *some* durable cell, never that the cell's static type agrees
with the declared `ref` param type, so this was a genuinely separate gap.

A `ref`-bound argument whose type does not match the declared `ref`
param type exactly (`infer::ty::ref_assignable`) now reports `E063` under
`types = strict`, mirroring the direct-call/UFCS checks' `ref`-arm
handling (this loop only ever checks `ref`-bound arguments, so its
observed-local carve-out is always the partial one, never the full skip
the direct-call check's non-ref arm uses).

By-value (non-`ref`) bound arguments at this creation site are
deliberately left unchecked — `infer_fn_literal` has never had a
by-value argument check either, and #2001 named that as new checking
needing its own scope call, not an assumed yes.

This is a `dialect = brink` (extension-syntax) change gated at the
`#fn` literal itself, which is brink-extension syntax — vanilla ink
does have `ref` parameters (e.g. `function alter(ref x, k)`), but an
ink-dialect file has no param type annotations and no strict policy,
so there is no declared type for an argument to disagree with, and
`#fn` isn't reachable from a vanilla-ink file at all. Observable
through `@brink-lang/web` because the wasm package re-exports the same
diagnostics.
