// ALGORITHMS CORPUS — randomness lane (issue #822)
// PCG (permuted congruential generator, PCG-STYLE): a deterministic,
// seedable RNG stream, ported here as an algorithm in its own right (per
// the catalog's own framing: "arguably foundational infrastructure rather
// than a standalone port") AND factored as `pcg.ink` — a shared
// in-corpus utility — because `weighted-loot-table` and `alias-method`
// (this same lane) want RNG state as an inspectable `int` they own, not
// vanilla ink's opaque built-in `RANDOM`.
//
// INCLUDE — see pcg.ink for the full library, its provenance citations
// (Wikipedia/pcg-random.org, NOT keithschwarz.com — this file is PCG, not
// the alias method, but the same "prefer Wikipedia/the paper" principle
// applies per the catalog's methodology note), and the load-bearing
// finding (no bitwise XOR/rotate operator exists in brink, so a faithful
// PCG permutation step is not expressible — the honest ceiling of what
// "PCG-style" can mean here).
INCLUDE pcg.ink

// TYPES POLICY: gradual (default). Every value is an `int` or a
// `PcgDraw` struct; nothing here escapes gradual inference.
//
// ERGONOMICS-FINDINGS:
// - The hard blocker the catalog predicted is real (see pcg.ink's
//   `pcg_output` header comment for the full writeup): no XOR, no
//   rotate, no shift. `int` being 32-bit rather than PCG's native 64-bit
//   state is a SECOND, independent gap — real PCG32 keeps 64 bits of LCG
//   state and outputs the top 32 after permutation; this port's state
//   IS the full 32 bits, so there is no "extra headroom" to permute out
//   of. Both gaps compound: even if brink had bitwise ops tomorrow, a
//   bit-exact port of real PCG32 would still need a wider integer type
//   first. This is the single sharpest "what's missing" finding in the
//   whole randomness lane, exactly the kind of thing this epic exists to
//   surface (issue #822's own words: "could be a hard blocker worth
//   surfacing early, before other randomness entries depend on it").
// - Despite that, "PCG-STYLE" (the issue's own qualifier, not "PCG") is
//   achievable and IS useful: a small, explicit, save/resumable `int`
//   state plus a state-transition/output split that mirrors the real
//   algorithm's shape closely enough to be recognizable, deterministic
//   across runs (this file's own golden transcript is the proof), and
//   good enough for a test corpus's fixed, small-N sampling needs.
// - Threading `state` through `next_raw`/`next_below` below (rather than
//   letting `pcg_next`/`pcg_below` mutate a global directly) is a
//   DELIBERATE wrapper choice, not a language limitation: brink functions
//   easily COULD read/write a top-level `VAR` as a side effect
//   (`memoized-fibonacci`'s `memo`/`calls` do exactly that) — the
//   explicit-return-value style is used here because `pcg.ink` is meant
//   to be INCLUDEd by files with their OWN naming for the RNG-state
//   variable (`weighted-loot-table`'s `rng_state`, `alias-method`'s
//   `rng_state`), and a library that reached into a hardcoded global
//   name would only work if every consumer used that exact name — a
//   real API-design tradeoff a hand-rolled ink "library" has to make
//   explicitly, since brink has no module-scoped/private state to hide
//   it behind.
// - `PcgDraw#{value: ..., state: ...}` — returning a two-field struct
//   from a function composes cleanly with destructuring-by-field-access
//   (`draw.value`, `draw.state`); no tuple-return sugar needed or missed.

VAR seed = 20260716
VAR seeded_state = 0

VAR raw_values = #[]
VAR below_values = #[]

~ {
    seeded_state = pcg_seed(seed)

    temp s = seeded_state
    temp i = 0
    while i < 10 {
        temp draw = pcg_next(s)
        push(raw_values, draw.value)
        s = draw.state
        i = i + 1
    }

    temp s2 = seeded_state
    i = 0
    while i < 10 {
        temp draw = pcg_below(s2, 100)
        push(below_values, draw.value)
        s2 = draw.state
        i = i + 1
    }
}

Seed: {seed}. Seeded state: {seeded_state}.
Raw values (10 draws, range [0, {PCG_OUTPUT_RANGE})): {raw_values}.
Bounded draws (10 draws, range [0, 100)): {below_values}.
-> END
