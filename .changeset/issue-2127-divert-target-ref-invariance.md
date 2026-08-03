---
"@brink-lang/web": patch
---

Issue #2127 (found while closing out #1995/#1920, deliberately fenced out
of it): a divert with arguments (`-> knot(a, b)`) now checks its `ref`
position arguments invariantly too.

`InferPass::infer_target` (the `-> knot(args)` divert-with-args site)
computed `arg_tys` and then explicitly discarded it (`let _ = arg_tys;`)
— it called `record_ref_param_writes` so a `ref` param's *write* was
tracked for effect purposes, but performed **zero argument-type
checking**, for `ref` or by-value positions alike. Unlike the three
sibling sites #1995/#1920 already fixed (direct call, UFCS-desugared
call, `#fn` creation site), there was no existing covariant check here
to invert — this is a whole check that never existed.

A `ref`-bound argument at a divert-with-args site whose type does not
match the declared `ref` param type exactly (`infer::ty::ref_assignable`)
now reports `E063` under `types = strict`, reusing the same
`DirectCallArgMismatch` fact and `check_direct_call_args` reporting path
the direct-call and `#fn`-creation-site checks already use (a divert
target is, like a `#fn` literal, "not a call at all" but the same
by-ref binding shape) — mirroring `infer_call`'s `ref` arm, including its
observed-local carve-out (scoped to the `ref` arm only, same as the
`#fn` creation-site fix).

By-value (non-`ref`) argument positions at this site are deliberately
left unchecked. Per the issue's own scope note (and the precedent PR
#2014 set for `infer_fn_literal`'s by-value params): whether/how to
check by-value divert-target arguments is its own design call, not an
assumed yes.

Observable through `@brink-lang/web` because the wasm package re-exports
the same diagnostics.
