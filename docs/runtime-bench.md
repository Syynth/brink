# Runtime benchmark corpus

Workstream A+B seed of the runtime-bench epic (#821), plus a second
program batch (fn-value benches + struct field access). The value model
shipped on argued performance claims — Arc-COW O(1) snapshots, the
mutate-while-shared cliff neutralized by take → `make_mut` → write-back,
the `ptr_eq` equality fast path (`docs/value-model-spec.md` §4/§5/§6) —
without ever measuring them. This corpus makes those claims falsifiable.
The second batch extends the same question to T1c function values
(`docs/t1c-spec.md`) and TM-4c structs (`docs/typed-mode-spec.md` §6),
neither of which the seed touched.

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
cargo bench -p brink-runtime --bench runtime -- fn_value_bench
cargo bench -p brink-runtime --bench runtime -- struct_field_access_bench

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

## The programs

`benchmarks/stories/` (brink-dialect corpus, same directory the existing
`loop-append-10k`/`hanoi-10`/`crucible-8` benchmark fixtures live in):

| Program | File | Mechanism isolated |
|---|---|---|
| loop-append collection churn | `loop-append-10k/story.ink` | 10k sequential `push`es onto a never-shared array — proves (or disproves) O(1) amortized append after #576's take/make_mut/write-back fix (the §5 "one cliff" case) |
| share-then-mutate COW cost | `share-then-mutate-5k/story.ink` | 5k iterations of "share a global into another, then mutate the copy" — the mirror image of loop-append: every iteration deliberately re-shares before mutating, so the COW copy is paid every time, not amortized away |
| `ptr_eq` equality fast path | `ptr_eq_bench` module in `runtime.rs` (no `.ink` file — see below) | Same-`Arc` vs distinct-but-structurally-equal `Value::Array` comparison, directly against `brink_format::Value`'s hand-written `PartialEq` |
| #fn creation density | `fn-creation-density-10k/story.ink` | 10k one-bound-arg closure creations (`#fn(ident, i)`), never called or shared — isolates `Value::closure`'s per-creation `Arc<ClosureValue>` allocation cost alone |
| bind-chain depth, shallow | `fn-bind-chain-shallow/story.ink` | Builds a closure's bound-arg prefix one `bind(f, i)` call at a time to depth 8, 1,000 times |
| bind-chain depth, deep | `fn-bind-chain-deep/story.ink` | Same shape, depth 32 — `bind_fn_value` (vm.rs) copies the *entire existing* bound-arg prefix on every `bind()` call before appending the new entry, so building one hop at a time pays O(depth) copy work per hop, O(depth²) total per chain build; comparing shallow vs deep isolates that scaling |
| dynamic-dispatch call throughput | `dynamic-dispatch-10k/story.ink` | 10k calls through a fn value (`call(f, …)`) against a fixed target — isolates T1c's per-call rehydration check + arity check + arg-row assembly (`prepare_fn_value_call`, vm.rs) |
| direct-call baseline | `direct-call-10k/story.ink` | Same target function, same iteration count, called via ordinary in-story dispatch (`Opcode::Call`) — the baseline `dynamic-dispatch-10k` is measured against |
| struct field access, strict | `struct-field-access-10k/story.ink` compiled with `types = strict` | 10k read-modify-write field accesses on a never-shared global struct, static-offset `RecordGet`/`RecordSet` dispatch |
| struct field access, gradual | same source, compiled with `types = gradual` | Same program, by-name `RecordGetDyn`/`RecordSetDyn` dispatch — the strict/gradual delta isolates the static-offset dispatch cost alone (see the `.ink` file's header for why COW behavior is held constant between the two) |

**Honest mechanism isolation note on `ptr_eq_bench`:** brink's `==`
operator has no `Array`/`Map` arm in `value_ops::binary_op` yet — comparing
two arrays with ink's `==` faults `TypeError`, it doesn't reach `Value`'s
`PartialEq` at all. There is no ink-program equivalent to isolate the
`ptr_eq` shortcut through today, so `ptr_eq_bench` calls
`brink_format::Value`'s `PartialEq` directly — the exact same impl the
runtime's own `map_contains`/list/record comparisons already use
internally. This is flagged rather than hidden, per the epic's "a bench
that can't isolate its mechanism says so" gate.

**Honest mechanism isolation note on the fn-value benches:** the
`bench-counters` feature (below) instruments `Array`/`Map`/`Record` COW
sites only — `bind_fn_value`'s `Closure` allocation (vm.rs) is not wired
to a counter. The fn-value benches therefore report wall time only; there
is no counted proof of Arc-clone/COW-copy behavior for them the way there
is for `loop-append-10k`/`share-then-mutate-5k`/the struct benches. Flagged
rather than silently omitted; wiring a `Closure`-allocation counter is a
reasonable Workstream-B follow-up, not attempted here (see the PR's scope
notes).

## Baseline

Captured 2026-07-14, Apple Silicon dev machine, `cargo bench` release
profile. **Absolute numbers are machine-specific and will drift — do not
treat them as a strict pass/fail gate** (that's Workstream D's job, not
this seed's). The load-bearing numbers are the `bench-counters` event
counts (exact, not timing-noise-sensitive) and the relative magnitude
between `same_arc`/`distinct_but_equal`, and between the strict/gradual
struct-access pair.

### `bench-counters` (exact — the mechanism proof)

```
loop-append-10k:                    cow_copies=     1  arc_clones= 10001
share-then-mutate-5k:                cow_copies=  5000  arc_clones= 10001
struct-field-access-10k (strict):    cow_copies=     0  arc_clones= 40002
struct-field-access-10k (gradual):   cow_copies=     0  arc_clones= 40002
```

`loop-append-10k` pays **one** COW copy across 10,000 pushes — O(1)
amortized, confirming the §5 claim post-#576 (before #576 this was
O(n²), one copy per push). `share-then-mutate-5k` pays **one COW copy per
iteration** (5,000 for 5,000 iterations) by construction — proving the
"mutate while shared" cost is real and bounded to exactly one copy per
share event, not zero and not unbounded.

`struct-field-access-10k` pays **zero** COW copies under both policies —
`p` is a single global, never shared into a second variable, so
`record_make_mut` always finds a unique `Arc` — and the *identical*
`arc_clones` count between strict and gradual (40,002 either way) is the
proof that both compiles do the same amount of "real" work. That
equivalence is what makes the wall-time delta below attributable to
dispatch mechanism (flat-offset index vs by-name shape lookup) and
nothing else.

### Wall time (`--sample-count 5`, informational)

```
ptr_eq_bench/same_arc            ~1.6 ns    (Arc::ptr_eq shortcut)
ptr_eq_bench/distinct_but_equal  ~44 µs     (20k-element structural walk)
  → ~28,000× — the ptr_eq fast path is not noise, it's the dominant term.

cow_sharing_bench/share_then_mutate_5k   ~4–12 ms  (5,000 iterations, 1 COW copy each)
loop_append_bench/push_10k               ~6–18 ms  (10,000 iterations, 1 COW copy total)

fn_value_bench/creation_density_10k      ~4.5–4.7 ms  (10,000 one-bound-arg closure creations)
fn_value_bench/bind_chain_shallow        ~4.9–6.3 ms  (1,000 chain builds, depth 8 — 28 copy-units/build)
fn_value_bench/bind_chain_deep           ~18–31 ms    (1,000 chain builds, depth 32 — 496 copy-units/build)
fn_value_bench/direct_call_10k           ~7.3–10.7 ms (10,000 direct in-story calls)
fn_value_bench/dynamic_dispatch_10k      ~8.0–9.1 ms  (10,000 calls through a fn value)

struct_field_access_bench/strict_static_offset     ~13.0–13.3 ms  (10,000 field r/w, RecordGet/RecordSet)
struct_field_access_bench/gradual_dynamic_fallback ~12.6–14.1 ms  (10,000 field r/w, RecordGetDyn/RecordSetDyn)
```

The two loop benchmarks' wall times are close despite a 5000:1 difference
in COW-copy count because loop-append does 2× the iterations and both
programs pay the same per-iteration `GetGlobal`/opcode-dispatch overhead
that dominates at this problem size — this is exactly why the epic wants
counters alongside wall time: reading wall time alone here would
under-state how much cheaper the never-shared path is per unit of "real"
work.

`bind_chain_deep` (depth 32, 496 copy-units/build) runs ~4× slower than
`bind_chain_shallow` (depth 8, 28 copy-units/build) for a nominal
17.7× copy-unit ratio — at these depths the fixed per-`bind()`-call
dispatch overhead (target resolution, arity check, `Vec` allocation)
dominates over the O(depth) element-copy itself, so the wall-time ratio
tracks depth more than depth². Reported as measured, not rounded up to
match the O(depth²) prediction — the mechanism (copy grows with existing
bound-arg count) is real and directly traceable in `bind_fn_value`'s
source, but at n≤32 the constant-factor call overhead is still the
larger term. A future deeper-chain variant (bound only by the u8
`param_count` ceiling) could isolate the asymptotic term more cleanly;
not attempted here (scope).

`dynamic_dispatch_10k` runs slightly slower than `direct_call_10k` (~8.6ms
vs ~8.0ms mean, this run) — the fn-value indirection tax
(`prepare_fn_value_call`'s rehydration + arity check + arg-row `Vec`
assembly) is present but modest relative to the shared per-call opcode
overhead both programs pay.

`strict_static_offset` runs marginally faster than `gradual_dynamic_fallback`
(~13.07ms vs ~13.59ms mean, this run, ~4%) — the static-offset path (a
`Vec` index) skips the by-name path's shape-table lookup
(`Program::struct_shape` + linear field-name scan) on every access, but
at 10k accesses on a 2-field struct the difference is a small fraction of
total wall time, most of which is the shared opcode-dispatch/interpreter
loop overhead both paths pay identically.

## Regenerating this baseline

```sh
cargo bench -p brink-runtime --features bench-counters --bench runtime -- loop_append_bench cow_sharing_bench struct_field_access_bench --sample-count 10
cargo bench -p brink-runtime --bench runtime -- ptr_eq_bench --sample-count 20
cargo bench -p brink-runtime --bench runtime -- fn_value_bench struct_field_access_bench --sample-count 10
```

Update the tables above by hand — there is no automated baseline-diff
tripwire yet (Workstream D, not this seed).
