// #fn creation density benchmark (issue #821 second program batch): 10k
// repeated one-bound-arg closure creations (`#fn(ident, i)` -> a fresh
// `Value::Closure(Arc<ClosureValue>)` allocation each time — see
// `brink_format::Value`'s `Closure` variant doc) in a tight loop. Nothing
// else in the loop body touches a collection, record, or second closure —
// the created closure is never called, never shared, never stored past its
// own iteration — so wall-time attributes almost entirely to
// `Value::closure`'s per-creation allocation cost, isolating "creation
// density" from bind-chain cost (see fn-bind-chain-shallow/deep) and call
// cost (see dynamic-dispatch-10k). Brink-dialect only (`~ { … }` blocks,
// `while`, and `#fn`/T1c function values are brink extensions).
VAR total = 0
~ {
    temp i = 0
    while i < 10000 {
        temp f = #fn(ident, i)
        total = i
        i = i + 1
    }
}
{total}
-> END

=== function ident(a) ===
~ return a
