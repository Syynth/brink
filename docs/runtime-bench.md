# Runtime benchmark corpus

Workstream A+B seed of the runtime-bench epic (#821), a second program
batch (fn-value benches + struct field access), and Workstream C (memory
footprint). The value model shipped on argued performance claims —
Arc-COW O(1) snapshots, the mutate-while-shared cliff neutralized by
take → `make_mut` → write-back, the `ptr_eq` equality fast path
(`docs/value-model-spec.md` §4/§5/§6), and bounded snapshot-retention
memory (§8) — without ever measuring them. This corpus makes those
claims falsifiable. The second batch extends the same question to T1c
function values (`docs/t1c-spec.md`) and TM-4c structs
(`docs/typed-mode-spec.md` §6); Workstream C (this update) extends it to
`SaveState` wire size, the §8 bounded-retention memory claim, and
Arc-clone/COW counter behavior across a save/load cycle.

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
cargo bench -p brink-runtime --bench runtime -- save_state_bench
cargo bench -p brink-runtime --bench runtime -- snapshot_retention_bench

# Fewer samples for a quick sanity pass (default is much higher and can
# take minutes across the full scenario matrix).
cargo bench -p brink-runtime --bench runtime -- --sample-count 10

# Arc-clone / COW-copy debug counters (bench-only, feature-gated — see
# "Bench-only counters" below). Prints a counted comparison to stderr
# before the timed runs, in addition to running them.
cargo bench -p brink-runtime --features bench-counters --bench runtime
```

Every run (with or without `--features bench-counters`) also prints, to
stderr before the timed benchmarks: `SaveState` wire sizes for the
small/medium/large shapes, and RSS-delta samples across the
snapshot-retention (G, M) matrix (Workstream C — see below). With
`--features bench-counters` added, it additionally prints the
snapshot-retention `cow_copies`/`arc_clones` matrix and the save/load
counter deltas.

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
| `SaveState` wire size, small | `save-state-small/story.ink` | A handful of scalar globals, no collections — the fixed per-save envelope floor |
| `SaveState` wire size, medium | `save-state-medium/story.ink` | Scalars + one 500-int array |
| `SaveState` wire size, large | `save-state-large/story.ink` | Scalars + one 10,000-int array |
| snapshot-retention, G=10/M=10 | `snapshot-retention-g10-m10/story.ink` | Retains 10 generations of a 20-element array into `history` (one Arc-share push each), 10 mutations per generation — the §8 bounded-retention claim's smallest matrix point |
| snapshot-retention, G=10/M=100 | `snapshot-retention-g10-m100/story.ink` | Same retention depth, 10x the mutations per generation — isolates "does `cow_copies` track M" (it shouldn't) |
| snapshot-retention, G=100/M=10 | `snapshot-retention-g100-m10/story.ink` | 10x the retention depth, same total mutation count as G=10/M=100 — isolates "does `cow_copies` track G" (it should) |
| snapshot-retention, G=100/M=100 | `snapshot-retention-g100-m100/story.ink` | 10x both dimensions of the G=10/M=10 point — the far corner of the matrix |

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

## Workstream C: memory footprint

Three measurements, per the epic's Workstream C ask:

**1. `SaveState` wire size (bytes serialized).** `SaveState`
(`brink-format`) is what `Story::save_state()` produces and what
`brink-web`'s wasm boundary serializes with plain `serde_json`
(`crates/brink-web/src/session.rs`'s `save`/`load` methods —
`serde_json::to_string(&session.save_state())` /
`serde_json::from_str::<SaveState>`). `print_save_state_wire_sizes`
(`runtime.rs`) captures that exact encoding for the three
small/medium/large shapes and prints the byte count for each — see the
Baseline table below. `save_state_bench`'s divan benches additionally
time the serialization itself (informational; the byte count is the
load-bearing number here, not the wall time).

**2. Snapshot-retention cost curve (the §8 bounded-retention claim).**
`docs/value-model-spec.md` §8: "a retained snapshot bounds memory at
(retained generations + 1 live) per value — the mutator pays one COW
copy at first divergence, history never accumulates." The four
`snapshot-retention-*` programs (see the table above) retain G
generations of a live array's state into a `history` array (one
Arc-share `push` per generation) while mutating the live array M times
between each retention point — see
`snapshot-retention-g10-m10/story.ink`'s header for the full argument.

`print_snapshot_retention_counters` (`--features bench-counters`) reports
`cow_copies` for all four (G, M) points. **Measured: `cow_copies = G +
1`, not `G`.** The `+1` is a one-time cost paid by `history`'s own first
`push` — `history = #[]` starts from the runtime's shared
empty-array-literal pool, and the first mutation of any value sharing
that pool pays exactly one COW to diverge from it, identical to
`loop-append-10k`'s own `cow_copies=1` (that program's `arr = #[]` pays
the same one-time cost). It is orthogonal to the G-generations retention
loop under test, constant across all four points, and doesn't change the
comparison's conclusion: g10-m10 and g10-m100 (same G, 10x different M)
report the *same* `cow_copies`; g100-m10 and g10-m100 (same total
mutation count, 10x different G) report a ~10x *different* `cow_copies`.
That two-sided comparison — invariant to M, proportional to G — is the
direct proof of the bounded-retention claim; the shared `+1` offset is
flagged rather than silently rounded away.

Memory (rather than COW-copy count) is measured via RSS-delta —
`print_snapshot_retention_rss`, always runs, no feature flag needed.
**Caveat, stated per the epic's honesty gate**: `#538` (heap_size
estimators) is still open as of this benchmark, so there is no precise
per-value byte accounting available; this shells out to `ps -o rss=` on
the current process (current, not peak, RSS in KB — uniform across
Linux/macOS, unlike `getrusage`'s `ru_maxrss`) before and after each
scenario, in the same process. This is a coarse, single-process
proxy — it reflects the whole process's memory (allocator overhead,
fragmentation, and whether freed pages were returned to the OS at all,
not just the `history` array under test) — appropriate for "does more
retention roughly cost more memory, not something worse," not a
byte-exact tripwire. See the Baseline section for captured numbers and
how noisy they are in practice.

**3. Arc-clone/COW counter deltas over a save/load cycle.**
`print_save_load_counter_deltas` (`--features bench-counters`) resets the
counters, calls `Story::save_state()`, then `Story::load_state()` on the
result, and reports what each half saw. This required extending
`bench-counters`' wiring: `save_state`/`load_state`
(`crates/brink-runtime/src/save.rs`) now call the same `note_value_share`
helper `Opcode::GetGlobal` uses (made `pub(crate)` in `vm.rs` for this),
on the common (no `#@was` alias-rebinding) path of each global
read/write — before this PR, a save/load cycle was invisible to
`bench-counters` entirely (it never calls `array_make_mut`/
`map_make_mut`/`record_make_mut`, and the counters weren't wired at
`ContextAccess::global`/`set_global` at all). Measured: `cow_copies == 0`
on both halves (save/load only ever reads or replaces a whole global
slot — it never mutates a collection in place, so the COW-copy sites are
never reached by this path, by construction, not because of missing
coverage); `arc_clones` tracks the number of collection-typed globals
touched (1, for `save-state-medium`'s single `inventory` array).

## Baseline

Captured 2026-07-14 (A/B seed + second batch), extended 2026-07-15
(Workstream C: `SaveState` wire size, snapshot-retention, save/load
counters), Apple Silicon dev machine, `cargo bench` release profile.
**Absolute numbers are machine-specific and will drift — do not treat
them as a strict pass/fail gate** (that's Workstream D's job, not this
seed's). The load-bearing numbers are the `bench-counters` event counts
(exact, not timing-noise-sensitive) and the relative magnitude between
`same_arc`/`distinct_but_equal`, and between the strict/gradual
struct-access pair.

### `bench-counters` (exact — the mechanism proof)

```
loop-append-10k:                    cow_copies=     1  arc_clones= 10001
share-then-mutate-5k:                cow_copies=  5000  arc_clones= 10001
struct-field-access-10k (strict):    cow_copies=     0  arc_clones= 40002
struct-field-access-10k (gradual):   cow_copies=     0  arc_clones= 40002

snapshot-retention g10-m10:          cow_copies=    11  arc_clones=   121
snapshot-retention g10-m100:         cow_copies=    11  arc_clones=  1021
snapshot-retention g100-m10:         cow_copies=   101  arc_clones=  1201
snapshot-retention g100-m100:        cow_copies=   101  arc_clones= 10201

save-state-medium save_state():      cow_copies=     0  arc_clones=     1
save-state-medium load_state():      cow_copies=     0  arc_clones=     1
```

The snapshot-retention row is the §8 bounded-retention proof: g10-m10 and
g10-m100 (same G=10, 10x different M) both report `cow_copies=11`;
g100-m10 and g10-m100 (same 1,000 total mutations, 10x different G)
report 101 vs 11 — a ~10x difference tracking G, not the (identical)
mutation count. Every point's count is `G + 1` — see "Workstream C:
memory footprint" above for why the `+1` is there and why it doesn't
change the conclusion.

The save/load row is the honest-zero result described above: a save/load
cycle never touches `array_make_mut`/`map_make_mut`/`record_make_mut`
(it only reads or replaces whole global slots), so `cow_copies=0` on both
halves is the correct measurement, not missing coverage.

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

save_state_bench/serialize_small    ~0.82–0.84 µs (SaveState, small shape, serde_json::to_vec)
save_state_bench/serialize_medium   ~7.2–7.9 µs   (SaveState, medium shape)
save_state_bench/serialize_large    ~135–140 µs   (SaveState, large shape)

snapshot_retention_bench/g10_m10     ~92–94 µs   (10 generations x 10 mutations, 100 total)
snapshot_retention_bench/g10_m100    ~834–842 µs (10 generations x 100 mutations, 1,000 total)
snapshot_retention_bench/g100_m10    ~912–943 µs (100 generations x 10 mutations, 1,000 total)
snapshot_retention_bench/g100_m100   ~8.5–8.6 ms (100 generations x 100 mutations, 10,000 total)
```

`save_state_bench`'s three timings scale roughly with array size (500
vs 10,000 ints) as expected for `serde_json`'s per-element encoding —
informational only, the wire-size *bytes* below are the load-bearing
number for this ask.

`snapshot_retention_bench/g10_m100` and `g100_m10` do the same total
mutation count (1,000) but g100-m10's ~10x-deeper retention makes it
slightly slower (~923 vs ~838 µs mean) — consistent with `history`
holding 10x as many retained snapshots (more `push`es, each an
Arc-clone) even though the `cow_copies` proof above shows the COW-copy
cost itself doesn't scale with G in a way wall time would make obvious
at these sizes (both pay a comparable amount of *dispatch* overhead per
mutation regardless of which side of the (G, M) split it's on).

### `SaveState` wire sizes (exact — bytes serialized, `serde_json`)

```
small    328 bytes   (4 scalar globals, no collections)
medium  6169 bytes   (2 scalars + a 500-int array)
large 129157 bytes   (2 scalars + a 10,000-int array)
```

Growth is close to linear in element count once past the small shape's
fixed envelope (~330 bytes of `SaveState`'s own JSON structure —
`version`/`turn_index`/`rng_seed`/`previous_random`/empty
`visits`/`turns` — dominates when there's nothing bigger to save):
medium's 500-int array adds ~5,800 bytes over small (~11.6 bytes/int,
JSON's `{"Int":N}` tagged-enum encoding plus separators); large's
10,000-int array is ~123,000 bytes over medium's baseline, ~12.3
bytes/int — consistent, not superlinear, at this scale.

### Snapshot-retention RSS deltas (coarse — see the caveat above)

```
g10-m10    delta ≈    0–16 KB
g10-m100   delta ≈    0 KB
g100-m10   delta ≈    0–48 KB
g100-m100  delta ≈    0 KB
```

Deltas this small relative to a ~35 MB process baseline are consistent
with "this corpus's array sizes (20 elements) don't move the needle on
process RSS at all" — the programs are deliberately sized to keep the
COW-copy/wall-time measurements fast (VM step-limit-friendly), not to
stress memory. A future variant with much larger retained arrays (or
many more generations) would be needed to see a non-noise RSS signal;
not attempted here (scope) — see the caveat in "Workstream C: memory
footprint" above for why this number is inherently this noisy regardless
of array size.

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

# Workstream C (memory footprint): wire sizes + RSS deltas print
# unconditionally (no feature flag); the snapshot-retention and
# save/load bench-counters need --features bench-counters.
cargo bench -p brink-runtime --bench runtime -- save_state_bench snapshot_retention_bench --sample-count 10
cargo bench -p brink-runtime --features bench-counters --bench runtime -- save_state_bench snapshot_retention_bench --sample-count 10
```

Update the tables above by hand — there is no automated baseline-diff
tripwire yet (Workstream D, not this seed).
