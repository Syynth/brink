// ALGORITHMS CORPUS — sorting/searching lane (issue #822)
//
// TYPES POLICY: gradual (default). Every value is an int, an int array, or
// a `bool` loop flag — gradual inference resolves the whole file; there is
// no `Unknown`/`Conflicted` escape for `types = strict` to catch.
//
// ERGONOMICS-FINDINGS:
// - Written iteratively (a `searching` bool flag driving a `while`), not
//   recursively, deliberately sidestepping the `&&`-in-a-loop-guard trap
//   documented in this lane's `insertion-sort` port (brink's `&&`/`and`
//   never short-circuits — matches vanilla ink). A recursive version's
//   natural base case, `if lo > hi { return -1 }`, is itself safe (no
//   compound guard needed) — early `return` from inside nested `if`/`else`
//   blocks inside a function body was confirmed to work correctly during
//   this port's development, so recursion was a real, working option here.
//   Iterative was chosen anyway because it needs one function instead of
//   two call frames per probe, and keeps this file's step count trivially
//   small and easy to reason about for a corpus program meant to double as
//   documentation.
// - No early-return-from-`while` construct (no `break`-with-value): the
//   `searching` flag pattern (set `false`, let the loop guard exit next
//   iteration) is the idiom for "stop this loop and also stop searching"
//   — `break` alone would exit the loop but not communicate *which* branch
//   caused the exit, so the flag does double duty as both loop control and
//   book-keeping for whether a match was ever found.

VAR arr = #[-3, 0, 1, 2, 5, 5, 6, 7, 9]
VAR found_5 = 0
VAR found_4 = 0
VAR found_9 = 0
VAR found_neg3 = 0

~ {
    found_5 = binary_search(arr, 5)
    found_4 = binary_search(arr, 4)
    found_9 = binary_search(arr, 9)
    found_neg3 = binary_search(arr, -3)
}

Index of 5: {found_5}.
Index of 4: {found_4}.
Index of 9: {found_9}.
Index of -3: {found_neg3}.
-> END

=== function binary_search(xs, target) ===
~ {
    temp lo = 0
    temp hi = len(xs) - 1
    temp result = -1
    temp searching = true
    while searching {
        if lo > hi {
            searching = false
        } else {
            temp mid = (lo + hi) / 2
            if xs[mid] == target {
                result = mid
                searching = false
            } else {
                if xs[mid] < target {
                    lo = mid + 1
                } else {
                    hi = mid - 1
                }
            }
        }
    }
    return result
}
