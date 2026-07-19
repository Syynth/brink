# The numeric tower — mini-spec (RULED 2026-07-19)

The §2b-owed mini-spec, ruled in-conversation (T1–T5, airport
sitting). Resolves the #827 representation question and unblocks
NS wave A8 (#1114). Design authority: stdlib-spec §2b (the tower
ruling), this document, decision-log 2026-07-19.

## T1 — Representation: glam-backed Value kinds

`Value::Vec2(glam::Vec2)` · `Vec3(glam::Vec3)` · `Vec4(glam::Vec4)`
· `Quat(glam::Quat)` · `Mat2(glam::Mat2)` · `Mat3(glam::Mat3)` ·
`Mat4(glam::Mat4)`.

- **glam is the in-memory compute type** — quat composition, mat
  inverses, slerp arrive correct-by-construction (the oracle cannot
  cover tower math; borrowing battle-tested ops beats hand-rolling
  + hand-testing). Dependency hygiene: glam is a pure-math crate,
  not an engine dependency — format host-agnosticism intact;
  wasm-clean.
- **Unaligned variants only** (`Vec3` not `Vec3A`, `Mat3` not
  `Mat3A`) — aligned variants would bloat every `Value`.
- **One workspace-pinned glam version** shared with bevy-brink
  (`dep.workspace = true`) → the bevy marshal is identity on the
  same types. Rejected alternative recorded: tower-as-blessed-
  structs (Record machinery) fails on fidelity — Record fields are
  f64 `Value::Float`s; f32 components would round-trip-drift
  against glam, the exact drift "the bevy boundary marshals
  structurally" exists to prevent.

## T2 — Sizes: all of them

mat2, mat3, mat4 all ship v1 ("i don't see why we don't just have
them" — with glam backing, each size is one value kind + one wire
tag wrapping tested code; the economy argument died with T1).

## T3 — Conventions: per glam, wholesale

Column-major matrices; right-handed; quat = (x, y, z, w);
`+`/`-`/`*` componentwise, scalar scale, `mat * vec` transforms,
`quat * quat` composes, `quat * vec` rotates; `dot`/`cross` verbs;
the scalar kit (lerp/clamp/min/max/…) defined across the tower as
its width-1 floor — all as already ruled in §2b, conventions now
pinned to glam's.

## T4 — Equality & ordering

Componentwise IEEE `==` — a NaN-bearing vec never equals itself,
exactly like bare float; `-0 == +0` per lane. Tower types are NOT
orderable (consistent with the §4b roster — no lexicographic vec
ordering; a vec in a sort key is a NotOrderable fault). NaN
components FLOW per the math domain's NaN-totality; the §4b
ordering contexts never receive tower types so the dev/prod knob
does not apply to them.

## T5 — Wire & saves

Hand-serialized **explicit little-endian f32 lanes** in new VAL
tags — NEVER glam's memory layout or serde: glam's internal repr
varies with SIMD features and versions; saves and seeded replays
must not. Glam computes; the wire is ours. Saves carry lanes
verbatim; new kinds = new tags, old saves unaffected, no version
gymnastics. Lane order: vecs x,y,z,w; quat x,y,z,w; mats
column-major column-by-column.

## Footnote (recorded, not solved)

Cross-platform float determinism for transcendentals (slerp,
from_euler) is bounded by the platform libm — already true of the
ruled scalar kit's sin/cos; glam does not worsen the existing
posture. Revisit only if replay divergence is ever observed in the
field.
