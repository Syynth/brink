# T1e spec — path projections

Status: **draft for ratification** (light spec pass, 2026-07-15 — the
design was ruled in the value-model round; this transcribes §7 of
`docs/value-model-spec.md` into implementable surface). Companions:
`docs/format-v4-rfc.md` (VAL_PROJECTION + Proj* opcodes, encodings
frozen), `docs/tier1-roadmap.md` §T1e, `docs/t1c-spec.md` (the ref
rules projections extend). **RULED** sections transcribe the ratified
model; **PROPOSED** items ratify at this PR's review.

## 1. The model — RULED

`ref npc.inventory[3]` creates a **symbolic projection**: a value
`(root cell, path segments)` — never an interior pointer. Reads walk
the path; writes desugar to read-modify-write on the *root cell*
(Swift `inout` copy-in/copy-out lineage). Consequences, all ratified:
COW, scope routing, speculation isolation, and ref collapsing survive
untouched; a save mid-call with a live projection works (projections
serialize like `VariablePointer`); exclusivity checking stays local
and syntactic.

The three semantics rulings (value-model §7, ratified in the closing
round):

1. **Index expressions snapshot at `ref` creation** — the segment
   list is fixed when the projection is made
   (`ref a[i]` captures the value of `i` then).
2. **Path invalidation under a live projection is a defined
   turn-terminating runtime fault** — not a clamp, not UB (e.g. the
   array shrank below the snapshot index before a read/write).
3. **Overlapping projections: immediate write-through order** — every
   write applies to the root cell at the moment it happens;
   deterministic without any aliasing check.

## 2. Grammar and creation — RULED shape, spellings PROPOSED

- v1 `ref` stays **cell-level at the declaration site**: projections
  are created only in `ref`-argument position —
  `heal(ref npc.hp, 5)`, `bind(f, ref inventory[idx])`,
  `#fn(heal, ref party[leader].hp)` — extending the existing lvalue
  grammar with the §4/T1b postfix path forms (dotted fields on
  structs, `[…]` indexing, chains).
- The root must be a durable cell (VAR / `#@local`) — the T1c rule
  unchanged; `temp` roots remain a compile error (E080 family).
- **PROPOSED**: no standalone projection *expressions* in T1e
  (`temp r = ref a[0]` stays illegal — projections exist only where
  `ref` already exists: argument binding). Standalone first-class
  refs are a future round if ever wanted.

## 3. Runtime and wire — RULED

- `Value::Projection { cell, segments }` mirroring the frozen
  `VAL_PROJECTION` encoding (cell reference = `VAL_VAR_POINTER`
  payload shape; segments: `0=index i32`, `1=key value`).
- Opcodes per the RFC's named reservations: `MakeProjection(desc)`,
  `ProjRead`, `ProjWrite` — first emission of that reserved block.
  `ProjWrite` implements root-cell RMW: take root → walk →
  `make_mut` spine → write → store back.
- `.inkt` atoms land WITH the reader (dump parity; the #742 lesson).
- Saves/journal/speculation: ordinary values; rehydration validates
  the root cell like `VariablePointer` today, and the `#@was` alias
  table applies to the root's identity on the miss path.

## 4. Semantics details — RULED + PROPOSED edges

- Reads walk the snapshot path against the root's *current* value;
  a missing key / out-of-range index at read or write time is the
  §1(2) fault (`ProjectionInvalidated`), consistent with §11c.
- Struct-field segments resolve by field name against the shape at
  access time; a field removed by recompile faults at rehydration
  (name/mode validation — the T1c pattern).
- **PROPOSED**: `string(p)` display form `ref gold` /
  `ref npc.inventory[3]` (root + rendered path, boring and stable);
  equality structural (same root cell + equal segments); not a map
  key; no ordering.
- **PROPOSED**: effects — a projection-typed `ref` param contributes
  its ROOT CELL to the row (writes: {root}) exactly like a plain
  `ref` param; path granularity in rows is explicitly NOT attempted
  (mirrors the entity-granular-capability reservation: the slot
  exists in the factored encoding if ever wanted).

## 5. Borrow analysis — RULED (doctrine restated)

An **optimizer, not a gatekeeper**: soundness never depends on it
(fallback is per-access path-walking RMW); when the compiler proves
exclusivity it may hold the `make_mut` spine across a region. It can
be incomplete and arrive later — backlog item, not in this milestone.

## 6. Diagnostics — PROPOSED

From the next free code: non-durable projection root (reuse the E080
family message shape), projection segment on a statically-known
non-collection (strict mode, existing mismatch machinery), plus the
runtime `ProjectionInvalidated` fault variant (not a diagnostic).

## 7. Testing — PROPOSED

- Oracle ratchet byte-identical (vanilla ink has no path refs).
- tier1-brink wing: projection through fn values (`#fn` ref-binding a
  path), mutation visibility through overlapping projections
  (write-through order), snapshot-at-creation (index var mutated
  after creation), invalidation faults (shrunk array, removed key),
  save/load mid-call with a live projection, `#@was` root rename.
- Property: RMW-through-projection ≡ manual take/mutate/write-back
  (extends the lane-B law); display stability; codec round-trips per
  pair (inkb/inkt/transcript — the wave-11 lesson).

## 8. Sequencing — PROPOSED (single reviewed agents, oracle-gated)

1. **T1e-1 grammar + HIR + analyzer**: path forms in ref-argument
   position, durable-root enforcement, snapshot semantics in
   lowering prep, diagnostics. Rejects at lowering (the E052 fence
   pattern).
2. **T1e-2 LIR + codegen + VM**: MakeProjection/ProjRead/ProjWrite
   emission, RMW discipline, faults, persistence + rehydration,
   `.inkt` atoms, corpus wing.
3. **T1e-3 tail**: bevy-brink pass-through audit (projections cross
   as values; host never walks paths), book section, IDE hover
   (projection display form), fmt.
