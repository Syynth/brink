// ALGORITHMS CORPUS — sorting/searching lane (issue #822)
//
// TYPES POLICY: gradual (default). Same reasoning as quicksort's sibling
// port in this lane: every value is an int, an array of ints, or a `bool`
// used only as a loop condition — gradual inference already resolves the
// whole file with no ambiguity, so `types = strict` would add annotation
// ceremony with nothing to check.
//
// ERGONOMICS-FINDINGS:
// - No slice syntax (`xs[a:b]`) exists (t1b-surface-spec.md §5 scopes it
//   out of stdlib slice 1) — splitting the input into `left`/`right` halves
//   is two manual `while` loops copying by index with `push`, not the
//   one-liner a slice would give. This is the same gap quicksort's finding
//   flags for concatenation; splitting and joining are two ends of the
//   same missing "array range" primitive.
// - `merge` reads cleanest as a *third* helper function taking two already-
//   sorted arrays rather than folding it into `mergesort` itself — brink's
//   plain function-call story (no closures/lambdas needed here) makes this
//   split free; nothing about recursion or scoping penalizes factoring the
//   merge step out.
// - Recursion depth for this array size (9 elements, ~4 levels) is a
//   non-issue; confirmed the compiled program runs well under the VM's
//   default step limit. Left as a signpost for later, much larger corpus
//   entries (the epic's A*/GOAP lanes) where recursion depth vs. step
//   budget will be a real design question, not a formality.

VAR arr = #[5, 2, 9, 1, 5, 6, -3, 0, 7]
VAR sorted = 0

~ sorted = mergesort(arr)

Sorted: {sorted}.
Original untouched: {arr}.
-> END

=== function mergesort(xs) ===
~ {
    temp n = len(xs)
    if n <= 1 {
        return xs
    }
    temp mid = n / 2
    temp left = #[]
    temp right = #[]
    temp i = 0
    while i < mid {
        push(left, xs[i])
        i = i + 1
    }
    while i < n {
        push(right, xs[i])
        i = i + 1
    }
    temp sorted_left = mergesort(left)
    temp sorted_right = mergesort(right)
    return merge(sorted_left, sorted_right)
}

=== function merge(a, b) ===
~ {
    temp out = #[]
    temp i = 0
    temp j = 0
    temp na = len(a)
    temp nb = len(b)
    while i < na {
        if j < nb {
            if a[i] <= b[j] {
                push(out, a[i])
                i = i + 1
            } else {
                push(out, b[j])
                j = j + 1
            }
        } else {
            push(out, a[i])
            i = i + 1
        }
    }
    while j < nb {
        push(out, b[j])
        j = j + 1
    }
    return out
}
