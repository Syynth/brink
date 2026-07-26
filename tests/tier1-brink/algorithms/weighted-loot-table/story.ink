// ALGORITHMS CORPUS — randomness lane (issue #822)
// Weighted loot table: roll a random reward from a pool where each entry
// has a different chance to be picked (a "common ore" drops far more
// often than a "legendary relic") — the single most common randomness
// need in an actual game's loot/encounter system, and the #521
// exotic-natives evidence case named directly in the issue: the parked
// exotic-natives epic proposes "Weighted tables — loot/random tables with
// seeded, journal-deterministic draws" as a candidate first-class native
// type; this file is what hand-rolling that same feature looks like in
// TODAY's brink, with no native support at all, so the friction below is
// real design evidence for that future round, not a hypothetical.
//
// INCLUDE — reuses `pcg-rng`'s shared `pcg.ink` utility (see that file's
// header) rather than vanilla ink's `RANDOM`/`SEED_RANDOM`: a loot table
// is exactly the kind of long-lived game system whose RNG stream a real
// game would want to own as an explicit, savable value scoped to just
// this table (so reloading a save doesn't reset every OTHER system's
// draw sequence too) — the same rationale pcg.ink's header documents.
INCLUDE ../pcg-rng/pcg.ink

// TYPES POLICY: gradual (default). `array<LootEntry>`, `array<int>`
// (cumulative weights), `map<string, int>` (tally) — gradual inference
// resolves all of it without annotation ceremony.
//
// ERGONOMICS-FINDINGS:
// - THE #521 EVIDENCE: there is no native "weighted table" value at all.
//   Hand-rolling one needs (a) a `STRUCT LootEntry` to pair a name with a
//   weight, since brink has no tuple type, (b) a manually-built parallel
//   `cumulative` array (`build_cumulative` below), and (c) a linear scan
//   per draw (`draw_index`) to find which bucket a random number lands
//   in — three separate pieces of ceremony for what a first-class
//   weighted-table type (per #521's own proposal: "loot/random tables
//   with seeded, journal-deterministic draws") would collapse into one
//   declaration + one `draw(table)` call. This is the exact friction
//   #521 predicted and this file is the hand-rolled evidence for it.
// - The linear scan is O(n) per draw — fine for this file's 5-entry
//   table, but see `alias-method`/story.ink next door for the O(1)
//   alternative (Vose's algorithm) and its own, different, friction
//   profile. Two techniques for the same problem, two different
//   friction shapes — worth reading both findings together.
// - `remove_at(small_or_large, ...)`-style pop-from-array-as-stack is NOT
//   needed here (that's `alias-method`'s table-construction phase) —
//   `weighted-loot-table` only needs a forward scan, which is the
//   simpler of the two randomness-lane data-shape stories.
// - Reusing `pcg.ink` across two sibling files (this one and
//   `alias-method`) via a relative `INCLUDE ../pcg-rng/pcg.ink` worked
//   with zero friction — brink's `INCLUDE` path resolution (string-based,
//   `../`-aware) handles a shared-utility-in-a-sibling-directory layout
//   exactly like a conventional module import would, once resolved.
// - `rng_state` is a plain top-level `VAR`, threaded through by
//   `next_below` re-assigning it after every draw — see `pcg.ink`'s
//   header for why the library itself returns state explicitly instead
//   of mutating a hardcoded global.

STRUCT LootEntry = #{
    name: string,
    weight: int,
}

VAR rng_state = 0

VAR table = 0
VAR cumulative = #[]
VAR total_weight = 0

VAR rolls = #[]
VAR tally = #{}

~ {
    table = #[LootEntry#{name: "common_ore", weight: 50}, LootEntry#{name: "iron_sword", weight: 25}, LootEntry#{name: "healing_potion", weight: 15}, LootEntry#{name: "rare_gem", weight: 8}, LootEntry#{name: "legendary_relic", weight: 2}]

    cumulative = build_cumulative(table)
    total_weight = cumulative[len(cumulative) - 1]

    rng_state = pcg_seed(90210)

    temp i = 0
    while i < 20 {
        temp pick = draw_index(cumulative, total_weight)
        temp entry_name = table[pick].name
        push(rolls, entry_name)
        if contains(tally, entry_name) {
            tally[entry_name] = tally[entry_name] + 1
        } else {
            insert(tally, entry_name, 1)
        }
        i = i + 1
    }
}

Table (name, weight out of {total_weight}): {table}.
20 rolls: {rolls}.
Tally: {tally}.
-> END

// Manual cumulative-weight array: cumulative[i] is the running total of
// table[0..=i]'s weights, so a draw becomes "find the first cumulative
// entry the random number is strictly less than".
=== function build_cumulative(table) ===
~ {
    temp out = #[]
    temp running = 0
    temp i = 0
    while i < len(table) {
        running = running + table[i].weight
        push(out, running)
        i = i + 1
    }
    return out
}

=== function next_below(bound) ===
~ {
    temp draw = pcg_below(rng_state, bound)
    rng_state = draw.state
    return draw.value
}

// Linear scan (O(n)) over the cumulative array — the exact O(1)-vs-O(n)
// contrast `alias-method`'s header discusses.
=== function draw_index(cumulative, total_weight) ===
~ {
    temp r = next_below(total_weight)
    temp i = 0
    while i < len(cumulative) {
        if r < cumulative[i] {
            return i
        }
        i = i + 1
    }
    return len(cumulative) - 1
}
