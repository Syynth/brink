use std::fmt;
use std::sync::Arc;

use brink_compiler::{AnalysisOptions, Dialect, TypePolicy};
use brink_format::StoryData;
use brink_runtime::{DotNetRng, Line, Program, Stats, Story};

// ── Scenarios ────────────────────────────────────────────────────────────────

struct Scenario {
    name: &'static str,
    /// `.ink` entry point, relative to this crate's manifest dir.
    ink: &'static str,
    inputs: Vec<usize>,
}

impl fmt::Display for Scenario {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

const MINIMAL_INK: &str = "../../tests/tier1/basics/I001-minimal-story/story.ink";

const HANOI_3_INK: &str = "../../tests/tier3/lists/tower-of-hanoi/story.ink";
const HANOI_3_INPUT: &str = include_str!("../../../tests/tier3/lists/tower-of-hanoi/input.txt");

const HANOI_10_INK: &str = "../../benchmarks/stories/hanoi-10/story.ink";
const HANOI_10_INPUT: &str = include_str!("../../../benchmarks/stories/hanoi-10/input.txt");

const CRUCIBLE_8_INK: &str = "../../benchmarks/stories/crucible-8/story.ink";
const CRUCIBLE_8_INPUT: &str = include_str!("../../../benchmarks/stories/crucible-8/input.txt");

/// Loop-append (issue #576, `docs/value-model-spec.md` §5's "one cliff")
/// benchmark: 10k sequential `push`es onto a freshly-created array in one
/// `~ { … }` block — brink-dialect only (no strict-ink/oracle equivalent;
/// see the `.ink` file's header comment for the before/after cliff this
/// isolates). Not part of `scenarios()`/`Scenario` (those all compile under
/// the default strict-ink dialect via `compile_story`) — `loop_append_bench`
/// below is a standalone `#[divan::bench]` using `compile_story_brink`.
const LOOP_APPEND_10K_INK: &str = "../../benchmarks/stories/loop-append-10k/story.ink";

/// Share-then-mutate (issue #821 Workstream A/B seed) benchmark: 5k
/// iterations of "share a global into another, then mutate the copy" —
/// the mirror image of [`LOOP_APPEND_10K_INK`]'s never-shared append.
/// Brink-dialect only, standalone like the loop-append bench (see the
/// `.ink` file's header for the mechanism this isolates).
const SHARE_THEN_MUTATE_5K_INK: &str = "../../benchmarks/stories/share-then-mutate-5k/story.ink";

/// #fn creation density (issue #821 second program batch): 10k repeated
/// one-bound-arg closure creations in a tight loop, never called or
/// shared. Brink-dialect only, standalone like the loop-append bench.
const FN_CREATION_DENSITY_10K_INK: &str =
    "../../benchmarks/stories/fn-creation-density-10k/story.ink";

/// Bind-chain depth, shallow variant (depth 8) — see the `.ink` file's
/// header for the O(depth²) mechanism this isolates against
/// [`FN_BIND_CHAIN_DEEP_INK`].
const FN_BIND_CHAIN_SHALLOW_INK: &str = "../../benchmarks/stories/fn-bind-chain-shallow/story.ink";

/// Bind-chain depth, deep variant (depth 32) — see [`FN_BIND_CHAIN_SHALLOW_INK`].
const FN_BIND_CHAIN_DEEP_INK: &str = "../../benchmarks/stories/fn-bind-chain-deep/story.ink";

/// Dynamic-dispatch call throughput: 10k calls through a fn value. Compare
/// against [`DIRECT_CALL_10K_INK`] for the honest baseline.
const DYNAMIC_DISPATCH_10K_INK: &str = "../../benchmarks/stories/dynamic-dispatch-10k/story.ink";

/// Direct-call baseline for [`DYNAMIC_DISPATCH_10K_INK`]: identical target
/// function and iteration count, called through ordinary in-story dispatch.
const DIRECT_CALL_10K_INK: &str = "../../benchmarks/stories/direct-call-10k/story.ink";

/// Struct field access (issue #821 second program batch,
/// docs/typed-mode-spec.md §6): 10k read-modify-write field accesses on a
/// never-shared global struct. Compiled twice by
/// [`struct_field_access_bench`] — once per [`TypePolicy`] — from this one
/// source, to isolate the static-offset (`RecordGet`/`RecordSet`) vs
/// by-name (`RecordGetDyn`/`RecordSetDyn`) dispatch cost the typed path
/// buys. See the `.ink` file's header for the full mechanism argument.
const STRUCT_FIELD_ACCESS_10K_INK: &str =
    "../../benchmarks/stories/struct-field-access-10k/story.ink";

/// `SaveState` wire-size benchmarks (issue #821 Workstream C): three
/// story-state shapes — small (scalars only), medium (a 500-int array),
/// large (a 10,000-int array) — see each `.ink` file's header.
const SAVE_STATE_SMALL_INK: &str = "../../benchmarks/stories/save-state-small/story.ink";
const SAVE_STATE_MEDIUM_INK: &str = "../../benchmarks/stories/save-state-medium/story.ink";
const SAVE_STATE_LARGE_INK: &str = "../../benchmarks/stories/save-state-large/story.ink";

/// Snapshot-retention cost curve (issue #821 Workstream C,
/// `docs/value-model-spec.md` §8's bounded-retention claim): four points
/// in a (retained generations `G`, mutations-per-generation `M`) matrix.
/// See `snapshot-retention-g10-m10/story.ink`'s header for the full
/// mechanism argument and the sibling files for how each point isolates
/// the claim.
const SNAPSHOT_RETENTION_G10_M10_INK: &str =
    "../../benchmarks/stories/snapshot-retention-g10-m10/story.ink";
const SNAPSHOT_RETENTION_G10_M100_INK: &str =
    "../../benchmarks/stories/snapshot-retention-g10-m100/story.ink";
const SNAPSHOT_RETENTION_G100_M10_INK: &str =
    "../../benchmarks/stories/snapshot-retention-g100-m10/story.ink";
const SNAPSHOT_RETENTION_G100_M100_INK: &str =
    "../../benchmarks/stories/snapshot-retention-g100-m100/story.ink";

#[expect(clippy::unwrap_used)]
fn parse_inputs(s: &str) -> Vec<usize> {
    s.lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.trim().parse().unwrap())
        .collect()
}

fn scenarios() -> &'static [Scenario] {
    static SCENARIOS: std::sync::OnceLock<Vec<Scenario>> = std::sync::OnceLock::new();
    SCENARIOS
        .get_or_init(|| {
            vec![
                Scenario {
                    name: "minimal",
                    ink: MINIMAL_INK,
                    inputs: vec![],
                },
                Scenario {
                    name: "hanoi-3",
                    ink: HANOI_3_INK,
                    inputs: parse_inputs(HANOI_3_INPUT),
                },
                Scenario {
                    name: "hanoi-10",
                    ink: HANOI_10_INK,
                    inputs: parse_inputs(HANOI_10_INPUT),
                },
                Scenario {
                    name: "crucible-8",
                    ink: CRUCIBLE_8_INK,
                    inputs: parse_inputs(CRUCIBLE_8_INPUT),
                },
            ]
        })
        .as_slice()
}

// ── Helpers ──────────────────────────────────────────────────────────────────

#[expect(clippy::unwrap_used)]
fn compile_story(ink_rel: &str) -> StoryData {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(ink_rel);
    brink_compiler::compile_path(&path).unwrap().data
}

/// Like [`compile_story`] but under the brink dialect (`push`/`~ { … }`
/// blocks are T1b extensions, invisible to the default strict-ink
/// compile) — used only by [`LOOP_APPEND_10K_INK`].
#[expect(clippy::unwrap_used)]
fn compile_story_brink(ink_rel: &str) -> StoryData {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(ink_rel);
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    brink_compiler::compile_path_with_options(&path, options)
        .unwrap()
        .data
}

/// Like [`compile_story_brink`] but with an explicit [`TypePolicy`] — used
/// only by [`struct_field_access_bench`], which compiles the *same* source
/// under both `TypePolicy::Strict` and `TypePolicy::Gradual` to isolate the
/// static-offset vs by-name field-op dispatch cost (see
/// [`STRUCT_FIELD_ACCESS_10K_INK`]'s doc for the full argument).
#[expect(clippy::unwrap_used)]
fn compile_story_brink_typed(ink_rel: &str, types: TypePolicy) -> StoryData {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(ink_rel);
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        types,
        ..AnalysisOptions::default()
    };
    brink_compiler::compile_path_with_options(&path, options)
        .unwrap()
        .data
}

#[expect(clippy::unwrap_used)]
fn run_to_completion(
    program: &Arc<Program>,
    line_tables: Vec<Vec<brink_format::LineEntry>>,
    inputs: &[usize],
) -> Stats {
    let mut story = Story::<DotNetRng>::new(Arc::clone(program), line_tables);
    let mut input_idx = 0;

    loop {
        let mut done = false;
        for line in story.continue_maximally().unwrap() {
            match line {
                Line::Text { .. } => {}
                Line::Done { .. } | Line::End { .. } => {
                    done = true;
                }
                Line::Choices { choices, .. } => {
                    if input_idx >= inputs.len() {
                        done = true;
                        break;
                    }
                    let idx = inputs[input_idx];
                    input_idx += 1;
                    assert!(idx < choices.len());
                    story.choose(idx).unwrap();
                }
            }
        }
        if done {
            break;
        }
    }

    story.stats().clone()
}

/// Like [`run_to_completion`] but returns the driven [`Story`] itself
/// instead of just its stats (issue #821 Workstream C) — callers that
/// need the story's retained state to still be resident afterward (e.g.
/// an RSS sample, or a `save_state()` call) need the story kept alive,
/// not dropped at the end of this function.
#[expect(clippy::unwrap_used)]
fn run_to_completion_keep_story(
    program: &Arc<Program>,
    line_tables: Vec<Vec<brink_format::LineEntry>>,
    inputs: &[usize],
) -> Story<DotNetRng> {
    let mut story = Story::<DotNetRng>::new(Arc::clone(program), line_tables);
    let mut input_idx = 0;

    loop {
        let mut done = false;
        for line in story.continue_maximally().unwrap() {
            match line {
                Line::Text { .. } => {}
                Line::Done { .. } | Line::End { .. } => {
                    done = true;
                }
                Line::Choices { choices, .. } => {
                    if input_idx >= inputs.len() {
                        done = true;
                        break;
                    }
                    let idx = inputs[input_idx];
                    input_idx += 1;
                    assert!(idx < choices.len());
                    story.choose(idx).unwrap();
                }
            }
        }
        if done {
            break;
        }
    }

    story
}

// ── Benchmark groups ─────────────────────────────────────────────────────────

mod compiler_bench {
    use super::{Scenario, compile_story, scenarios};

    #[divan::bench(args = scenarios())]
    fn compile(bencher: divan::Bencher, scenario: &Scenario) {
        bencher.bench_local(|| compile_story(scenario.ink));
    }
}

mod linker_bench {
    use super::{Scenario, compile_story, scenarios};

    #[divan::bench(args = scenarios())]
    #[expect(clippy::unwrap_used)]
    fn link(bencher: divan::Bencher, scenario: &Scenario) {
        let data = compile_story(scenario.ink);
        bencher.bench_local(|| brink_runtime::link(&data).unwrap());
    }
}

mod runtime_step {
    use super::{Scenario, compile_story, run_to_completion, scenarios};

    #[divan::bench(args = scenarios())]
    fn run(bencher: divan::Bencher, scenario: &Scenario) {
        let data = compile_story(scenario.ink);
        #[expect(clippy::unwrap_used)]
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        bencher.bench_local(|| run_to_completion(&program, line_tables.clone(), &scenario.inputs));
    }
}

/// Loop-append (issue #576) benchmark: isolates the RMW-mutate cost alone
/// (link once, run repeatedly), matching `runtime_step`'s granularity —
/// the compile step is brink-dialect-specific setup, not part of what this
/// benchmark measures. Before #576, this scenario is O(n^2) in the push
/// count (10k re-COWs of an up-to-10k-element array); after #576, O(n)
/// amortized. See the PR description for measured before/after numbers
/// (`docs/value-model-spec.md` §5 predicts, this benchmark verifies).
mod loop_append_bench {
    use super::{LOOP_APPEND_10K_INK, compile_story_brink, run_to_completion};

    #[divan::bench]
    fn push_10k(bencher: divan::Bencher) {
        let data = compile_story_brink(LOOP_APPEND_10K_INK);
        #[expect(clippy::unwrap_used)]
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        bencher.bench_local(|| run_to_completion(&program, line_tables.clone(), &[]));
    }
}

/// Share-then-mutate (issue #821) benchmark: isolates the deliberate
/// mutate-while-shared cost, matching `loop_append_bench`'s granularity
/// (link once, run repeatedly). Every iteration re-shares the array before
/// mutating it, so — unlike `loop_append_bench`, which amortizes to ~0
/// copies — this pays one COW copy per iteration by construction. See the
/// `.ink` file's header comment and `docs/runtime-bench.md` for the counted
/// comparison between the two (`bench-counters` feature).
mod cow_sharing_bench {
    use super::{SHARE_THEN_MUTATE_5K_INK, compile_story_brink, run_to_completion};

    #[divan::bench]
    fn share_then_mutate_5k(bencher: divan::Bencher) {
        let data = compile_story_brink(SHARE_THEN_MUTATE_5K_INK);
        #[expect(clippy::unwrap_used)]
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        bencher.bench_local(|| run_to_completion(&program, line_tables.clone(), &[]));
    }
}

/// Fn-value benchmarks (issue #821 second program batch): creation
/// density, bind-chain depth, and dynamic-dispatch call throughput vs a
/// direct-call baseline. Each program links once, runs repeatedly, same
/// granularity as [`loop_append_bench`]/[`cow_sharing_bench`].
mod fn_value_bench {
    use super::{
        DIRECT_CALL_10K_INK, DYNAMIC_DISPATCH_10K_INK, FN_BIND_CHAIN_DEEP_INK,
        FN_BIND_CHAIN_SHALLOW_INK, FN_CREATION_DENSITY_10K_INK, compile_story_brink,
        run_to_completion,
    };

    /// #fn creation density: 10k one-bound-arg closure creations, never
    /// called or shared — isolates `Value::closure`'s per-creation
    /// allocation cost alone.
    #[divan::bench]
    fn creation_density_10k(bencher: divan::Bencher) {
        let data = compile_story_brink(FN_CREATION_DENSITY_10K_INK);
        #[expect(clippy::unwrap_used)]
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        bencher.bench_local(|| run_to_completion(&program, line_tables.clone(), &[]));
    }

    /// Bind-chain depth, shallow (depth 8) — compare against
    /// [`bind_chain_deep`] for the O(depth²) scaling `bind_fn_value`'s
    /// existing-prefix copy produces.
    #[divan::bench]
    fn bind_chain_shallow(bencher: divan::Bencher) {
        let data = compile_story_brink(FN_BIND_CHAIN_SHALLOW_INK);
        #[expect(clippy::unwrap_used)]
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        bencher.bench_local(|| run_to_completion(&program, line_tables.clone(), &[]));
    }

    /// Bind-chain depth, deep (depth 32) — see [`bind_chain_shallow`].
    #[divan::bench]
    fn bind_chain_deep(bencher: divan::Bencher) {
        let data = compile_story_brink(FN_BIND_CHAIN_DEEP_INK);
        #[expect(clippy::unwrap_used)]
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        bencher.bench_local(|| run_to_completion(&program, line_tables.clone(), &[]));
    }

    /// Dynamic-dispatch call throughput: 10k calls through a fn value —
    /// compare against [`direct_call_10k`] for the honest baseline.
    #[divan::bench]
    fn dynamic_dispatch_10k(bencher: divan::Bencher) {
        let data = compile_story_brink(DYNAMIC_DISPATCH_10K_INK);
        #[expect(clippy::unwrap_used)]
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        bencher.bench_local(|| run_to_completion(&program, line_tables.clone(), &[]));
    }

    /// Direct-call baseline for [`dynamic_dispatch_10k`]: identical target
    /// function and iteration count, called through ordinary in-story
    /// dispatch (`Opcode::Call`) instead of a fn value.
    #[divan::bench]
    fn direct_call_10k(bencher: divan::Bencher) {
        let data = compile_story_brink(DIRECT_CALL_10K_INK);
        #[expect(clippy::unwrap_used)]
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        bencher.bench_local(|| run_to_completion(&program, line_tables.clone(), &[]));
    }
}

/// Struct field access (issue #821 second program batch,
/// docs/typed-mode-spec.md §6): the *same* source
/// ([`STRUCT_FIELD_ACCESS_10K_INK`]) compiled under both `TypePolicy`
/// values, isolating the static-offset (`RecordGet`/`RecordSet`, strict)
/// vs by-name (`RecordGetDyn`/`RecordSetDyn`, gradual) field-op dispatch
/// cost — "the difference the typed path buys." See the `.ink` file's
/// header for the full mechanism argument, including why this isolates
/// dispatch cost rather than COW behavior (both policies pay identical
/// `bench-counters` COW-copy counts — see [`print_bench_counters`]).
mod struct_field_access_bench {
    use super::{
        STRUCT_FIELD_ACCESS_10K_INK, TypePolicy, compile_story_brink_typed, run_to_completion,
    };

    #[divan::bench]
    fn strict_static_offset(bencher: divan::Bencher) {
        let data = compile_story_brink_typed(STRUCT_FIELD_ACCESS_10K_INK, TypePolicy::Strict);
        #[expect(clippy::unwrap_used)]
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        bencher.bench_local(|| run_to_completion(&program, line_tables.clone(), &[]));
    }

    #[divan::bench]
    fn gradual_dynamic_fallback(bencher: divan::Bencher) {
        let data = compile_story_brink_typed(STRUCT_FIELD_ACCESS_10K_INK, TypePolicy::Gradual);
        #[expect(clippy::unwrap_used)]
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        bencher.bench_local(|| run_to_completion(&program, line_tables.clone(), &[]));
    }
}

/// `SaveState` wire-size benchmarks (issue #821 Workstream C): each
/// program runs once to build up its story-state shape, then
/// `serde_json::to_vec` — the exact encoding `brink-web`'s
/// `session.rs`/`story_runner.rs` use at the wasm boundary
/// (`serde_json::to_string(&session.save_state())`) — is timed
/// repeatedly against the resulting fixed `SaveState`. Byte counts are
/// reported separately by [`print_save_state_wire_sizes`] (below,
/// `main`), since that's the load-bearing number for this ask ("bytes
/// serialized"), not the serialization wall time (informational only).
mod save_state_bench {
    use super::{
        SAVE_STATE_LARGE_INK, SAVE_STATE_MEDIUM_INK, SAVE_STATE_SMALL_INK, compile_story_brink,
        run_to_completion_keep_story,
    };

    #[expect(clippy::unwrap_used)]
    fn save_state_for(ink: &str) -> brink_format::SaveState {
        let data = compile_story_brink(ink);
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        let story = run_to_completion_keep_story(&program, line_tables, &[]);
        story.save_state()
    }

    #[divan::bench]
    fn serialize_small(bencher: divan::Bencher) {
        let save = save_state_for(SAVE_STATE_SMALL_INK);
        #[expect(clippy::unwrap_used)]
        bencher.bench_local(|| serde_json::to_vec(&save).unwrap());
    }

    #[divan::bench]
    fn serialize_medium(bencher: divan::Bencher) {
        let save = save_state_for(SAVE_STATE_MEDIUM_INK);
        #[expect(clippy::unwrap_used)]
        bencher.bench_local(|| serde_json::to_vec(&save).unwrap());
    }

    #[divan::bench]
    fn serialize_large(bencher: divan::Bencher) {
        let save = save_state_for(SAVE_STATE_LARGE_INK);
        #[expect(clippy::unwrap_used)]
        bencher.bench_local(|| serde_json::to_vec(&save).unwrap());
    }
}

/// Snapshot-retention cost curve (issue #821 Workstream C,
/// `docs/value-model-spec.md` §8's bounded-retention claim): wall time
/// for each of the four (G, M) matrix points — see
/// `snapshot-retention-g10-m10/story.ink`'s header for the mechanism.
/// The load-bearing proof is [`print_bench_counters`]'s `cow_copies`
/// count (exact, per-generation), not this wall-time comparison
/// (informational, same caveat as the other wall-time benches in this
/// file).
mod snapshot_retention_bench {
    use super::{
        SNAPSHOT_RETENTION_G10_M10_INK, SNAPSHOT_RETENTION_G10_M100_INK,
        SNAPSHOT_RETENTION_G100_M10_INK, SNAPSHOT_RETENTION_G100_M100_INK, compile_story_brink,
        run_to_completion,
    };

    #[divan::bench]
    fn g10_m10(bencher: divan::Bencher) {
        let data = compile_story_brink(SNAPSHOT_RETENTION_G10_M10_INK);
        #[expect(clippy::unwrap_used)]
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        bencher.bench_local(|| run_to_completion(&program, line_tables.clone(), &[]));
    }

    #[divan::bench]
    fn g10_m100(bencher: divan::Bencher) {
        let data = compile_story_brink(SNAPSHOT_RETENTION_G10_M100_INK);
        #[expect(clippy::unwrap_used)]
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        bencher.bench_local(|| run_to_completion(&program, line_tables.clone(), &[]));
    }

    #[divan::bench]
    fn g100_m10(bencher: divan::Bencher) {
        let data = compile_story_brink(SNAPSHOT_RETENTION_G100_M10_INK);
        #[expect(clippy::unwrap_used)]
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        bencher.bench_local(|| run_to_completion(&program, line_tables.clone(), &[]));
    }

    #[divan::bench]
    fn g100_m100(bencher: divan::Bencher) {
        let data = compile_story_brink(SNAPSHOT_RETENTION_G100_M100_INK);
        #[expect(clippy::unwrap_used)]
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        bencher.bench_local(|| run_to_completion(&program, line_tables.clone(), &[]));
    }
}

/// `ptr_eq` equality fast path (issue #821, value-model-spec §4/§5):
/// `Value`'s hand-written `PartialEq` short-circuits `Array`/`Map`/`Record`
/// comparison via `Arc::ptr_eq` before falling back to an element-wise
/// structural walk. Exercised directly against `brink_format::Value`
/// rather than through an `.ink` program: brink's `==` operator has no
/// `Array`/`Map` arm in `value_ops::binary_op` yet (unsupported types fault
/// `TypeError`), so there is no ink-level equivalent to isolate the
/// mechanism through today — this bench hits the exact same `PartialEq`
/// impl the runtime's own `map_contains`/list/record comparisons use,
/// which is the honest level to measure at (honest mechanism isolation,
/// per the epic's gate).
mod ptr_eq_bench {
    use std::sync::Arc;

    use brink_format::Value;

    /// Large enough that an O(n) structural walk is measurably slower than
    /// the O(1) `ptr_eq` shortcut, small enough the bench stays fast.
    const N: i32 = 20_000;

    fn big_array() -> Value {
        Value::Array(Arc::new((0..N).map(Value::Int).collect()))
    }

    /// Same `Arc` on both sides (an `Arc::clone` share, e.g. a snapshot
    /// compared against itself) — hits the `ptr_eq` shortcut, O(1).
    #[divan::bench]
    fn same_arc(bencher: divan::Bencher) {
        let a = big_array();
        let b = a.clone();
        bencher.bench_local(|| a == b);
    }

    /// Distinct allocations with structurally identical contents (e.g. two
    /// independently-built arrays, or one COW-copied off the other) — the
    /// `ptr_eq` shortcut misses, falls through to a full O(n) element walk.
    #[divan::bench]
    fn distinct_but_equal(bencher: divan::Bencher) {
        let a = big_array();
        let b = big_array();
        bencher.bench_local(|| a == b);
    }
}

mod end_to_end {
    use super::{Scenario, compile_story, run_to_completion, scenarios};

    #[divan::bench(args = scenarios())]
    fn full_pipeline(bencher: divan::Bencher, scenario: &Scenario) {
        bencher.bench_local(|| {
            let data = compile_story(scenario.ink);
            #[expect(clippy::unwrap_used)]
            let (program, line_tables) = brink_runtime::link(&data).unwrap();
            let program = std::sync::Arc::new(program);
            run_to_completion(&program, line_tables, &scenario.inputs);
        });
    }

    #[divan::bench(args = scenarios())]
    #[expect(clippy::unwrap_used)]
    fn precompiled(bencher: divan::Bencher, scenario: &Scenario) {
        let data = compile_story(scenario.ink);
        bencher.bench_local(|| {
            let (program, line_tables) = brink_runtime::link(&data).unwrap();
            let program = std::sync::Arc::new(program);
            run_to_completion(&program, line_tables, &scenario.inputs);
        });
    }
}

#[expect(clippy::unwrap_used, clippy::print_stderr)]
fn print_hanoi_10_stats() {
    let data = compile_story(HANOI_10_INK);
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let program = std::sync::Arc::new(program);
    let inputs = parse_inputs(HANOI_10_INPUT);
    let stats = run_to_completion(&program, line_tables, &inputs);

    eprintln!("\n── hanoi-10 VM stats ──────────────────────────");
    eprintln!("  opcodes:              {:>10}", stats.opcodes);
    eprintln!("  steps:                {:>10}", stats.steps);
    eprintln!("  threads_created:      {:>10}", stats.threads_created);
    eprintln!("  threads_completed:    {:>10}", stats.threads_completed);
    eprintln!("  frames_pushed:        {:>10}", stats.frames_pushed);
    eprintln!("  frames_popped:        {:>10}", stats.frames_popped);
    eprintln!("  choices_presented:    {:>10}", stats.choices_presented);
    eprintln!("  choices_selected:     {:>10}", stats.choices_selected);
    eprintln!("  snapshot_cache_hits:  {:>10}", stats.snapshot_cache_hits);
    eprintln!(
        "  snapshot_cache_misses:{:>10}",
        stats.snapshot_cache_misses
    );
    eprintln!("  materializations:     {:>10}", stats.materializations);
    eprintln!("───────────────────────────────────────────────\n");
}

#[expect(clippy::unwrap_used, clippy::print_stderr)]
fn print_crucible_8_stats() {
    let data = compile_story(CRUCIBLE_8_INK);
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let program = std::sync::Arc::new(program);
    let inputs = parse_inputs(CRUCIBLE_8_INPUT);
    let stats = run_to_completion(&program, line_tables, &inputs);

    eprintln!("\n── crucible-8 VM stats ────────────────────────");
    eprintln!("  opcodes:              {:>10}", stats.opcodes);
    eprintln!("  steps:                {:>10}", stats.steps);
    eprintln!("  threads_created:      {:>10}", stats.threads_created);
    eprintln!("  threads_completed:    {:>10}", stats.threads_completed);
    eprintln!("  frames_pushed:        {:>10}", stats.frames_pushed);
    eprintln!("  frames_popped:        {:>10}", stats.frames_popped);
    eprintln!("  choices_presented:    {:>10}", stats.choices_presented);
    eprintln!("  choices_selected:     {:>10}", stats.choices_selected);
    eprintln!("  snapshot_cache_hits:  {:>10}", stats.snapshot_cache_hits);
    eprintln!(
        "  snapshot_cache_misses:{:>10}",
        stats.snapshot_cache_misses
    );
    eprintln!("  materializations:     {:>10}", stats.materializations);
    eprintln!("───────────────────────────────────────────────\n");
}

/// Print `bench_counters` snapshots for the two mechanism-isolation
/// programs (issue #821 Workstream B seed) — direct proof, not inference,
/// that `loop-append-10k` amortizes to ~0 COW copies (the §5 cliff, fixed
/// by #576) while `share-then-mutate-5k` pays exactly one copy per share by
/// construction. Only compiled when `--features bench-counters` is passed;
/// with the feature off, `main` skips straight to `divan::main()` and this
/// function doesn't exist.
#[cfg(feature = "bench-counters")]
#[expect(clippy::unwrap_used, clippy::print_stderr)]
fn print_bench_counters() {
    use brink_runtime::bench_counters;

    bench_counters::reset();
    let data = compile_story_brink(LOOP_APPEND_10K_INK);
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let program = std::sync::Arc::new(program);
    run_to_completion(&program, line_tables, &[]);
    let loop_append = bench_counters::snapshot();

    bench_counters::reset();
    let data = compile_story_brink(SHARE_THEN_MUTATE_5K_INK);
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let program = std::sync::Arc::new(program);
    run_to_completion(&program, line_tables, &[]);
    let share_then_mutate = bench_counters::snapshot();

    // Struct field access (issue #821 second program batch): both
    // TypePolicy compiles of the same source, to prove the strict/gradual
    // COW-copy count is identical — the wall-time delta between the two
    // divan benches is dispatch-mechanism cost, not a COW-behavior
    // difference. See STRUCT_FIELD_ACCESS_10K_INK's doc.
    bench_counters::reset();
    let data = compile_story_brink_typed(STRUCT_FIELD_ACCESS_10K_INK, TypePolicy::Strict);
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let program = std::sync::Arc::new(program);
    run_to_completion(&program, line_tables, &[]);
    let struct_strict = bench_counters::snapshot();

    bench_counters::reset();
    let data = compile_story_brink_typed(STRUCT_FIELD_ACCESS_10K_INK, TypePolicy::Gradual);
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let program = std::sync::Arc::new(program);
    run_to_completion(&program, line_tables, &[]);
    let struct_gradual = bench_counters::snapshot();

    eprintln!("\n── bench-counters (Arc-clone / COW-copy events) ──");
    eprintln!(
        "  loop-append-10k:       cow_copies={:>6} arc_clones={:>6}",
        loop_append.cow_copies, loop_append.arc_clones
    );
    eprintln!(
        "  share-then-mutate-5k:  cow_copies={:>6} arc_clones={:>6}",
        share_then_mutate.cow_copies, share_then_mutate.arc_clones
    );
    eprintln!(
        "  struct-field-access-10k (strict):  cow_copies={:>6} arc_clones={:>6}",
        struct_strict.cow_copies, struct_strict.arc_clones
    );
    eprintln!(
        "  struct-field-access-10k (gradual): cow_copies={:>6} arc_clones={:>6}",
        struct_gradual.cow_copies, struct_gradual.arc_clones
    );
    eprintln!(
        "  (fn-value benches: not instrumented — bench-counters covers \
         Array/Map/Record COW only, not Closure allocation; see \
         docs/runtime-bench.md's honest-mechanism-isolation note)"
    );
    eprintln!("───────────────────────────────────────────────────\n");
}

/// Print `bench_counters` snapshots for the snapshot-retention (G, M)
/// matrix (issue #821 Workstream C, `docs/value-model-spec.md` §8's
/// bounded-retention claim) — the load-bearing proof that `cow_copies`
/// tracks retained generations (`G`) alone, not `G * M` total mutations:
/// g10-m10 and g10-m100 should report the same `cow_copies` despite a
/// 10x difference in mutation count at the same retention depth, and
/// g10-m100/g100-m10 (identical total mutation count, 10x different
/// retention depth) should report a 10x *different* `cow_copies` — the
/// two-sided isolation that pins the mechanism down to `G`, not the
/// total work done.
#[cfg(feature = "bench-counters")]
#[expect(clippy::unwrap_used, clippy::print_stderr)]
fn print_snapshot_retention_counters() {
    use brink_runtime::bench_counters;

    let combos: [(&str, &str); 4] = [
        ("g10-m10", SNAPSHOT_RETENTION_G10_M10_INK),
        ("g10-m100", SNAPSHOT_RETENTION_G10_M100_INK),
        ("g100-m10", SNAPSHOT_RETENTION_G100_M10_INK),
        ("g100-m100", SNAPSHOT_RETENTION_G100_M100_INK),
    ];

    eprintln!("\n── snapshot-retention bench-counters (G, M matrix) ──");
    for (name, ink) in combos {
        bench_counters::reset();
        let data = compile_story_brink(ink);
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        run_to_completion(&program, line_tables, &[]);
        let snap = bench_counters::snapshot();
        eprintln!(
            "  {name:<10} cow_copies={:>6} arc_clones={:>6}",
            snap.cow_copies, snap.arc_clones
        );
    }
    eprintln!(
        "  (expect cow_copies == G + 1 for each point, independent of M \
         — the §8 bounded-retention claim: one COW copy per retained \
         generation, not per mutation. The '+1' is a one-time cost paid \
         by `history`'s own first push, diverging from the shared \
         empty-array-literal pool `#[]` starts from — the same \
         mechanism loop-append-10k's single cow_copies=1 already proves \
         — not part of the G-generations retention loop under test; see \
         docs/runtime-bench.md.)"
    );
    eprintln!("──────────────────────────────────────────────────────\n");
}

/// Print `bench_counters` deltas across a save/load cycle (issue #821
/// Workstream C) on the medium `SaveState` shape — resets the counters,
/// captures `Story::save_state()`, then reconciles it back in via
/// `Story::load_state`, and reports what the counters saw. Both
/// `save_state`/`load_state` (`crates/brink-runtime/src/save.rs`) now
/// call the same `note_value_share` the VM's `Opcode::GetGlobal` uses
/// (added alongside this benchmark), so a collection-typed global
/// read/written during the cycle is counted exactly like a VM-driven
/// read would be. Scalars (the common case in a save with few
/// collections) aren't collection-typed and so don't increment either
/// counter — expect `arc_clones` roughly proportional to the number of
/// collection-typed globals touched, `cow_copies == 0` (a fresh
/// save/load round trip on a single-owner `Story` never finds a shared
/// `Arc`, so `array_make_mut`/`map_make_mut`/`record_make_mut` are never
/// reached by this path at all — save/load only ever reads/replaces
/// whole global slots, never mutates a collection in place).
#[cfg(feature = "bench-counters")]
#[expect(clippy::unwrap_used, clippy::print_stderr)]
fn print_save_load_counter_deltas() {
    use brink_runtime::bench_counters;

    let data = compile_story_brink(SAVE_STATE_MEDIUM_INK);
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let program = std::sync::Arc::new(program);
    let mut story = run_to_completion_keep_story(&program, line_tables, &[]);

    bench_counters::reset();
    let saved = story.save_state();
    let after_save = bench_counters::snapshot();

    bench_counters::reset();
    let _report = story.load_state(&saved);
    let after_load = bench_counters::snapshot();

    eprintln!("\n── save/load bench-counters (save-state-medium) ──");
    eprintln!(
        "  save_state(): cow_copies={:>6} arc_clones={:>6}",
        after_save.cow_copies, after_save.arc_clones
    );
    eprintln!(
        "  load_state(): cow_copies={:>6} arc_clones={:>6}",
        after_load.cow_copies, after_load.arc_clones
    );
    eprintln!(
        "  (save/load only reads/replaces whole global slots — it never \
         calls array_make_mut/map_make_mut/record_make_mut, so \
         cow_copies == 0 here is expected, not a gap in coverage)"
    );
    eprintln!("───────────────────────────────────────────────────\n");
}

/// Print `SaveState` wire sizes (issue #821 Workstream C — "bytes
/// serialized") for the three story-state shapes, JSON-encoded exactly
/// as `brink-web`'s wasm boundary does
/// (`crates/brink-web/src/session.rs`'s `save`/`load` methods use
/// `serde_json::to_string`/`from_str` on the same `SaveState` type).
/// Always runs (not gated behind `bench-counters` — this doesn't need
/// the counter instrumentation, just the serialized byte count).
#[expect(clippy::unwrap_used, clippy::print_stderr)]
fn print_save_state_wire_sizes() {
    let shapes: [(&str, &str); 3] = [
        ("small", SAVE_STATE_SMALL_INK),
        ("medium", SAVE_STATE_MEDIUM_INK),
        ("large", SAVE_STATE_LARGE_INK),
    ];

    eprintln!("\n── SaveState wire sizes (serde_json, wasm-boundary encoding) ──");
    for (name, ink) in shapes {
        let data = compile_story_brink(ink);
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        let story = run_to_completion_keep_story(&program, line_tables, &[]);
        let save = story.save_state();
        let bytes = serde_json::to_vec(&save).unwrap();
        eprintln!("  {name:<8} {:>10} bytes", bytes.len());
    }
    eprintln!("─────────────────────────────────────────────────────────────\n");
}

/// Best-effort current resident-set-size read, in kilobytes, via `ps -o
/// rss=` on the current process (issue #821 Workstream C: "if `heap_size`
/// (#538) landed via FG-5 use it; else RSS-delta with the caveat
/// stated" — #538 is still open as of this benchmark, so this is the
/// fallback). Deliberately shells out to `ps` rather than adding a new
/// `libc`/`getrusage` dependency: `ps -o rss=` reports *current* RSS in
/// KB uniformly on both Linux and macOS (unlike `getrusage`'s
/// `ru_maxrss`, which is peak-since-start and platform-unit-inconsistent
/// — bytes on macOS, KB on Linux). Returns `None` on any failure
/// (`ps` unavailable, unparseable output, non-Unix platform) rather than
/// panicking — this is diagnostic-only output, never load-bearing for
/// the gate.
fn current_rss_kb() -> Option<u64> {
    let pid = std::process::id();
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

/// Print RSS-delta samples across the snapshot-retention (G, M) matrix
/// (issue #821 Workstream C, `docs/value-model-spec.md` §8's memory
/// claim: "bounds memory at (retained generations + 1 live) per
/// value"). **Caveat, stated per the epic's honesty gate**: this is a
/// coarse, single-process, best-effort proxy, not an exact per-value
/// byte accounting (that would need `#538`'s `heap_size` estimators,
/// still open as of this benchmark). Process RSS reflects the whole
/// process's memory (allocator overhead, fragmentation, unrelated
/// allocations, and whether the allocator has returned freed pages to
/// the OS — which it may not, even after the `Story` from a prior combo
/// is dropped), not just the `history` array under test. Read as "does
/// growing (G, M) roughly track a linear-ish RSS growth, not something
/// worse" — a qualitative sanity check, not a byte-exact tripwire
/// (Workstream D's job, not this one's, if a tripwire is ever built on
/// top of this).
#[expect(clippy::unwrap_used, clippy::print_stderr)]
fn print_snapshot_retention_rss() {
    let combos: [(&str, &str); 4] = [
        ("g10-m10", SNAPSHOT_RETENTION_G10_M10_INK),
        ("g10-m100", SNAPSHOT_RETENTION_G10_M100_INK),
        ("g100-m10", SNAPSHOT_RETENTION_G100_M10_INK),
        ("g100-m100", SNAPSHOT_RETENTION_G100_M100_INK),
    ];

    eprintln!("\n── snapshot-retention RSS deltas (coarse — see caveat above main()) ──");
    match current_rss_kb() {
        Some(_) => {
            for (name, ink) in combos {
                let before = current_rss_kb();
                let data = compile_story_brink(ink);
                let (program, line_tables) = brink_runtime::link(&data).unwrap();
                let program = std::sync::Arc::new(program);
                let story = run_to_completion_keep_story(&program, line_tables, &[]);
                let after = current_rss_kb();
                // Keep `story` (and its retained `history` array) alive
                // until after the "after" sample, then drop explicitly
                // before the next combo so each measurement starts from
                // a comparable baseline.
                drop(story);
                let delta = after
                    .zip(before)
                    .map(|(a, b)| a.cast_signed() - b.cast_signed());
                eprintln!(
                    "  {name:<10} rss_before={before:>8?}KB rss_after={after:>8?}KB delta={delta:>8?}KB"
                );
            }
        }
        None => {
            eprintln!("  (ps -o rss= unavailable on this platform — skipped)");
        }
    }
    eprintln!("────────────────────────────────────────────────────────────────────\n");
}

fn main() {
    // Force scenario initialization before benchmarks run.
    let _ = scenarios();
    print_hanoi_10_stats();
    print_crucible_8_stats();
    print_save_state_wire_sizes();
    print_snapshot_retention_rss();
    #[cfg(feature = "bench-counters")]
    {
        print_bench_counters();
        print_snapshot_retention_counters();
        print_save_load_counter_deltas();
    }
    divan::main();
}
