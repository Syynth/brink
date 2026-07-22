// ALGORITHMS CORPUS — randomness lane (issue #822)
// PCG-STYLE RNG — SHARED IN-CORPUS UTILITY.
//
// INCLUDEd by `story.ink` (this directory's own demo port) and reused by
// `weighted-loot-table` and `alias-method` (sibling randomness-lane
// entries that want RNG state as an explicit, inspectable `int` value they
// own — not vanilla ink's built-in `RANDOM`/`SEED_RANDOM`, whose internal
// state lives in the runtime and isn't a value ink code can read, pass
// around, or store next to the rest of a system's own save-relevant state).
// See `story.ink` in this directory for the full ergonomics-findings
// header — this file is the library half, kept comment-light so the two
// consuming lanes aren't duplicating the same prose.
//
// PROVENANCE: shape (separate state-transition / output functions) follows
// the PCG family's own design as described at pcg-random.org (M.E.
// O'Neill) and Wikipedia's "Permuted congruential generator" article (CC
// BY-SA) — cited for provenance, not transcribed. The state-transition
// constants (multiplier 1664525, increment 1013904223) are the classic
// 32-bit LCG parameters from the "Numerical Recipes" table reproduced on
// Wikipedia's "Linear congruential generator" article (CC BY-SA, the
// parameter table itself, not prose). The output-mixing multiplier
// (-1640531535, the signed 32-bit form of 0x9E3779B9 / 2654435769 — Donald
// Knuth's multiplicative-hashing constant derived from the golden ratio)
// is likewise public arithmetic knowledge, described on Wikipedia's "Hash
// function" article (CC BY-SA). None of this derives from
// keithschwarz.com's "Darts, Dice, and Coins" article — issue #822's
// catalog comment flags that source specifically for the ALIAS METHOD, not
// PCG, but it is avoided here too on the same "prefer Wikipedia/the paper"
// principle.

STRUCT PcgDraw = #{
    value: int,
    state: int,
}

CONST PCG_MULT = 1664525
CONST PCG_INC = 1013904223
CONST PCG_MIX = -1640531535
CONST PCG_OUTPUT_RANGE = 1000000007

// C-style truncating `%` (confirmed by value-noise-field/story.ink's
// finding) means a single `%` can return negative — normalize to
// `[0, m)` via the standard "mod twice" idiom. `m` must be small enough
// that `r + m` cannot itself overflow `i32` (true for every call site in
// this lane: `PCG_OUTPUT_RANGE` and every caller-supplied `bound` are well
// under `i32::MAX / 2`).
=== function pcg_nonneg_mod(x, m) ===
~ {
    temp r = x % m
    r = (r + m) % m
    return r
}

// State-transition function: a 32-bit LCG step. `int` is 32-bit and wraps
// silently on overflow (confirmed empirically by value-noise-field's own
// finding), which is exactly the modular arithmetic an LCG wants — no
// explicit `mod 2^32` needed, the VM's own overflow behavior provides it
// for free.
=== function pcg_advance(state) ===
~ return state * PCG_MULT + PCG_INC

// Output function: real PCG's "permuted" half applies a data-dependent
// xorshift then a data-dependent bit ROTATE to the LCG state before
// exposing it as output — that is the entire point of the algorithm (an
// LCG alone has notoriously weak low-order bits; the permutation step is
// what makes PCG's output pass modern statistical test suites despite a
// cheap linear core). Brink has NEITHER a bitwise XOR nor a rotate/shift
// operator (`^` exists but is ink's LIST-intersection operator, not
// integer XOR — the same gap value-noise-field's header already flagged).
// This is precisely the "may reveal a hard blocker" friction issue #822's
// own catalog predicted for this entry, confirmed: a faithful PCG
// permutation step is NOT expressible in brink today. The best available
// substitute is a second multiplicative mix (odd constant, Knuth's
// golden-ratio hash multiplier) folded into `PCG_OUTPUT_RANGE` — this
// measurably improves on exposing the raw LCG state directly (whose low
// bits are periodic and weak) but is NOT a claim of PCG-quality output;
// it's the honest ceiling of what "PCG-style" can mean without bitwise
// primitives. THE FINDING, not a hedge: any brink port that actually needs
// production-grade random quality (not just "deterministic and
// good-enough for a demo/test corpus") is blocked on bitwise operators
// landing in the language first.
=== function pcg_output(state) ===
~ {
    temp mixed = state * PCG_MIX
    return pcg_nonneg_mod(mixed, PCG_OUTPUT_RANGE)
}

// Mix an arbitrary caller-supplied seed into the LCG's state space (an
// unmixed seed like `1` would otherwise put the generator in a
// low-quality, highly-correlated-with-`0` starting state for several
// steps).
=== function pcg_seed(seed) ===
~ return pcg_advance(seed)

// Advance one step and produce a draw in `[0, PCG_OUTPUT_RANGE)` alongside
// the next state — callers thread `state` themselves (see `story.ink`'s
// `next_raw`/`next_below` wrapper-over-a-global-VAR pattern, reused
// verbatim by `weighted-loot-table` and `alias-method`). Returning BOTH
// the value and the next state as one `PcgDraw` struct is the direct
// consequence of brink functions being pure w.r.t. the caller's locals: a
// function can't silently mutate a variable in the calling scope the way
// C's `rand()` mutates hidden global state, so the "next state" has to be
// an explicit return value, not a side effect, for a hand-rolled RNG.
// (Vanilla ink's own `RANDOM`/`SEED_RANDOM` sidesteps this entirely by
// keeping its state in the runtime, not in an ink-visible value — see this
// file's header for why that isn't what this utility is for.)
=== function pcg_next(state) ===
~ {
    temp new_state = pcg_advance(state)
    temp value = pcg_output(new_state)
    return PcgDraw#{value: value, state: new_state}
}

// Convenience: a draw reduced to `[0, bound)`, still returning the next
// state alongside it.
=== function pcg_below(state, bound) ===
~ {
    temp draw = pcg_next(state)
    return PcgDraw#{value: pcg_nonneg_mod(draw.value, bound), state: draw.state}
}
