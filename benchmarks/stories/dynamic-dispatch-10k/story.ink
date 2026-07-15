// Dynamic-dispatch call throughput benchmark (issue #821 second program
// batch): 10k calls through a fn value (`call(f, …)`, the explicit T1c
// call form) against a fixed zero-bound `#fn` target. Isolates the
// dispatch overhead a call-through-value pays that a direct in-story call
// never does: `fn_value_target_idx`'s target resolution,
// `prepare_fn_value_call`'s per-call rehydration check (bound-entry
// name/mode against the *current* signature) and arity re-check, and the
// full-arg-row assembly (vm.rs's `enter_fn_value`). Compare against
// direct-call-10k/story.ink — identical target function, identical
// iteration count and argument shape, called with plain `add3(i, i, i)`
// syntax instead — for the honest baseline this bench isolates against.
// Brink-dialect only (`~ { … }` blocks, `while`, and `#fn`/`call` are T1c
// extensions).
VAR total = 0
~ {
    temp f = #fn(add3)
    temp i = 0
    while i < 10000 {
        total = call(f, i, i, i)
        i = i + 1
    }
}
{total}
-> END

=== function add3(a, b, c) ===
~ return a + b + c
