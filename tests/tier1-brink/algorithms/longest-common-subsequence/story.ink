// ALGORITHMS CORPUS — DP lane (issue #822)
// Longest common subsequence: top-down memoized recursion over
// (index into A, index into B), memo table as a MAP OF MAPS — the
// alternative to `knapsack-01`'s composite-string-key workaround.
//
// TYPES POLICY: gradual (default). Indices/lengths are `int`, sequence
// elements and the LCS result are single-character `string`s (see the
// no-string-indexing finding below), the memo is `Map<int, Map<int,
// int>>`; gradual inference handles all of it without ambiguity.
//
// ERGONOMICS-FINDINGS:
// - No `char_at`/string-indexing primitive exists in stdlib (confirmed:
//   no `Value::String` indexing path in `brink-runtime`, unlike arrays/
//   maps). A character-level algorithm over raw strings is therefore not
//   directly expressible — both sequences are represented as arrays of
//   single-character strings (`#["A", "G", "C", ...]`) instead, and the
//   result is rejoined with a `for`/`+` loop at the end. This is a real
//   coverage gap, not just a convenience one (see `knapsack-01`'s
//   composite-key finding for the convenience-vs-coverage distinction) —
//   worth flagging for `docs/t1b-surface-spec.md`'s stdlib scoping.
// - SHARP EDGE, confirmed empirically: `container[newKey] = value` only
//   ever *updates* an existing key — it does NOT insert a new one, for
//   maps exactly as for arrays (an out-of-bounds array index needs
//   `push`, not assignment). `memo[i] = #{}` on a key `i` the map doesn't
//   have yet fails at runtime with `MapKeyNotFound` — this is not a
//   compile-time check, it's a runtime fault, discovered only by actually
//   running the program (a minimal 6-line repro nailed it down before
//   this file's real recursion made the failure mode confusing). The fix
//   is `insert(memo, i, #{})` to create the missing outer entry, THEN
//   `insert(memo[i], j, result)` (mutator-on-nested-lvalue, same pattern
//   as `tests/tier1-brink/stdlib-mutator-nested-lvalue`) to populate it —
//   `insert` is the only way to add a key that wasn't there before,
//   direct index assignment is exclusively for keys that already exist.
//   This is the single most surprising thing this DP lane turned up and
//   deserves to be stated plainly in whatever map-ergonomics docs this
//   epic's findings feed into.
// - Map-of-maps as the 2D memo, compared against `knapsack-01`'s
//   composite-string-key approach: this one keeps the compiler-checked
//   `int` key type on both dimensions (no stringify-and-hope), but pays
//   for it with lazy-initialization ceremony — every write path needs an
//   `if not contains(memo, i) { insert(memo, i, #{}) }` guard before the
//   inner `insert`. The composite-key version has exactly one
//   `contains`/`insert` pair per memo access; this version has two
//   (outer existence, then inner read/write). Net assessment: map-of-maps
//   is the more *type-honest* choice, composite-string-key is the more
//   *concise* one — genuinely a wash, not a clear winner either way.
// - Reading through a confirmed-present chain (`memo[i][j]` once both
//   keys are known to exist) works with no ceremony, and mutator-through-
//   nested-lvalue (`insert(memo[i], j, result)`) works exactly like the
//   array case — it's specifically *inserting a fresh key via plain `=`
//   assignment* that's unsupported, not nested indexing in general.
// - `reconstruct()` re-invokes the already-fully-populated memoized `lcs`
//   function while walking the answer back out greedily; because the walk
//   only ever visits `(i, j)` pairs that were already computed on the way
//   to the top-level answer, every call it makes is a guaranteed memo hit
//   — no extra recursion happens during reconstruction.

VAR seqA = #["A", "G", "C", "A", "T"]
VAR seqB = #["G", "A", "C"]

#@local
VAR memo = #{}

VAR lcsLen = 0
VAR lcsText = ""

~ {
    lcsLen = lcs(0, 0)
    lcsText = reconstruct()
}

LCS length: {lcsLen}. LCS: {lcsText}.
-> END

=== function lcs(i, j) ===
~ {
    if i >= len(seqA) or j >= len(seqB) {
        return 0
    }
    if not contains(memo, i) {
        insert(memo, i, #{})
    }
    if contains(memo[i], j) {
        return memo[i][j]
    }
    temp result = 0
    if seqA[i] == seqB[j] {
        result = 1 + lcs(i + 1, j + 1)
    } else {
        temp skip_a = lcs(i + 1, j)
        temp skip_b = lcs(i, j + 1)
        if skip_a > skip_b {
            result = skip_a
        } else {
            result = skip_b
        }
    }
    insert(memo[i], j, result)
    return result
}

=== function reconstruct() ===
~ {
    temp i = 0
    temp j = 0
    temp out = #[]
    while i < len(seqA) and j < len(seqB) {
        if seqA[i] == seqB[j] {
            push(out, seqA[i])
            i = i + 1
            j = j + 1
        } else {
            temp right = lcs(i + 1, j)
            temp down = lcs(i, j + 1)
            if right >= down {
                i = i + 1
            } else {
                j = j + 1
            }
        }
    }
    temp s = ""
    for c in out {
        s = s + c
    }
    return s
}
