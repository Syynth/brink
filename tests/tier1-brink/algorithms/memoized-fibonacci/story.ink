// ALGORITHMS CORPUS — DP lane (issue #822)
// Memoized (top-down) Fibonacci: the canonical "map as memo table" case.
//
// TYPES POLICY: gradual (default). Every value here is an `int` or a
// `Map<int, int>`; gradual inference resolves `memo[n]`, `n - 1`, etc.
// cleanly end to end. Nothing here earns strict's escape-error discipline.
//
// ERGONOMICS-FINDINGS:
// - `#@local` on the memo map: this is exactly the "per-call[-tree] memo"
//   shape the epic anticipated. `#@local` is documented
//   (`docs/directive-annotations-spec.md`) as a *flow-private storage
//   class* — it isolates the VAR per concurrent flow, not per individual
//   top-level call — so in this single-flow harness it's observably a
//   plain global. The honest finding: `#@local` earns its keep only once
//   a story runs `fib` from two independent flows that must NOT see each
//   other's memo entries (a real scenario — e.g. two NPCs independently
//   computing something memoized) — for a single-flow program like this
//   one it's a no-op with good intentions, not a load-bearing choice.
//   Left in anyway because it's the honest annotation for "this cache
//   should not leak across flows" even when this harness can't observe
//   the difference.
// - `memo[n]` read-indexing and `insert(memo, n, result)` write are both
//   one-liners — a single-key int->int memo table has zero friction.
//   `contains(memo, n)` as the "have I computed this" guard reads exactly
//   like the textbook memoized-fibonacci pseudocode. This is the friction
//   FLOOR for map-as-memo-table; see `knapsack-01` and
//   `longest-common-subsequence` for what happens once the memo key needs
//   more than one dimension.
// - No `ref` parameters and no fn-value indirection were needed — `fib`
//   reads/writes the top-level `memo`/`calls` VARs directly by name, the
//   same way every non-recursive example in this corpus does. Recursion
//   itself (a function calling itself by name) has no special ceremony.

#@local
VAR memo = #{}
VAR calls = 0

VAR fib10 = 0
VAR fib20 = 0
VAR fib30 = 0

~ {
    fib10 = fib(10)
    fib20 = fib(20)
    fib30 = fib(30)
}

fib(10) = {fib10}, fib(20) = {fib20}, fib(30) = {fib30}. Total calls (incl. memo hits): {calls}. Memo entries: {len(memo)}.
-> END

=== function fib(n) ===
~ {
    calls = calls + 1
    if contains(memo, n) {
        return memo[n]
    }
    if n <= 1 {
        insert(memo, n, n)
        return n
    }
    temp result = fib(n - 1) + fib(n - 2)
    insert(memo, n, result)
    return result
}
