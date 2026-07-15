// ALGORITHMS CORPUS — sorting/searching lane (issue #822)
//
// TYPES POLICY: gradual (default). This port never annotates a single
// declaration — every value here is an int or an array of ints, and
// gradual inference already resolves `x < pivot`, `len(xs)`, etc. cleanly
// end to end. Nothing here earns strict's escape-error discipline; a
// `types = strict` pass would add ceremony with no payoff.
//
// ERGONOMICS-FINDINGS:
// - Idiom chosen: FUNCTIONAL partition-and-recombine, not in-place
//   Lomuto/Hoare swapping. Arrays are copy-on-write values in brink (no
//   pointers/references into a caller's array survive a plain — not `ref`
//   — function argument), so "partition in place, recurse on sub-ranges"
//   needs either `ref` params threaded through every recursive call or a
//   pair of (lo, hi) index bounds. Building three fresh arrays (less/
//   equal/greater) via `push` in a single `for` pass and recursing on
//   those was the path of least resistance — and it reads closer to the
//   textbook algorithm description than an index-juggling in-place port
//   would. Confirmed empirically: the caller's `arr` is untouched after
//   `quicksort(arr)` returns a new array — no defensive copy needed.
// - No array concatenation/slice primitive: "left ++ equal ++ right" is
//   three `for`/`push` loops, not one expression. `docs/t1b-surface-spec.md`
//   §5 already scopes slices/ranges out of stdlib slice 1; this is a live
//   data point for when that gap gets prioritized.
// - `push`/`len` from the T1b-3 stdlib slice cover everything this port
//   needs — no missing collection primitive blocked this port, unlike the
//   concat/slice gap above (which is a *convenience* gap, not a *coverage*
//   gap: everything is still expressible, just via more loops).

VAR arr = #[5, 2, 9, 1, 5, 6, -3, 0]
VAR sorted = 0

~ sorted = quicksort(arr)

Sorted: {sorted}.
Original untouched: {arr}.
-> END

=== function quicksort(xs) ===
~ {
    if len(xs) <= 1 {
        return xs
    }
    temp pivot = xs[0]
    temp less = #[]
    temp equal = #[]
    temp greater = #[]
    for x in xs {
        if x < pivot {
            push(less, x)
        } else if x == pivot {
            push(equal, x)
        } else {
            push(greater, x)
        }
    }
    temp left = quicksort(less)
    temp right = quicksort(greater)
    temp out = #[]
    for x in left {
        push(out, x)
    }
    for x in equal {
        push(out, x)
    }
    for x in right {
        push(out, x)
    }
    return out
}
