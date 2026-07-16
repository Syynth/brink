// ALGORITHMS CORPUS — randomness lane (issue #822)
// Shuffle bag (bag randomizer): draw from a shuffled copy of a fixed
// multiset, refilling and reshuffling only once it empties — guarantees
// "no long droughts" over any run of N draws where N <= the bag size
// (Tetris's 7-bag piece randomizer is the canonical example: you are
// mathematically guaranteed to see every piece at least once every 7
// draws, which a plain independent `RANDOM` roll every turn cannot
// promise).
//
// SEEDED RNG NOTE: deliberately vanilla ink's `RANDOM`/`SEED_RANDOM`, NOT
// this lane's shared `pcg.ink` utility (contrast `weighted-loot-table`/
// `alias-method` next door, which both reuse `pcg.ink`) — see
// `fisher-yates-shuffle`/story.ink's header for the full "why hand-rolled
// PCG isn't needed" reasoning, which applies here without modification:
// this file's only randomness need is a single seeded shuffle per
// refill, which `RANDOM`/`SEED_RANDOM` already gives byte-for-byte
// reproducibly. `weighted-loot-table`/`alias-method` reach for `pcg.ink`
// instead because THEY want RNG state as an explicit value scoped to one
// system; this file doesn't need that — the bag's own contents are
// already the interesting piece of state (see below), and the built-in
// RNG's determinism is sufficient for reshuffling it. Choosing the
// simpler tool deliberately, not an oversight, is itself the finding.
//
// TYPES POLICY: gradual (default). `array<string>` (the bag/pull-log),
// `int` (bag position); gradual inference resolves everything.
//
// ERGONOMICS-FINDINGS:
// - THE REAL FINDING (per the catalog's own framing: "trivial
//   algorithmically; the interesting part is proving the *save/resume*
//   story is airtight"): the bag's REMAINING contents at any point ARE
//   the entire piece of state a save file needs to carry — reloading
//   mid-bag must resume with the SAME remaining pieces in the SAME
//   drawn-so-far order, or a player could save-scum a bag draw by
//   reloading before an unwanted piece comes up. This file demonstrates
//   the shape that guarantee needs (`bag` as a single mutable `VAR array`
//   that shrinks as `draw_from_bag` removes from its end, refilling only
//   on empty) — a real save/resume test would assert the *exact same*
//   `bag` array value round-trips through a save/load boundary, which is
//   out of this file's scope (no save/load harness exists in this
//   corpus yet) but is exactly the follow-on worth flagging for whenever
//   this epic's save/resume variants get built.
// - Refilling reuses the EXACT Fisher–Yates in-place shuffle
//   `fisher-yates-shuffle`/story.ink already ports in this corpus's
//   sorting/searching lane — same "temp swap" shape, same `RANDOM(0, i)`
//   inclusive-range convention, copy-pasted rather than shared via
//   INCLUDE because it's a few lines and pulling in a whole sibling
//   lane's file for one loop felt like the wrong shared-utility
//   boundary (contrast `pcg.ink`, which genuinely earns being a shared
//   file because THREE files in this same lane need it).
// - `remove(bag, len(bag) - 1)` (pop-from-the-end) is the cheapest way to
//   "draw and shrink" a `VAR array` — no dedicated `pop` builtin exists,
//   but removing the LAST index specifically avoids the O(n) shift-left
//   cost `bfs-grid-path`'s `remove(arr, 0)` finding already flagged for
//   front-removal; this file's queue never needs front-removal, so it
//   sidesteps that cost entirely by drawing from the back instead.

CONST BAG_SIZE = 7

VAR bag = #[]
VAR pulls = #[]

~ {
    SEED_RANDOM(3113)

    refill_bag()
    temp i = 0
    while i < 15 {
        if len(bag) == 0 {
            refill_bag()
        }
        push(pulls, draw_from_bag())
        i = i + 1
    }
}

Bag size: {BAG_SIZE}. 15 pulls (refills mid-sequence once): {pulls}.
-> END

// Fresh full bag, one of each piece, then Fisher–Yates shuffle in place —
// same shape as `fisher-yates-shuffle`/story.ink, see this file's header
// for why it's copy-pasted rather than INCLUDEd.
=== function refill_bag() ===
~ {
    bag = #["I", "O", "T", "S", "Z", "J", "L"]
    temp i = len(bag) - 1
    while i > 0 {
        temp j = RANDOM(0, i)
        temp t = bag[i]
        bag[i] = bag[j]
        bag[j] = t
        i = i - 1
    }
}

=== function draw_from_bag() ===
~ {
    temp piece = bag[len(bag) - 1]
    remove(bag, len(bag) - 1)
    return piece
}
