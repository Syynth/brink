---
"@brink-lang/web": patch
---

NS-A7 (#1113): `Weighted[T]` + heap verbs (stdlib-spec §8, collections+).
Observable through `@brink-lang/web`, brink dialect only:

- **Five new verbs**: `weighted(w1, v1, w2, v2, …)` builds a
  `Weighted[T]` table (the brink-dialect spelling of the chartered
  `Weighted { w: v }` literal until B5) — evidence-by-construction:
  statically-malformed tables (empty, dangling weight, literal
  zero/negative/non-int weights) are the **new compile error E120** in
  both type regimes; computed weights fault at construction
  (`WeightedBadWeight`), so a table that exists is always rollable.
  `roll(w) → T` draws through the one RNG cell (seeded-deterministic,
  total over any existing table). The humble heap over ordinary arrays:
  `heap_push(ref a, x)` (statement-only, §4b dev NaN entry-fault / prod
  pinned placement), `heap_pop(ref a) → Option[T]` and
  `heap_peek(a) → Option[T]` (empty → `none`); min-heap by the §4b
  doctrine order — the same comparison core as `sort`/`min`/`max`.
- **One new opcode** `Collect` (0xFA + kind byte: `weighted_new`,
  `rand_roll`, `heap_push`, `heap_pop`, `heap_peek`) appears in
  disassembly, and **one new value form**: `Weighted` (wire tag 0x19;
  `(weighted (3 "sword") …)` in `.inkt`; construction-literal display
  `Weighted { 3: sword, … }`; F17 multiset equality —
  order-insensitive, multiplicity-sensitive; always truthy; marshals to
  JS as `[{weight, value}]` natively and a typed `weighted` entry list
  on the JSON boundary; survives SaveState round-trips).

Vanilla-ink stories are unaffected; the oracle corpus is byte-identical.
