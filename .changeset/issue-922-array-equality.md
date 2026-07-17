---
"@brink-lang/web": patch
---

Fixed `Array`/`Array` equality (`==`/`!=`) faulting with a `TypeError` at
runtime instead of comparing. `value_ops::binary_op` had no match arm at all
for this variant pair, even though `Value`'s own `PartialEq` already
implements the ratified structural-equality-with-an-`Arc::ptr_eq`-fast-path
rule (value-model-spec §4). The arm now delegates to `Value`'s `PartialEq`;
ordering operators (`<`, `>`, `<=`, `>=`) on arrays still fault, as before —
no ordering is defined.

Unlike the parked map-ordering question in #909, array equality is
unambiguously order-sensitive by construction — element order is observable
array structure, not an incidental insertion artifact — so there is no
analogous ruling to park here: `[1, 2] == [2, 1]` is `false`.
