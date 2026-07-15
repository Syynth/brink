// Bind-chain depth benchmark (issue #821 second program batch, shallow):
// builds a function value's bound-arg prefix one argument at a time via
// repeated single-arg `bind(f, i)` calls to depth 8, then invokes the
// fully-bound closure — 1000 times. `bind_fn_value` (vm.rs) copies the
// *entire existing* bound-arg prefix on every call
// (`env.extend_from_slice(existing)`) before appending the one new entry,
// so building a chain one hop at a time pays O(depth) copy work per hop —
// O(depth^2) total per full chain build. Comparing this program's wall
// time against fn-bind-chain-deep/story.ink (depth 32, same 1000 outer
// iterations) isolates that scaling directly: shallow pays 28 copy-units
// per chain build, deep pays 496 — the ratio is the depth-squared
// mechanism, not incidental noise.
VAR total = 0
~ {
    temp outer = 0
    while outer < 1000 {
        temp f = #fn(sum8)
        temp i = 0
        while i < 8 {
            f = bind(f, i)
            i = i + 1
        }
        total = f()
        outer = outer + 1
    }
}
{total}
-> END

=== function sum8(a0, a1, a2, a3, a4, a5, a6, a7) ===
~ return a0 + a1 + a2 + a3 + a4 + a5 + a6 + a7
