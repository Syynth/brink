# Runtime benchmark corpus

Workstream A+B seed of the runtime-bench epic (#821). The value model
shipped on argued performance claims — Arc-COW O(1) snapshots, the
mutate-while-shared cliff neutralized by take → `make_mut` → write-back,
the `ptr_eq` equality fast path (`docs/value-model-spec.md` §4/§5/§6) —
without ever measuring them. This corpus makes those claims falsifiable.

The harness is [`divan`](https://docs.rs/divan), the same crate the
existing `compile`/`link`/`run` benches in
`crates/brink-runtime/benches/runtime.rs` already use (`#498` baseline,
`editor_session_bench`) — this seed extends that file rather than
introducing a second harness, per the workspace convention.

## How to run

```sh
# Wall-time + step-throughput for every scenario/program in the file.
cargo bench -p brink-runtime --bench runtime

# Filter to one group (divan filters by substring on the benchmark path).
cargo bench -p brink-runtime --bench runtime -- loop_append_bench
cargo bench -p brink-runtime --bench runtime -- cow_sharing_bench
cargo bench -p brink-runtime --bench runtime -- ptr_eq_bench

# Fewer samples for a quick sanity pass (default is much higher and can
# take minutes across the full scenario matrix).
cargo bench -p brink-runtime --bench runtime -- --sample-count 10

# Arc-clone / COW-copy debug counters (bench-only, feature-gated — see
# "Bench-only counters" below). Prints a counted comparison to stderr
# before the timed runs, in addition to running them.
cargo bench -p brink-runtime --features bench-counters --bench runtime
```

`cargo bench` builds in the `bench` (release-like) profile; expect the
first run to take a while to compile the whole workspace dependency graph.

## Bench-only counters (`bench-counters` feature)

`crates/brink-runtime/src/bench_counters.rs` is a `#[cfg(feature =
"bench-counters")]`-gated module: two `AtomicU64` counters, `cow_copies`
and `arc_clones`, incremented at the exact mechanism sites (`Value::
array_make_mut`/`map_make_mut`/`record_make_mut`'s take → make_mut →
write-back call sites in `collection_ops.rs`/`record_ops.rs`, and
`Opcode::GetGlobal`'s collection-value read in `vm.rs`).

This answers the question wall-time alone can't: not just "is program A
faster than program B" but "did the COW-copy actually happen, and how many
times." It is off by default — not part of the `default` feature set — so
an ordinary `cargo build`/`cargo test`/`cargo bench` (no extra flags) never
compiles the module in at all: no atomics, no call-site branches, a
compile-time cut rather than a runtime toggle. Verified by the gate:
`cargo build -p brink-runtime` and `cargo clippy -p brink-runtime
--all-targets -- -D warnings` both pass with the module physically absent
(no `bench_counters` in the crate), and separately with `--features
bench-counters` added.

## The first three programs

`benchmarks/stories/` (brink-dialect corpus, same directory the existing
`loop-append-10k`/`hanoi-10`/`crucible-8` benchmark fixtures live in):

| Program | File | Mechanism isolated |
|---|---|---|
| loop-append collection churn | `loop-append-10k/story.ink` | 10k sequential `push`es onto a never-shared array — proves (or disproves) O(1) amortized append after #576's take/make_mut/write-back fix (the §5 "one cliff" case) |
| share-then-mutate COW cost | `share-then-mutate-5k/story.ink` | 5k iterations of "share a global into another, then mutate the copy" — the mirror image of loop-append: every iteration deliberately re-shares before mutating, so the COW copy is paid every time, not amortized away |
| `ptr_eq` equality fast path | `ptr_eq_bench` module in `runtime.rs` (no `.ink` file — see below) | Same-`Arc` vs distinct-but-structurally-equal `Value::Array` comparison, directly against `brink_format::Value`'s hand-written `PartialEq` |

**Honest mechanism isolation note on `ptr_eq_bench`:** brink's `==`
operator has no `Array`/`Map` arm in `value_ops::binary_op` yet — comparing
two arrays with ink's `==` faults `TypeError`, it doesn't reach `Value`'s
`PartialEq` at all. There is no ink-program equivalent to isolate the
`ptr_eq` shortcut through today, so `ptr_eq_bench` calls
`brink_format::Value`'s `PartialEq` directly — the exact same impl the
runtime's own `map_contains`/list/record comparisons already use
internally. This is flagged rather than hidden, per the epic's "a bench
that can't isolate its mechanism says so" gate.

## Baseline

Captured 2026-07-14, Apple Silicon dev machine, `cargo bench` release
profile. **Absolute numbers are machine-specific and will drift — do not
treat them as a strict pass/fail gate** (that's Workstream D's job, not
this seed's). The load-bearing numbers are the `bench-counters` event
counts (exact, not timing-noise-sensitive) and the relative magnitude
between `same_arc`/`distinct_but_equal`.

### `bench-counters` (exact — the mechanism proof)

```
loop-append-10k:       cow_copies=     1   arc_clones= 10001
share-then-mutate-5k:  cow_copies=  5000   arc_clones= 10001
```

`loop-append-10k` pays **one** COW copy across 10,000 pushes — O(1)
amortized, confirming the §5 claim post-#576 (before #576 this was
O(n²), one copy per push). `share-then-mutate-5k` pays **one COW copy per
iteration** (5,000 for 5,000 iterations) by construction — proving the
"mutate while shared" cost is real and bounded to exactly one copy per
share event, not zero and not unbounded.

### Wall time (`--sample-count 5`–`20`, informational)

```
ptr_eq_bench/same_arc            ~1.6 ns    (Arc::ptr_eq shortcut)
ptr_eq_bench/distinct_but_equal  ~44 µs     (20k-element structural walk)
  → ~28,000× — the ptr_eq fast path is not noise, it's the dominant term.

cow_sharing_bench/share_then_mutate_5k   ~4–12 ms  (5,000 iterations, 1 COW copy each)
loop_append_bench/push_10k               ~6–18 ms  (10,000 iterations, 1 COW copy total)
```

The two loop benchmarks' wall times are close despite a 5000:1 difference
in COW-copy count because loop-append does 2× the iterations and both
programs pay the same per-iteration `GetGlobal`/opcode-dispatch overhead
that dominates at this problem size — this is exactly why the epic wants
counters alongside wall time: reading wall time alone here would
under-state how much cheaper the never-shared path is per unit of "real"
work.

## Regenerating this baseline

```sh
cargo bench -p brink-runtime --features bench-counters --bench runtime -- loop_append_bench cow_sharing_bench --sample-count 10
cargo bench -p brink-runtime --bench runtime -- ptr_eq_bench --sample-count 20
```

Update the tables above by hand — there is no automated baseline-diff
tripwire yet (Workstream D, not this seed).
