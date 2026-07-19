---
"@brink-lang/web": patch
---

NS-A1 (#1107): `Option[T]` lands as the third compiler-known parameterized
builtin, with the ruled stdlib verb flips as brink-dialect intrinsics —
text `find`, seq `index_of`/`min`/`max`/`first`/`last`/`pop`, map
`get`/`contains_value`/`clear` — all returning typed absence (`none` /
`some(x)`) instead of sentinels or faults. New compileable surface
(`some(x)` constructor, bare `none` literal, the ten verbs), a new wire
value tag (`VAL_OPTION`) with lossless `TypedValueJs::Option` on the JSON
boundary and value-or-null marshalling on the native JS boundary, and a
new compile diagnostic E107 (bare `none` needs a type from context).
Vanilla-ink stories are byte-identical; the oracle corpus is unaffected.
