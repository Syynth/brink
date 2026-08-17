// Struct field access benchmark (issue #821 second program batch,
// docs/typed-mode-spec.md §6): 10k read-modify-write field accesses on a
// single, never-shared, never-resized global struct. This program is
// compiled TWICE by the harness (struct_field_access_bench in runtime.rs)
// — once under `types = strict`, once under `types = gradual` — from this
// one source, per the strict/gradual required-equivalence rule
// (crates/brink-compiler/tests/tm4c_structs_codegen.rs's
// `strict_and_gradual_produce_equivalent_output_for_well_formed_program`).
//
// The `VAR p: Point` declared-type annotation is what makes strict-mode
// codegen eligible for static-offset field ops: under `types = strict`,
// `expr::static_offset_for` (brink-ir's lir/lower/expr.rs) sees the
// annotation, resolves `p`'s shape at compile time, and emits
// `RecordGet`/`RecordSet` (flat-offset ops, no shape lookup at runtime).
// `known_shape` (same file) resolves this from the `: Point` annotation
// itself — not from the initializer's shape — so the concrete zero-value
// literal below doesn't affect static-offset eligibility (issue #2138:
// the previous `= 0` placeholder was a bogus int initializer for a
// struct-typed VAR and now fails E063 post-#2085's initializer-type
// check; a real `Point#{...}` literal is both a valid initializer and an
// equally-eligible `known_shape` root).
// Under `types = gradual` the *same* annotated source still only ever
// emits `RecordGetDyn`/`RecordSetDyn` (by-name ops, one shape lookup by
// `NameId` per access) — the annotation-driven static path is strict-only
// by construction (typed-mode-spec §6), never a gradual-mode optimization.
//
// `p` is a single global, assigned once and never shared into a second
// variable, so `record_make_mut`'s COW copy is paid at most once in
// either policy — verified via the `bench-counters` feature
// (`print_bench_counters` in runtime.rs), which instruments
// `record_set`/`record_set_dyn` identically regardless of which op form
// is dispatched. That equivalence is what makes the wall-time delta
// between the two compiles an honest measurement of the static-offset
// dispatch itself (skip the shape/name lookup), not a COW-behavior
// difference — "the difference the typed path buys."
STRUCT Point = #{
    x: float,
    y: float,
}

VAR p: Point = Point#{x: 0.0, y: 0.0}
VAR total = 0

~ {
    p = Point#{x: 0.0, y: 0.0}
    temp i = 0
    while i < 10000 {
        p.x = p.x + 1.0
        p.y = p.y + 1.0
        i = i + 1
    }
    total = p.x + p.y
}
{total}
-> END
