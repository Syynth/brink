---
"@brink-lang/web": patch
---

Runtime `==`/`!=` completeness sweep (issue #939, tracked from #397):
`VariablePointer`, `TempPointer`, and `Projection` values no longer
fault with a type error when compared with `==`/`!=` — they now compare
correctly (token equality for the pointers, structural same-root-cell +
equal-segments equality for projections), delegating to `Value`'s own
`PartialEq` exactly like the prior fixes for `FnRef`/`Closure`/`Handle`/
`Array`/`Map`/`Record` (#918, #931).

Also fixes a float-equality inconsistency: direct float `==`/`!=` used
to tolerate an `f32::EPSILON` fudge factor while a float nested inside
an array/map/record/projection always compared by exact IEEE equality.
Both routes now use exact equality (matching the C# reference ink
runtime's plain `x == y` and the already-shipped collection-equality
behavior) — a small behavior change: two floats that previously
compared equal only because they happened to land within
`f32::EPSILON` of each other (e.g. accumulated rounding error from
independent arithmetic paths) now compare unequal, same as arrays/maps
already did with the same inputs.
