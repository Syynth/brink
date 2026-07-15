// ALGORITHMS CORPUS — sorting/searching lane (issue #822)
//
// TYPES POLICY: gradual (default). Every value is an int or an int array;
// `types = strict` has nothing to escape-check here.
//
// SEEDED RNG NOTE (per issue #822): brink already ships a deterministic,
// seedable RNG reachable from ink source — vanilla ink's own `RANDOM(min,
// max)` (inclusive both ends) plus `SEED_RANDOM(seed)`, both dialect-
// independent core builtins (not a T1b/brink-only extension), backed at
// the runtime by whichever `StoryRng` the embedding chooses (this corpus's
// test harness uses `DotNetRng` — see `algorithms_corpus.rs` — matching
// every other `tests/tier1-brink/` case and the oracle-comparison harness
// itself). A hand-rolled in-ink PCG generator is therefore NOT needed for
// this lane: `SEED_RANDOM` + `RANDOM` already gives bit-for-bit
// reproducible output across runs and across the whole corpus's shared RNG
// story. (A future randomness-lane entry in this epic — the catalog's own
// "PCG" row — is about porting a PCG *as an algorithm*, a different goal
// from what this file needs, which is just "a seed knob".)
//
// ERGONOMICS-FINDINGS:
// - `RANDOM(0, i)` reads naturally as the inclusive-range draw
//   Fisher–Yates needs (`0..=i`, not `0..i`) — no off-by-one translation
//   required from the textbook algorithm description, unlike languages
//   whose native RNG is exclusive-upper-bound by convention.
// - In-place swap is `temp t = arr[i]` + two indexed assignments — same
//   "no tuple/multi-assignment swap sugar" shape as `insertion-sort`'s
//   finding in this lane; consistent, not a new gap, but worth noting it
//   recurs in every in-place-mutation port so far.
// - Determinism is provable at the corpus level, not just asserted: this
//   file's own golden `expected.txt` IS the reproducibility proof — the
//   harness re-runs `SEED_RANDOM(1729)` fresh every test invocation and
//   must land on byte-identical output, or the test fails outright.

VAR arr = #[1, 2, 3, 4, 5, 6, 7, 8]

~ SEED_RANDOM(1729)
~ {
    temp i = len(arr) - 1
    while i > 0 {
        temp j = RANDOM(0, i)
        temp t = arr[i]
        arr[i] = arr[j]
        arr[j] = t
        i = i - 1
    }
}

Shuffled: {arr}.
-> END
