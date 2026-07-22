---
"@brink-lang/web": patch
---

NS-A6 (#1112): rng-as-cell formalization + the `std::rand` draw verbs.
The RNG is formalized as ONE named runtime state cell (the
`rng_seed`/`previous_random` pair stories have always saved), owned by
`std::rand` and named `DefinitionId::RNG_CELL` in the effect-row space —
every draw is an ordinary **write** to it (no new row dimension), on both
surfaces: the frozen ink `RANDOM`/`SEED_RANDOM`/`LIST_RANDOM` and the new
brink-dialect verbs. Observable through `@brink-lang/web`:

- New brink-dialect intrinsics (lowercase, E035-shadowable,
  strict-ink-gated): `float()` → uniform `[0,1)` (nullary; the unary
  `float(x)` conversion is unchanged — arity disambiguates, and any other
  arity is an E031 naming both forms) · `chance(p)` → bool (F3 ruled:
  `p` clamped to `[0,1]`, NaN → false, total; always exactly one draw) ·
  `pick(coll)` → `Option[T]` over arrays and flags subsets (empty →
  `none`, no draw; ranges deferred to A5) · `shuffle(a)` statement-only
  in-place Fisher-Yates (E056 in expression position, E058 arity, E055
  rvalue receiver) · `shuffled(a)` functional twin · `seed(n)`
  statement-only, lowering to the frozen `SEED_RANDOM` op (one cell, two
  surfaces, no drift). `rand::int` is deliberately NOT shipped — it
  arrives with A5's inhabited-range refinement.
- Four new opcodes (`rand_float` 0xEC, `rand_chance` 0xED, `rand_pick`
  0xEE, `rand_shuffle` 0xEF) with inkt spellings.
- The draw algorithm is pinned as a stability contract (per-draw
  `seed = rng_seed + previous_random` chain, 24-bit-exact `[0,1)` float
  shaping, top-down Fisher-Yates); seeded replay is transcript-identical
  and the cell round-trips through saves exactly as before.
- Effect-surface: draw-bearing defs show the write in their row, so
  `@[effects(pure)]` exceedance (E103) now names `rng`, a new `writes:
  rng` clause spelling covers draw-bearing defs (a user cell named `rng`
  shadows it), and wake conditions calling draw-bearing defs are rejected
  by the existing purity gate (E105).

Vanilla-ink stories are unaffected; the oracle corpus is byte-identical.
