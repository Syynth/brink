// Snapshot-retention cost curve (issue #821 Workstream C,
// docs/value-model-spec.md §8: "a retained snapshot bounds memory at
// (retained generations + 1 live) per value — the mutator pays one COW
// copy at first divergence, history never accumulates").
//
// `history` retains G generations of `live`'s state: each generation
// pushes `live` (an Arc-share, O(1)) into `history` before mutating
// `live` M times. The claim under test: `cow_copies` (via the
// `bench-counters` feature) scales with G alone — one COW copy per
// generation, paid at the first mutation after that generation's share —
// not with G*M. The remaining M-1 mutations per generation are free
// because `live` is unique again immediately after the first one. This
// program is point (G=10, M=10) on the curve — smallest of the four
// (see the sibling snapshot-retention-* directories for G=10/M=100,
// G=100/M=10, G=100/M=100).
//
// `history` itself is never shared elsewhere, so its own growth (G
// pushes) amortizes to exactly ONE extra copy total, not one per push —
// the loop-append-10k mechanism (`history = #[]` starts from the shared
// empty-array-literal pool; the first `push` diverges from it, one COW;
// every push after that finds `history` uniquely owned). That one-time
// cost is the same for every point on this (G, M) curve regardless of G
// or M, so measured `cow_copies` across the whole matrix is **G + 1**,
// not G — see docs/runtime-bench.md's baseline table for the measured
// numbers and this same explanation spelled out once. What grows
// `history`'s memory footprint is the G retained Arc-shares of `live`'s
// snapshots at each generation boundary, which is exactly the "(retained
// generations + 1 live) per value" memory bound this benchmark measures
// (via RSS-delta — see docs/runtime-bench.md's caveat on that
// measurement's precision, given #538 heap_size estimators haven't
// landed).
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
