// Snapshot-retention cost curve (issue #821 Workstream C) — point
// (G=100, M=100): 100 retained generations, 100 mutations per generation
// (10,000 total mutations — the largest point on the curve, 10x both
// dimensions of g10-m10). See snapshot-retention-g10-m10/story.ink's
// header for the full mechanism argument. Expected `cow_copies` tracks
// G alone (same as g100-m10, despite 10x the mutation count) — the far
// corner of the (G, M) matrix that pins down the claim across two orders
// of magnitude in both dimensions. (Measured `cow_copies` is G+1, not
// G — see docs/runtime-bench.md for the shared one-time offset.)
VAR live = 0
VAR history = 0
VAR total = 0
~ {
    live = #[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    history = #[]
    temp g = 0
    while g < 100 {
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
