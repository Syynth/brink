---
"@brink-lang/web": patch
---

A shuffle sequence (`{~a|b|c}`, `{ shuffle: … }`) now removes each picked
alternative order-preservingly, matching the reference runtime's
`unpickedIndices.RemoveAt(chosen)`. brink used `swap_remove`, which moves
the last unpicked element into the hole and permutes the survivors, so from
the second draw of a loop onward it indexed a differently-ordered list and
picked a different alternative — while the first draw of each loop still
agreed, nothing having been removed yet.

Player-visible: a story's shuffles now come out in ink's order.
`tier2/conditional/shuffle` goes 0/1 → 1/1 and
`tier2/sequences/I107-shuffle-stack-muddying` 0/2 → 2/2 against the C#
oracle; the ratchet moves 5624 → 5627.

A second, independent shuffle divergence (#3538) remains open: brink's
container path for a sequence carries an implicit stitch level inklecate
does not emit, so the two seed some shuffles differently.
