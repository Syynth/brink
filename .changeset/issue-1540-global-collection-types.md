---
"@brink-lang/web": patch
---

Compiler: `VAR`/`CONST` globals now carry a static `array`/`map`/`struct`/
`fn`/`option`/`range` type (issue #1540).

A global's declaration-derived type used to travel as `InferredType`, whose
domain is scalars plus `divert` and `list<L>`. Every other shape —
`array<T>`, `map<K, V>`, a nominal `STRUCT`, `fn(T…): R`, `handle<K>`,
`option<T>`, `range` — was silently discarded on the way into the globals
map that every typed check reads, so a collection-typed global was invisible
to all of them: `E149` (`remove` is map-only) could fire for a `temp` but
never for `VAR arr = #[…]`, and `int(someArrayGlobal)` compiled clean where
the `temp` spelling reported `E078`.

`Sig` now carries `value_ty`, the declaration's type at full fidelity —
resolved from the annotation with no downcast, else from the initializer
literal (`#[…]` / `#{…}` / `Name#{…}` included), else from a `#fn(…)`
initializer. The narrow `value_type` field is unchanged, so hover and the
program model see exactly what they saw before.

Collection-typed diagnostics also reach the UFCS spelling now
(`arr.remove(0)`, not just `remove(arr, 0)`): inference types a
multi-segment callee as unknown before intrinsic dispatch runs, so those
call sites recorded no facts at all — the strict check now reads the B3a
verdict table, which already carries the receiver's resolved type beside
the verb's name.

Programs that were relying on a collection-typed global going unchecked may
see a diagnostic they did not see before; that diagnostic is reporting a
real mistyping.
