// NS-A4 ordering verbs (docs/stdlib-spec.md §4b, issue #1110).
//
// The four-verb family (F0): imperative in-place `sort`/`sort_by`,
// functional past-participle `sorted`/`sorted_by`. Doctrine order:
// int/float (numeric promotion), bool, string (USV-lexicographic),
// arrays lexicographic element-wise; stable throughout. All data here is
// NaN-free, so the default (dev) mode and prod agree exactly — the
// modes-agree leg of the §4b gate; the dev-fault leg is pinned by the
// runtime unit tests and the compiler-level ExecMode test.
//
// TYPES POLICY: strict (the brink-dialect default). The comparator's
// params/return are annotated — `b - a` alone can't pin them under
// mono-HM (subtraction spans int/float), and a comparator invoked only
// through a fn value has no direct call site to infer from.
~ temp xs = #[3, 1, 2, 1]
~ temp ys = sorted(xs)
sorted copy: {ys}; source kept: {xs}.
~ sort(xs)
sorted in place: {xs}.
~ temp mixed = #[2, 1.5, 1]
~ sort(mixed)
mixed numerics: {mixed}.
~ temp words = #["pear", "apple", "fig"]
words: {sorted(words)}.
~ temp nested = #[#[2], #[1, 5], #[1]]
lex: {sorted(nested)}.
~ temp zs = #[3, 1, 2]
~ sort_by(zs, #fn(desc))
sort_by desc: {zs}.
~ temp ws = sorted_by(#[1, 4, 2], #fn(desc))
sorted_by desc: {ws}.
-> END

=== function desc(a: int, b: int): int ===
~ return b - a
