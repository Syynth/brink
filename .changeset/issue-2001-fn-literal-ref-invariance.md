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
`types = strict`, mirroring the direct-call/UFCS checks exactly (same
observed-local carve-out, scoped to the `ref` arm only).

By-value (non-`ref`) bound arguments at this creation site are
deliberately left unchecked — `infer_fn_literal` has never had a
by-value argument check either, and #2001 named that as new checking
needing its own scope call, not an assumed yes.

This is a `dialect = brink` (extension-syntax), by-ref-only change —
vanilla ink has no `ref` parameters to reach it. Observable through
`@brink-lang/web` because the wasm package re-exports the same
diagnostics.
