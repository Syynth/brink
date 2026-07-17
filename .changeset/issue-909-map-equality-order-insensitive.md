---
"@brink-lang/web": patch
---

Map/record `==`: map equality is now content-based, not
insertion-order-sensitive (issue #909, ruled 2026-07-18 —
`docs/decision-log.md` "Map/record equality is insertion-order-insensitive").

`#{a:1, b:2} == #{b:2, a:1}` now evaluates `true`. Previously,
`OrderedMap`'s derived `PartialEq` compared its backing `Vec<(MapKey,
Value)>` positionally, so two maps holding identical key/value pairs
inserted in different orders compared unequal — a silent correctness bug,
since ink authors have no way to observe or control the internal `Vec`
layout an equality check was leaking.

`OrderedMap` now hand-implements `PartialEq` as a content comparison: same
entry count (fast-path reject on size mismatch), then every key in one map
looked up and value-compared in the other — order-independent by
construction. Every equality-derived operation (`==`, `!=`, and any future
membership/contains-style check built on `Value::eq`) picks this up
automatically through `Value`'s existing `PartialEq` delegation to
`Value::Map`'s `Arc::ptr_eq` fast path and structural fallback — no call
site changes needed.

**Unchanged**: iteration order (`iter`/`keys`/`values`) and
serialization/wire order both stay insertion-order — only equality ignores
it. Record equality (shape-ordered fields, not insertion-ordered) is
unaffected by this ruling.

Observable through `@brink-lang/web`: any ink script comparing two map or
record values containing maps via `==`/`!=` now gets content-based results
regardless of the order the maps' keys were built in.
