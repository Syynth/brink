// ALGORITHMS CORPUS — DP lane (issue #822)
// Edit distance (Levenshtein): bottom-up 2D table over an array-of-arrays,
// the deliberate contrast case against this lane's other three top-down
// map-memo ports.
//
// TYPES POLICY: gradual (default). Every value is an `int`, a single-
// character `string`, or an `Array<Array<int>>` table; gradual inference
// resolves the whole file with no ambiguity.
//
// ERGONOMICS-FINDINGS:
// - Deliberately bottom-up + array-of-arrays instead of top-down + map
//   memo (this lane's other three ports): once a DP table's index domain
//   is a dense rectangle known up front (`0..=len(a)` by `0..=len(b)`,
//   with the boundary row/column filled unconditionally), a plain nested
//   array — built exactly like `bfs-grid-path`/`dijkstra-grid`'s
//   `make_grid` helper — is strictly less ceremony than a map: no
//   `contains` guard before every read, no lazy inner-map initialization
//   (`longest-common-subsequence`'s finding), no composite-key stringify
//   (`knapsack-01`'s finding). The map-as-memo-table idiom earns its keep
//   specifically when the reachable index set is sparse/recursive and
//   not worth precomputing in full — for a table this small (7x8) that
//   condition never holds, so "just use a grid" wins outright. Net
//   finding for the lane: map-memo tables are for sparse top-down
//   recursion; dense bottom-up tables want plain nested arrays, and
//   choosing between them is a real design decision, not a style
//   preference.
// - Same no-string-indexing gap as `longest-common-subsequence`: both
//   words are pre-split into arrays of single-character strings
//   (`#["k", "i", "t", "t", "e", "n"]`) rather than indexed directly as
//   strings.
// - The classic "kitten" -> "sitting" textbook example (distance 3) was
//   used specifically because it's independently checkable against any
//   reference source — this port's correctness is checkable by a reader
//   without re-deriving the DP recurrence by hand.

VAR wordA = #["k", "i", "t", "t", "e", "n"]
VAR wordB = #["s", "i", "t", "t", "i", "n", "g"]

VAR dist = 0

~ dist = edit_distance(wordA, wordB)

Edit distance from "kitten" to "sitting": {dist}.
-> END

=== function edit_distance(a, b) ===
~ {
    temp n = len(a)
    temp m = len(b)
    temp table = #[]
    temp i = 0
    while i <= n {
        temp row = #[]
        temp j = 0
        while j <= m {
            push(row, 0)
            j = j + 1
        }
        push(table, row)
        i = i + 1
    }
    i = 0
    while i <= n {
        table[i][0] = i
        i = i + 1
    }
    temp j = 0
    while j <= m {
        table[0][j] = j
        j = j + 1
    }
    i = 1
    while i <= n {
        j = 1
        while j <= m {
            if a[i - 1] == b[j - 1] {
                table[i][j] = table[i - 1][j - 1]
            } else {
                temp sub = table[i - 1][j - 1]
                temp del = table[i - 1][j]
                temp ins = table[i][j - 1]
                temp best = sub
                if del < best {
                    best = del
                }
                if ins < best {
                    best = ins
                }
                table[i][j] = best + 1
            }
            j = j + 1
        }
        i = i + 1
    }
    return table[n][m]
}
