---
"@brink-lang/web": patch
---

TM-4 (#620) foundation: `Value::Record` lands in the shared value core —
closed-shape records with an interned `ShapeId` and a flat, shape-ordered
field vector, following the exact COW/equality/serialization machinery
already ratified for `Array`/`Map` (Arc-shared field vector, `make_mut`
copy-on-write, structural equality with an `Arc::ptr_eq` fast path, plus a
shape-identity check — two records are never equal unless their shapes
match). Round-trips through `.inkb`, the `.inkt` text format, and the
runtime transcript (`.brkt`), all via the new `VAL_RECORD` (`0x0F`) wire tag
(`docs/format-v4-rfc.md` §1).

Format: the reserved `StructShapes` `.inkb` section (`0x0C`) goes live —
shape id, name, and ordered field names, wired into `write_inkb`/`read_inkb`
alongside the existing sections (`SECTION_COUNT` 11 → 12; every checked-in
`.inkb` fixture regenerates once with the extra section, per the
single-version regenerate-on-mismatch policy). Three new opcodes go live in
the previously-reserved field-op block (`0xCE`-`0xD0`): `RecordNew(shape_id)`,
`RecordGetDyn(name_id)`, `RecordSetDyn(name_id)` — the by-name field
construct/get/set ops correct in both dialects (turn-terminating fault on a
missing field, matching the existing `MapGet`/`IndexGet` fault pattern).
Static-offset field ops (`RecordGet(offset)`/`RecordSet(offset)`, the
strict-mode performance payoff `docs/typed-mode-spec.md` §6 anticipates)
stay named and numbered (`0xD1`-`0xD2`) but reserved — no `Opcode` variant
yet, the same "reserved, decode rejects" discipline `StoreVarIfNew`/`EqVars`
already established.

No compiler surface (grammar/HIR/analyzer/LIR/codegen for `STRUCT`
declarations or `Name#{…}` construction) is included in this PR — every new
opcode/section is inert until a follow-up compiler milestone emits it,
mirroring how T1a's collection-value reservation preceded T1b's live
grammar/codegen wiring. See the PR description's scope note for what
remains open against issue #620.

Oracle corpus: unchanged, 5,577 passing episodes — nothing in the compiler
pipeline changes; the new surface is reachable only through direct
hand-assembled bytecode (this PR's own VM tests) and the `.inkb`/`.inkt`/
transcript round-trip tests.
