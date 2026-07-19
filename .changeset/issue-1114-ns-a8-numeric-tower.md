---
"@brink-lang/web": patch
---

NS-A8 (#1114): the numeric tower lands per the ruled mini-spec
(`docs/tower-mini-spec.md` T1–T5) — `vec2`/`vec3`/`vec4`/`quat`/`mat2`/
`mat3`/`mat4` as glam-backed compiler-known value kinds with brink-dialect
constructors, `dot`/`cross`, the tower-wide two-arg `min`/`max` plus
`clamp`/`lerp`, glam's operator conventions on the frozen arithmetic
operators (componentwise `+`/`-`/`*`, scalar scale, `mat * vec` transform,
`quat * quat` composition, `quat * vec` rotation, componentwise negation),
glam-named component access (`v.x`, `m.y_axis`), componentwise-IEEE
equality (NaN lanes make a value unequal to itself; tower kinds are NOT
orderable — ordering contexts fault). New wire value tags
`VAL_VEC2`..`VAL_MAT4` (0x12–0x18) carrying hand-serialized little-endian
f32 lanes (never glam's memory layout), one new opcode `Tower(kind)` at
0xF7, a lossless `TypedValueJs::Tower` kind+lanes form on the JSON
boundary, `{x, y, …}` / `{x_axis, …}` objects on the native JS boundary,
and a new compile diagnostic E118 (tower kinds can never implement
registry protocols — `compare` for a tower kind is impossible by
construction). Vanilla-ink stories are byte-identical; the oracle corpus
is unaffected.
