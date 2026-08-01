---
"@brink-lang/web": patch
---

A UFCS call into a T1b/NS stdlib prelude verb (`xs.push(v)`, `m.get(k)`,
`m.insert(k, v)`, and the rest of that family) now gets the same
argument-domain checking its direct-call spelling already had — the UFCS
sibling of issue #1881's `FreeFnDesugar`/`FreeFnAutoRef` argument-type
check (#1919).

Previously, `m.get(k)` on a project with `dialect = "brink"` and
`types = strict` reported no diagnostic even when `k`'s statically-known
type disagreed with `m`'s declared key type, while the byte-equivalent
`get(m, k)` was already caught. A prelude verb has no `DefinitionId`/
declared parameter list to compare against, so the check keys the
expected argument type off the receiver's own inferred container type
(an array's element type, a map's key/value types) instead.

Covered: `push`/`heap_push` (array element), `insert`/`get`/`remove`
(map key, plus `insert`'s value), `index_of`/`contains` (array element
or map key), `contains_value` (map value). `remove`'s array leg is
unaffected — that shape stays the existing `E149` migration diagnostic
(issue #1540), a disjoint diagnostic family from this one.

Observable through the wasm package's `.brink` compile/analysis surface:
a native project's UFCS-spelled prelude call with a statically
disagreeing argument now reports `E063` where it previously compiled
clean.
