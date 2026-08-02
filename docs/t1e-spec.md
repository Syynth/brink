# T1e spec — path projections

Status: **RATIFIED 2026-07-15 as current posture** — the two narrowing
choices (§2: argument-position-only; §4: root-cell-only effect rows)
are deliberate v1 posture with exploration intents recorded as icebox
issues #825 (first-class projection values) and #826 (path-granular
rows), NOT permanent rejections. Transcribes §7 of
`docs/value-model-spec.md`. Companions:
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
  **Segment kind `2=range (start i32, end i32)` is RESERVED, never
  emitted in T1e**: sequence slices/ranges are an endorsed future
  addition (icebox #829) and a slice-as-view is exactly a
  range-segment projection — the wire must not foreclose it (the
  flat-rows lesson).
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

## 4b. Related future directions — recorded, not designed

- **Vector math types (vec2/vec3)**: structs-first posture (decision
  2026-07-15, icebox #827) — the struct feature plus the future
  methods round should account for them; native types only on #822
  friction evidence, per the facility-doctrine ladder.
- **Sequence slices/ranges**: endorsed (icebox #829); design as
  range-kind projection segments per §3's reservation.

## 5. Borrow analysis — RULED (doctrine restated)

An **optimizer, not a gatekeeper**: soundness never depends on it
(fallback is per-access path-walking RMW); when the compiler proves
exclusivity it may hold the `make_mut` spine across a region. It can
be incomplete and arrive later — backlog item, not in this milestone.

## 5b. Ref-parameter argument checking is invariant — RULED 2026-08-01 (#1920/#1995)

A `ref` slot both reads *and* writes through the caller's own storage
cell, so `assignable`'s covariant widening (an `int` argument fits a
`float` slot) is unsound there: `assignable(Float, Int)` is `true`,
so `fn scale(ref x: float)` accepted an `int` cell and the callee
wrote a `float` back through storage statically declared `int`. Ruled:
`ref` parameter arguments are checked **invariantly** — the erased
argument type must match the erased parameter type exactly
(`infer::ty::ref_assignable`, row-insensitive for the same reason
`assignable` is, issue #1680 step 2). By-value arguments keep the ordinary
covariant `assignable` widening; only a `ref` slot needs the stricter
twin. Applies uniformly to every by-ref call-checking site: the
direct-call check (#1864), the UFCS-desugared receiver/argument check
(#1881), and — **outstanding**, filed as a follow-up rather than
blocking this ruling — the `#fn(target, args…)` partial-application
binding site (`infer_fn_literal`), which per §2 above is itself a
by-ref *binding* site and has no by-ref (or by-value) argument check
of any kind yet.

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
