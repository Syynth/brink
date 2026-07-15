// ALGORITHMS CORPUS — sorting/searching lane (issue #822)
//
// TYPES POLICY: gradual (default). Purely int scalars and an int array;
// nothing here is ambiguous under inference, so `types = strict` has no
// escape errors to catch and adds nothing.
//
// ERGONOMICS-FINDINGS:
// - REAL BUG FOUND IN THIS PORT'S FIRST DRAFT, kept here as the finding:
//   `while j >= 0 && arr[j] > key { ... }` panics with an out-of-bounds
//   array read at `j == -1`. Traced to codegen, not a doc-only footnote:
//   `InfixOp::And`/`Or` always lower to the eager `Opcode::And`/`Or`
//   (`brink-codegen-inkb/src/expr.rs`) — both operands are evaluated
//   unconditionally, so `arr[j]` still runs even when `j >= 0` is already
//   false. This matches vanilla ink's own documented `&&`/`and` semantics
//   (both sides always evaluate — oracle-anchored, not a brink
//   regression); note `format-spec.md`'s "short-circuit is handled by
//   compiler via JumpIfFalse" comment describes the *if/while condition*
//   getting one `JumpIfFalse` after being evaluated as a single value, not
//   lazy evaluation of an `&&`/`or` sub-expression inside that condition —
//   worth a doc clarification, since as written it reads like the latter.
//   Either way this is exactly the trap an author porting from a
//   short-circuiting language hits immediately — insertion sort's classic
//   `while (j >= 0 && arr[j] > key)` guard is *the* idiomatic shape that
//   bites. Fixed here with an explicit nested-`if` state machine
//   (`shifting` flag) instead of a compound boolean guard — more verbose,
//   but correct and arguably clearer about the two independent stopping
//   conditions. Strong candidate for a first-class short-circuit boolean
//   operator in a future round (#521-adjacent) since every
//   loop-with-a-lookback-guard algorithm in this corpus hits the same
//   shape.
// - In-place mutation (`arr[j + 1] = arr[j]`) on a `VAR`-declared array is
//   the natural idiom here, unlike quicksort/mergesort's functional
//   recombination — insertion sort's whole definition is "shift in
//   place", and brink's RMW indexed-assignment lowering (already proven
//   by the `nested-index-assignment` T1b corpus case) covers it directly.
//   No `ref` parameter needed since the mutation happens in the same
//   scope as the declaration; this is a single `~ { }` block, not a
//   helper function.

VAR arr = #[5, 2, 9, 1, 5, 6, -3, 0]

~ {
    temp n = len(arr)
    temp i = 1
    while i < n {
        temp key = arr[i]
        temp j = i - 1
        temp shifting = true
        while shifting {
            if j >= 0 {
                if arr[j] > key {
                    arr[j + 1] = arr[j]
                    j = j - 1
                } else {
                    shifting = false
                }
            } else {
                shifting = false
            }
        }
        arr[j + 1] = key
        i = i + 1
    }
}

Sorted: {arr}.
-> END
