// The fn-value verb layer's pure trio (docs/stdlib-spec.md §4, issue
// #1679): `map`, `filter`, `fold`.
//
// Callbacks are pure·silent-required (RULED 2026-07-18) — which is what
// makes "one logical pass, order unobservable" true and dissolves the
// eager/lazy question. Every callback here is a `#fn(target)` literal over
// a named function: lambdas parse and lower to HIR (#1685) but still stop
// at the LIR codegen fence, so `#fn(…)` is the only fn-value spelling that
// reaches these ops today.
//
// TYPES POLICY: strict (the brink-dialect default). Callback params and
// returns are annotated for the same reason `sort-verbs`' comparator is —
// a function invoked only through a fn value has no direct call site for
// mono-HM to infer from.
~ temp xs = #[1, 2, 3, 4]
map: {map(xs, #fn(double))}; source kept: {xs}.
filter: {filter(xs, #fn(is_even))}.
fold: {fold(xs, 0, #fn(add))}.

// Chained — `map`'s result is an ordinary array, so the verbs compose
// without any pipeline machinery.
chained: {fold(map(filter(xs, #fn(is_even)), #fn(double)), 100, #fn(add))}.

// Element type may change under `map`.
~ temp names = #["ada", "grace"]
lengths: {map(names, #fn(size))}.

// Empty array: `map`/`filter` yield empty, `fold` yields `init` untouched
// (no absence case, so no Option — contrast `min`/`max`).
~ temp empty = #[]
empty map: {map(empty, #fn(double))}; empty fold: {fold(empty, 7, #fn(add))}.

// A closure over a bound argument (`bind`) is a function value too, so it
// is a legal callback.
~ temp add_ten = bind(#fn(add), 10)
bound: {map(xs, add_ten)}.
-> END

=== function double(n: int): int ===
~ return n * 2

=== function is_even(n: int): bool ===
~ return n % 2 == 0

=== function add(acc: int, n: int): int ===
~ return acc + n

=== function size(s: string): int ===
~ return len(s)
