// ALGORITHMS CORPUS — DP lane (issue #822)
// 0/1 knapsack: top-down memoized recursion over (item index, remaining
// capacity) — the first two-dimensional memo table in this lane.
//
// TYPES POLICY: gradual (default). Weights/values/capacity are `int`,
// the memo table is `Map<string, int>`; nothing here needs strict's
// escape-error discipline to type-check cleanly.
//
// ERGONOMICS-FINDINGS:
// - THE finding of this file: brink maps have no tuple/composite keys —
//   only the scalar key types `contains`/`insert`/indexing accept (int,
//   string, bool, float per `map-key-domain-contains-edges`'s coverage).
//   A 2D memo table (item index, remaining capacity) therefore needs its
//   key flattened into one scalar, and the only ergonomic option is a
//   string built with `string(i) + ":" + string(w)`. This works — string
//   concatenation and `string()` conversions are unconditionally
//   available — but it trades a compiler-checked tuple key for a
//   hand-built, un-typechecked string convention: a typo in the separator
//   or an accidental collision (e.g. an item count and a capacity that
//   could stringify to overlapping keys with a different separator choice)
//   is a silent wrong-answer bug, not a compile error. `longest-common-
//   subsequence` tries the alternative (a map of maps) and its header
//   compares the two.
// - Aside from the composite-key workaround, the recursive shape is
//   ordinary: `contains(memo, key)` guards a cache hit exactly like
//   `memoized-fibonacci`'s single-key version, and the take/skip branches
//   are a direct transcription of the textbook recurrence — the *value*
//   side of memoization has no friction here, only the *key* side does.
// - Recursion depth is bounded by item count (4 items here, depth <= 4);
//   not a real concern until this lane ports a knapsack with a much
//   larger item list.

VAR weights = #[2, 3, 4, 5]
VAR values = #[3, 4, 5, 6]
VAR capacity = 5

VAR best = 0

#@local
VAR memo = #{}

~ best = knapsack(0, capacity)

Best value for capacity {capacity}: {best}. Memo entries: {len(memo)}.
-> END

=== function knapsack(i, w) ===
~ {
    if i >= len(weights) {
        return 0
    }
    temp key = string(i) + ":" + string(w)
    if contains(memo, key) {
        return memo[key]
    }
    temp result = 0
    if weights[i] > w {
        result = knapsack(i + 1, w)
    } else {
        temp skip = knapsack(i + 1, w)
        temp take = values[i] + knapsack(i + 1, w - weights[i])
        if take > skip {
            result = take
        } else {
            result = skip
        }
    }
    insert(memo, key, result)
    return result
}
