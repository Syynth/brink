---
"@brink-lang/web": patch
---

Fixed `Map`/`Map` and `Record`/`Record` equality (`==`/`!=`) faulting with a
`TypeError` at runtime instead of comparing. `value_ops::binary_op` had no
match arm at all for these two variant pairs, even though `Value`'s own
`PartialEq` already implements the ratified structural-equality-with-an-
`Arc::ptr_eq`-fast-path rule (value-model-spec §4) — the same comparison
`contains()`'s Array branch already exercises for element containment.
Both arms now delegate to `Value`'s `PartialEq`; ordering operators
(`<`, `>`, `<=`, `>=`) on maps/records still fault, as before — no ordering
is defined for either.

Note: map equality currently follows `OrderedMap`'s existing (insertion-
order-sensitive) derived `PartialEq` unchanged. Whether two maps with the
same entries in a different insertion order should compare equal is a
separate, still-open question tracked in #909 (parked for a maintainer
ruling) — this fix does not decide it either way, and map-equality
semantics may change once that ruling lands.
