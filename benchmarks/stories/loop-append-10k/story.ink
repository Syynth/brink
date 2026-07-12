// Loop-append benchmark (issue #576, docs/value-model-spec.md §5's "one
// cliff" case): 10k sequential pushes onto a freshly-created array, all
// within a single `~ { … }` block — the flat (n == 1 / bare-variable)
// indexed-write/mutator shape `TakeGlobal`/`TakeTemp` closes the O(n^2)
// COW cliff for. Brink-dialect only (no strict-ink/oracle equivalent
// exists — `push`/`~ { … }` blocks are T1b extensions).
//
// Before #576: every `push(arr, i)` reads `arr` via `GetGlobal` (an Arc
// clone), so `array_make_mut` always sees itself as shared and COW-copies
// the whole backing Vec on every iteration — O(n) per push, O(n^2) total.
// After #576: `push` takes `arr` out of its slot before mutating, so
// `array_make_mut` sees a unique Arc whenever nothing else aliases it —
// O(1) amortized per push, O(n) total.
VAR arr = 0
VAR total = 0
~ {
    arr = #[]
    temp i = 0
    while i < 10000 {
        push(arr, i)
        i = i + 1
    }
    total = len(arr)
}
{total}
-> END
