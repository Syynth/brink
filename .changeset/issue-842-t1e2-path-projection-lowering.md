---
"@brink-lang/web": patch
---

T1e-2: real `MakeProjection`/`ProjRead`/`ProjWrite` lowering, root-cell RMW,
persistence, and `.inkt` support for path projections (docs/t1e-spec.md
§3/§4, issue #842, tracking #828). Replaces the T1e-1 `E099` lowering fence
for every real path-projection ref-argument (`heal(ref npc.hp, 5)`,
`#fn(heal, ref party[leader].hp)`) with genuine execution.

- **`Value::Projection`** (wire tag `VAL_PROJECTION`, first emission of that
  reserved tag): `(root cell, ordered segments)`, each segment `Index(i32)`
  or `Key(Value)` — the range-segment kind (`2`) stays RESERVED, never
  emitted (icebox #829, sequence slices). Structural equality (same root +
  equal segments), `Arc`-wrapped for O(1) clone.
- **`MakeProjection` opcode**: emitted at every real path-projection
  `ref`-argument creation site — index/field-name segment expressions
  evaluate once, in source order (snapshot-at-creation, spec §1(1)).
- **Root-cell RMW** (`ProjRead`/`ProjWrite`, spec §3: take → walk →
  `make_mut` spine → write → store back): a projection-bound `ref`
  parameter's reads/writes dereference through the identical walk, reused
  by `GetTemp`/`SetTemp`/`TakeTemp`'s dispatch — purely additive, no
  behavior change for any pre-T1e program.
- **`ProjectionInvalidated`** turn-terminating runtime fault (spec §1(2)):
  a shrunk array, a removed map key, or a struct field the current shape no
  longer declares, checked at read/write time against the root's *current*
  value — never a clamp, never silent.
- **Persistence**: a projection serializes as an ordinary value; rehydration
  validates the root cell exactly like `VariablePointer` today, including
  the `#@was` alias-table miss path.
- **`.inkt` atoms** land with a reader in this same PR (the `docs/t1e-spec.md`-
  adjacent #742 discipline): `(projection <cell> (segments (index N) |
  (key V) …))`, plus per-codec round-trips (`.inkb`, `.inkt`, transcript).
- Fixes a pre-existing gap in `#fn(target, ref …)` ref-argument validation
  that rejected every `ref`-marked argument (even the T1c-era bare-path
  form) as "not an lvalue" once T1e's `ref` grammar wrapper was in play —
  `#fn(heal, ref npc.hp)` now validates and lowers correctly.

Not observable through `@brink-lang/web` directly (no wasm-facing API
change), but the wire format (`VAL_PROJECTION`) and VM fault surface
(`ProjectionInvalidated`) are new behavior any consumer executing compiled
`.inkb` through the wasm runtime can now encounter, so this ships as a
patch per the wasm-observable-behavior convention.
