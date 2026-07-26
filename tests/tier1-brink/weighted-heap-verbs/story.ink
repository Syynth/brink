// NS-A7 collections+ (docs/stdlib-spec.md §8, issue #1113).
//
// Weighted[T]: evidence-by-construction tables (the weighted(w, v, …)
// brink-dialect spelling of the chartered Weighted { w: v } literal),
// F17 multiset equality (order-insensitive, multiplicity-sensitive),
// construction-literal display, and roll — one seeded draw through the
// pinned chain + DotNetRng, so the roll values below are stable
// oracle-free goldens exactly like rand-verbs' (any change to the draw
// chain breaks this case loudly; that is the point).
//
// The humble heap: min-heap verbs over an ordinary [T] by the §4b
// doctrine order — heap_push (statement-only, RMW write-back),
// heap_pop (mutator + Option expression), heap_peek (pure Option read);
// empty pops are absence (none), never faults. The drain line's final
// `heap_pop` runs the heap dry — its `none` is the display-boundary
// forgiveness (§1.6b, Track B4): it renders as nothing, not the word
// `none`.
//
// TYPES POLICY: strict (the brink-dialect default) — the whole surface
// types cleanly: roll(Weighted[string]) is a string, heap_pop([int]) is
// Option[int].
~ seed(6)
~ temp loot = weighted(3, "sword", 1, "shield", 3, "potion")
table: {loot}
multiset: {loot == weighted(3, "potion", 1, "shield", 3, "sword")}
rolls: {roll(loot)}, {roll(loot)}, {roll(loot)}, {roll(loot)}, {roll(loot)}
~ seed(6)
replay: {roll(loot)}, {roll(loot)}, {roll(loot)}, {roll(loot)}, {roll(loot)}
~ temp open = #[5, 9, 8]
~ heap_push(open, 3)
~ heap_push(open, 7)
peek: {heap_peek(open)}
~ temp first = heap_pop(open)
~ temp second = heap_pop(open)
popped: {first} then {second}
drain: {heap_pop(open)} {heap_pop(open)} {heap_pop(open)} {heap_pop(open)}
-> END
