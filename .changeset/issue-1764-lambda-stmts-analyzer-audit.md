---
"@brink-lang/web": patch
---

Analyzer: four diagnostic passes now see inside a block-bodied lambda's
statements (issue #1764, the audit umbrella over #1749's effect-row
instance).

Each of these passes has a hand-written recursion for file-level
`VAR`/`CONST` initializers — the one position the shared HIR visitor does
not cover — and every one of them stopped at a lambda's *trailing value
expression*, silently skipping everything the lambda does before it. A
construct inside `|…| { let x = …; … }` was therefore invisible to:

- **`E106` / `E138`** — map-literal key domain and duplicate keys;
- **`E069` / `E070` / `E071` / `E084`** — struct construction shape
  agreement and duplicate fields;
- **`E078`** — the `int(x)` / `float(x)` conversion domain;
- **`E152`** — a statically always-false `contains(map, needle)`;
- **`E066`** — `or`-coalescing type mismatch. This one also fed the gate
  on building a coalesce table at all and the lookup lowering reads, so a
  chain's recorded *shape* went missing too (absence is safe by design —
  the effect is a lost static shape, not a miscompile).

Native (`.brink`) source only: lambdas exist on no other surface. Vanilla
ink stories are unaffected and the oracle corpus is byte-identical.
