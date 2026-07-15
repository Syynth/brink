// Snapshot-retention cost curve (issue #821 Workstream C) — point
// (G=100, M=10): 100 retained generations, 10 mutations per generation
// (1,000 total mutations — the same total as g10-m100, but reached via
// 10x the retention depth instead of 10x the mutation rate). See
// snapshot-retention-g10-m10/story.ink's header for the full mechanism
// argument. Comparing this program's `cow_copies` against g10-m100's is
// the other half of the isolation: this one should report ~10x
// g10-m100's `cow_copies` despite identical total mutation count —
// proving the cost tracks retention depth (G), not total mutations, in
// either direction. (Measured `cow_copies` is G+1, not G — see
// docs/runtime-bench.md for the shared one-time offset; the ~10x ratio
// between this program and g10-m100 still holds since both carry the
// same +1.)
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
        while m < 10 {
            live[0] = g * 1000 + m
            m = m + 1
        }
        g = g + 1
    }
    total = len(history)
}
{total}
-> END
