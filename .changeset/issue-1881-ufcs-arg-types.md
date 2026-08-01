---
"@brink-lang/web": patch
---

UFCS-desugared calls (`recv.name(args)` → `name(recv, args)`) are now
argument-type-checked against the desugared free function's declared param
types under `types = strict` (#1881, the third and final position in the
#1864 argument-type-checking family — #1875 did direct calls, #1899 did
declaration initializers and assignments).

- Both the receiver (the desugar's first positional slot) and every
  written argument are now checked: `g.greet(3)` where `fn greet(name:
  string)` is declared and `g` is `int`-typed now reports `E063` instead
  of compiling with zero diagnostics.
- Covers both desugar shapes: the plain by-value free-call
  (`UfcsVerdict::FreeFnDesugar`) and the D5 auto-ref desugar for a `ref`
  first param (`UfcsVerdict::FreeFnAutoRef`) — a `ref` param's declared
  type is read the same way in both cases (the referent's own type, never
  a separate "reference" type), so a genuine receiver-type mismatch is
  caught through auto-ref too, with no false positive from `ref`-ness
  alone.
- The resolved free-function target's declared param types come from
  `InferenceResult::signatures`, the same firewall-facing projection a
  direct call's own check reads; the per-argument types are recorded by
  `infer::body`'s existing body walk (the only place argument expressions
  have types) and consumed by `ufcs::UfcsVisitor`'s own resolution pass,
  which already had the resolved target at the point it emits
  `E140`-`E144`.

Strict-mode-only, reported as `E063` — the same code the direct-call and
assignment-site siblings use, no new code minted. `types = gradual` is
unaffected (UFCS is native-only, and native compiles are strict-only, so
there is no native `types = gradual` compile this check could ever reach
regardless).
