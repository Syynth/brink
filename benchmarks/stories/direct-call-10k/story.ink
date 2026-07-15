// Direct-call baseline (issue #821 second program batch) for
// dynamic-dispatch-10k/story.ink: identical target function, identical
// iteration count and argument shape, but called through ordinary
// `add3(i, i, i)` in-story dispatch (`Opcode::Call`) rather than a fn
// value. The wall-time delta between this program and
// dynamic-dispatch-10k isolates exactly the tax T1c's function-value
// indirection pays over a static call — rehydration check, arity
// re-check, and full-arg-row assembly (vm.rs's `prepare_fn_value_call`)
// that a direct call skips entirely. Brink-dialect only for parity with
// the dynamic-dispatch counterpart (no language-level reason `-> END` +
// `~ { … }` would need it here, but keeping both programs on the same
// dialect keeps the comparison honest).
VAR total = 0
~ {
    temp i = 0
    while i < 10000 {
        total = add3(i, i, i)
        i = i + 1
    }
}
{total}
-> END

=== function add3(a, b, c) ===
~ return a + b + c
