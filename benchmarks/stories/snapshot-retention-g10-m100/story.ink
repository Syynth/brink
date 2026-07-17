// Snapshot-retention cost curve (issue #821 Workstream C) — point
// (G=10, M=100): 10 retained generations, 100 mutations per generation
// (1,000 total mutations, 10x snapshot-retention-g10-m10's mutation
// count at the same retention depth). See that program's header for the
// full mechanism argument. Comparing this program's `cow_copies` against
// g10-m10's is the isolation that proves the claim: both should report
// the *same* `cow_copies` count despite a 10x difference in total
// mutation count, since the cost is bounded per-generation (one copy
// per share-then-mutate divergence), not per-mutation. (Measured
// `cow_copies` is G+1, not G — see docs/runtime-bench.md for the
// one-time `history`/`#[]` offset this and every sibling point shares;
// it doesn't change the comparison's conclusion.)
VAR live = 0
VAR history = 0
VAR total = 0
~ {
    live = #[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    history = #[]
    temp g = 0
    while g < 10 {
        push(history, live)
        temp m = 0
        while m < 100 {
            live[0] = g * 1000 + m
            m = m + 1
        }
        g = g + 1
    }
    total = len(history)
}
{total}
-> END
