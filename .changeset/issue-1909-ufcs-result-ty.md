---
"@brink-lang/web": patch
---

A UFCS method call that desugars to a free function
(`recv.name(args)` → `name(recv, args)`) now carries that function's own
declared return type instead of escaping inference as `Unknown` (#1909).

Previously, `fn f() { let n = 21; return n.double(); }` reported `E065`
("`f`'s return type escapes strict inference as Unknown") on a `.brink`
project with `dialect = "brink"`, while the byte-equivalent
`return double(n);` compiled clean — identical bodies, only the call
spelling differing. The `Unknown` also propagated: an `Unknown` reaching a
call position is what the value-call check's own `Unknown` arm turns into
a further diagnostic downstream.

- The desugar's call-graph edge is recorded too, which is what makes the
  target's signature reliably available before the caller is solved.
- Both desugar shapes are covered: the plain by-value free call and the D5
  auto-ref desugar for a `ref` first parameter.
- Deliberately unchanged: a prelude verb (`m.len()`), a struct-typed
  receiver (where field access wins over a same-named free function), a
  projected receiver (`a.b.c()`), an ambiguous cross-module name, and a
  wrong-arity call all keep their previous `Unknown` result — none of them
  can be decided without inputs this stage does not have, and guessing
  would risk contradicting the resolution pass's own verdict.

Observable through the wasm package's `.brink` compile/analysis surface:
a native project that previously failed to compile on this spurious
`E065` now compiles, and the inferred signature reported for the enclosing
function is concrete rather than `Unknown`.
