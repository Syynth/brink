// Share-then-mutate benchmark (issue #821 Workstream A/B seed,
// docs/value-model-spec.md §5/§6's "sharing is O(1), mutating a shared
// value is exactly one O(n) copy" claim). Brink-dialect only (`~ { … }`
// blocks and indexed assignment are T1b extensions).
//
// Each loop iteration:
//   1. `b = a`      — read global `a` (an `Arc<Vec<Value>>`) and store it
//                      into global `b`. This is a bare `Arc::clone` — O(1)
//                      — `a` and `b` now alias the same backing allocation.
//   2. `b[0] = i`    — take → `make_mut` → write-back on `b`. `array_make_mut`
//                      finds the Arc shared (refcount > 1, since `a`'s slot
//                      still holds it) and pays exactly one O(n) copy; after
//                      that copy `b` is unique again until the next `b = a`
//                      re-shares it.
//
// This isolates the mutate-while-shared cost *deliberately*, on every
// iteration — the mirror image of `loop-append-10k`, where nothing else
// ever aliases the array so the cost amortizes to ~0 copies. Comparing the
// two programs' `cow_copies` counts (via the `bench-counters` feature)
// tells the two mechanisms apart instead of inferring from wall time alone.
VAR a = 0
VAR b = 0
VAR total = 0
~ {
    a = #[0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    temp i = 0
    while i < 5000 {
        b = a
        b[0] = i
        i = i + 1
    }
    total = b[0]
}
{total}
-> END
