// Field loop-append benchmark (issue #2123, docs/value-model-spec.md §5's
// "one cliff" case one field deeper than #576 closed): 10k sequential
// pushes onto a struct field's array, all within a single `~ { … }` block
// — the single-level struct-field-projection mutator shape
// `lower_field_mutator` compiles (`push(a.items, v)`, `a: Bag`). Mirrors
// `loop-append-10k/story.ink` exactly, except the array being appended to
// lives one field deep instead of being a bare variable. Brink-dialect
// only (no strict-ink/oracle equivalent exists — `push`/`~ { … }` blocks
// and `STRUCT` are T1b/TM-4 extensions).
//
// Before issue #2123's fix: `push(a.items, i)` always read `a.items` via a
// cloning `RecordGet` before mutating it, so the field's `Arc` was doubly
// referenced (once still embedded in the intact root, once in the read's
// own temp) by the time `array_make_mut` ran — O(n) re-COW on every push,
// O(n^2) total, despite #576 already closing this exact cliff for a bare
// variable (`loop-append-10k`). After the fix: the root's own reference to
// the field is dropped (via `RecordSet`) before the mutator runs, so
// `array_make_mut` sees a unique `Arc` whenever nothing else aliases the
// field — O(1) amortized per push, O(n) total, matching
// `loop-append-10k`'s own before/after story.
STRUCT Bag = #{
    items: Array<int>,
}

VAR a = 0
VAR total = 0
~ {
    a = Bag#{items: #[]}
    temp i = 0
    while i < 10000 {
        push(a.items, i)
        i = i + 1
    }
    total = len(a.items)
}
{total}
-> END
