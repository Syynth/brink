---
"@brink-lang/web": patch
---

Issue #2240: `brink-ir`'s struct-shape table builder no longer silently
drops a declared `STRUCT` when its own definition can't be resolved.

- **New diagnostic `E181`** (non-suppressible backstop, the `E060`/`E073`
  posture): `lir::lower::structs::build_shape_table` raises it if a
  declared struct's own self-declaration lookup comes back `None` — the
  narrow case where an analyzer-dropped intra-module duplicate's only
  surviving same-name sibling is itself std-declared. Before this, the
  struct silently vanished from the shape table and the seeded name table,
  shifting every subsequent `ShapeId`/`NameId` and the bytecode built from
  them with no diagnostic at all.
- Not reachable from any project compilable today (the standard library
  ships a single preset file, so an intra-*std* duplicate can't yet arise)
  — this closes the gap for when it becomes reachable, and makes the
  compiler's own invariant violation loud instead of silent if it is ever
  hit some other way.
- `lir::lower::structs::build_struct_shape_data` (the `NameId`-free
  cutoff-friendly twin `brink-db`'s `struct_shape_data_query` memoizes)
  performs the identical lookup and deliberately does **not** duplicate
  this diagnostic — see its own doc comment and `E181`'s doc for why: it
  is a pure `Eq`-cutoff salsa data query with no diagnostic sink to push
  into, and every real compile always computes it alongside
  `build_shape_table` in the same salsa revision, over the same inputs, so
  the same drop condition always raises `E181` from that side instead.
