// NS-A6 (issue #1112, docs/stdlib-spec.md §7): the std::rand draw verbs.
// Deterministic under the pinned draw algorithm + DotNetRng (the corpus
// harness's generator): seed(n) fixes the whole transcript, so the
// expected.txt values below are stable oracle-free goldens — any change
// to the pinned chain (seed derivation, 24-bit float shaping, Fisher-
// Yates order) breaks this case loudly. That is the point.
~ seed(7)
~ temp u = float()
in range: {u >= 0.0}, below one: {u < 1.0}
sure thing: {chance(1)}, no way: {chance(0)}, coin: {chance(0.5)}
~ temp a = #[10, 20, 30, 40, 50]
picked {string(pick(a))} then {string(pick(a))}
empty pick: {string(pick(#[]))}
~ shuffle(a)
shuffled: {a[0]} {a[1]} {a[2]} {a[3]} {a[4]}
~ temp b = shuffled(a)
source keeps len {len(a)}, twin has len {len(b)}
~ seed(7)
replay matches: {u == float()}
one then two: {string(pick(#[1]))} {string(pick(#[2]))}
-> END
