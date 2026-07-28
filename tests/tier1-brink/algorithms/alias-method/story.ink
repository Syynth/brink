// ALGORITHMS CORPUS — randomness lane (issue #822)
// Alias method (Vose's algorithm): build a table once so that every
// SUBSEQUENT weighted draw is O(1) — two array reads and a coin flip —
// instead of `weighted-loot-table`'s O(n) cumulative-array scan. The
// payoff matters when a table gets rerolled every frame/tick rather than
// occasionally (the catalog's own framing: "reroll a big loot/encounter
// table every frame without re-scanning cumulative weights").
//
// LICENSE NOTE (per issue #822's catalog, flagged row): the catalog
// explicitly flags Keith Schwarz's widely-cited "Darts, Dice, and Coins"
// article (keithschwarz.com) as having no explicit license grant and
// prefers Wikipedia/the paper instead. This port's construction algorithm
// is written from Wikipedia's "Alias method" article (CC BY-SA, prose
// read for the shape of the two-worklist construction, not transcribed)
// and Vose, "A Linear Algorithm for Generating Random Numbers with a
// Given Distribution" (IEEE TSE 1991) for provenance — keithschwarz.com is
// not consulted or cited anywhere in this file.
//
// INCLUDE — reuses `pcg-rng`'s shared `pcg.ink` utility for BOTH random
// numbers the per-draw step needs (a uniform index AND a uniform unit
// float — see `next_below`/`next_unit_float` below), the second
// confirmed consumer of that shared utility alongside
// `weighted-loot-table` next door.
INCLUDE ../pcg-rng/pcg.ink

// TYPES POLICY: gradual (default). `Array<int>` (weights), `Array<float>`
// (the scaled-probability worklists and the final probability table),
// `Array<string>` (names) — gradual inference handles the float/int mix
// without annotation ceremony.
//
// ERGONOMICS-FINDINGS:
// - THE two-worklist construction the catalog predicted would be
//   "fussy to get exactly right" held up exactly that way: `small`/
//   `large` are both plain arrays used as stacks (`temp x =
//   arr[len(arr)-1]` + `remove_at(arr, len(arr)-1)` to pop — the same
//   pattern `dfs-grid-path`'s explicit backtracking stack already uses
//   in this corpus, reused here for a completely different algorithm,
//   which is exactly the "same friction shape recurring" signal this
//   epic's findings-aggregation step is meant to catch).
// - Pre-sizing `prob_table`/`alias_table` needed a `push`-filled loop
//   BEFORE any indexed assignment — direct index-assignment only updates
//   an EXISTING slot (confirmed by `longest-common-subsequence`'s
//   already-documented finding: "an out-of-bounds array index needs
//   `push`, not assignment"); there is no "make an array of size n,
//   default-filled" constructor, so every fixed-size-array algorithm in
//   this corpus (this one included) pays a small "declare then fill"
//   ceremony tax up front.
// - Two independent RNG draws per sample (`next_below(n)` for the bucket
//   index, `next_unit_float()` for the coin flip) both thread the SAME
//   `rng_state` global forward in sequence — no aliasing/ordering bug
//   surfaced, but it is worth noting explicitly that "one draw per
//   sample" (`weighted-loot-table`'s shape) and "two draws per sample"
//   (this file's shape) both fell out naturally from the same
//   `pcg_next`/`pcg_below` primitives with zero special-casing needed
//   for the second draw.
// - `float(draw.value) / float(PCG_OUTPUT_RANGE)` — the same "int output,
//   divide by range for a `[0, 1)` float" pattern `value-noise-field`'s
//   `hash_to_unit` already established; the randomness lane didn't need
//   to invent a new idiom for it, a mild positive finding.
// - Float-printing noise (`f32` rounding, e.g. `0.4` may print with
//   trailing digits) applies to `prob_table`'s printed values exactly as
//   `value-noise-field`/`utility-ai` already documented — not re-derived
//   here, just confirmed to reproduce again.

VAR rng_state = 0

VAR names = #["gold", "silver", "bronze", "stone"]
VAR weights = #[4, 3, 2, 1]

VAR prob_table = #[]
VAR alias_table = #[]

VAR draws = #[]
VAR tally = #{}

~ {
    rng_state = pcg_seed(424242)

    temp built = build_alias_table(weights)
    prob_table = built[0]
    alias_table = built[1]

    temp i = 0
    while i < 20 {
        temp pick = alias_draw(prob_table, alias_table)
        temp picked_name = names[pick]
        push(draws, picked_name)
        if contains(tally, picked_name) {
            tally[picked_name] = tally[picked_name] + 1
        } else {
            insert(tally, picked_name, 1)
        }
        i = i + 1
    }
}

Weights: {weights}. Prob table: {prob_table}. Alias table: {alias_table}.
20 draws: {draws}.
Tally: {tally}.
-> END

=== function next_below(bound) ===
~ {
    temp draw = pcg_below(rng_state, bound)
    rng_state = draw.state
    return draw.value
}

=== function next_unit_float() ===
~ {
    temp draw = pcg_next(rng_state)
    rng_state = draw.state
    return float(draw.value) / float(PCG_OUTPUT_RANGE)
}

// Vose's two-worklist construction. Returns `#[prob_table, alias_table]`
// (an array-of-two-arrays stand-in for a tuple return, since brink has no
// tuple type — the same "collapse a multi-value return into an array"
// shape this corpus's other multi-output functions already use).
=== function build_alias_table(weights) ===
~ {
    temp n = len(weights)
    temp total = 0
    temp i = 0
    while i < n {
        total = total + weights[i]
        i = i + 1
    }

    temp scaled = #[]
    i = 0
    while i < n {
        push(scaled, float(weights[i]) * float(n) / float(total))
        i = i + 1
    }

    temp prob_table = #[]
    temp alias_table = #[]
    i = 0
    while i < n {
        push(prob_table, 0.0)
        push(alias_table, 0)
        i = i + 1
    }

    temp small = #[]
    temp large = #[]
    i = 0
    while i < n {
        if scaled[i] < 1.0 {
            push(small, i)
        } else {
            push(large, i)
        }
        i = i + 1
    }

    while len(small) > 0 and len(large) > 0 {
        temp g = small[len(small) - 1]
        remove_at(small, len(small) - 1)
        temp l = large[len(large) - 1]
        remove_at(large, len(large) - 1)

        prob_table[g] = scaled[g]
        alias_table[g] = l

        scaled[l] = scaled[l] - (1.0 - scaled[g])
        if scaled[l] < 1.0 {
            push(small, l)
        } else {
            push(large, l)
        }
    }

    // Leftover entries land at probability 1.0 exactly (floating-point
    // drift is the only reason anything remains once one worklist is
    // empty — see the header's construction-friction finding).
    while len(large) > 0 {
        temp l = large[len(large) - 1]
        remove_at(large, len(large) - 1)
        prob_table[l] = 1.0
    }
    while len(small) > 0 {
        temp g = small[len(small) - 1]
        remove_at(small, len(small) - 1)
        prob_table[g] = 1.0
    }

    return #[prob_table, alias_table]
}

// O(1) draw: one bucket index, one coin flip.
=== function alias_draw(prob_table, alias_table) ===
~ {
    temp i = next_below(len(prob_table))
    temp r = next_unit_float()
    if r < prob_table[i] {
        return i
    }
    return alias_table[i]
}
