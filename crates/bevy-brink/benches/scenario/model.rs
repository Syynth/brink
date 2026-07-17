//! Scenario configuration, synthetic story generator, frame-loop systems,
//! and the scenario driver — the reusable core of the scenario harness
//! (issue #900, BH-B-1). See `benches/scenario_bench.rs`'s module docs for
//! the full honesty write-up (what each phase/axis can and can't see).
//!
//! Included via `#[path]` from both `benches/scenario_bench.rs` (the real
//! bench binary — `report.rs` adds CLI/CSV/markdown on top) and
//! `tests/scenario_bench_model.rs` (a real `cargo test` path exercising
//! this same source, since the bench binary's `test = false` means
//! `cargo test` never runs its `main()` — see that file's module docs).

use std::io;
use std::time::{Duration, Instant};

use bevy::MinimalPlugins;
use bevy::asset::{AssetPlugin, Assets};
use bevy::ecs::system::SystemId;
use bevy::prelude::{
    App, Commands, Component, Entity, IntoScheduleConfigs as _, Query, Res, ResMut, Resource,
    Update, With, Without, World as EcsWorld,
};
use bevy_brink::{
    Advance, BrinkContext, BrinkFlow, BrinkGlobals, BrinkPlugin, BrinkProgram, BrinkStory,
    FallbackHandler, FlowInstance, Line, LineTablesAsset, Program, ProgramAsset, advance_batch,
    flow_context_view,
};

// ── 1. Scenario configuration — the pre-parallelism axes ───────────────

/// Per-turn ink workload shape. Held constant across a scenario run;
/// varying it is a future exploration axis (the checked-in baselines fix
/// it at [`TurnWeight::Medium`] and vary `flow_count` only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnWeight {
    /// One generated sentence, no var mutation.
    Light,
    /// Three sentences + one var increment + one interpolation.
    Medium,
    /// Six sentences + one var increment + one interpolation + one inline
    /// conditional.
    Heavy,
}

impl TurnWeight {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Medium => "medium",
            Self::Heavy => "heavy",
        }
    }
}

/// The axes that exist **pre-parallelism** (`BH-1`/`BH-3` contention axes —
/// access disjointness, wake fan-out, change pressure — land with their own
/// phases, per the epic's #897 scope note).
#[derive(Debug, Clone)]
pub struct ScenarioConfig {
    /// Human-readable name for report rows (e.g. `"serial-1k"`).
    pub name: String,
    /// Total flow entities spawned.
    pub flow_count: usize,
    /// Fraction of `flow_count` assigned **active** at spawn (advanced
    /// every frame); the rest are spawned `ScenarioParked` and never
    /// touched — see the module-level Collect honesty note.
    pub active_fraction: f64,
    /// Inert background entities sharing the `World` — see the
    /// module-level `world_size` honesty note.
    pub world_size: usize,
    /// Per-turn ink workload — see [`TurnWeight`].
    pub turn_weight: TurnWeight,
    /// Number of `App::update()` frames to drive.
    pub frames: usize,
    /// Fixed PCG seed — no wall-clock/OS entropy anywhere in the generated
    /// story or the scenario itself, so a run is byte-identical across
    /// machines (deterministic per the epic's gate).
    pub seed: u64,
    /// BH follow-up (#911, deliverable 3): when `true`, the generated story
    /// carries a collection-typed global (`live`, an `Array`) that every
    /// turn shares into a second global (`history`, an Arc-clone —
    /// `arc_clones` moves) and then mutates in place while shared (a
    /// `Value::array_make_mut` COW — `cow_copies` moves), mirroring
    /// `benchmarks/stories/snapshot-retention-g10-m10/story.ink`'s
    /// share-then-mutate shape. Every other scenario config in this file
    /// holds this `false` and stays scalar-only — this is a deliberately
    /// separate exploration axis, not a change to the checked-in
    /// `serial-driver.csv` baselines (`docs/bevy-bench.md`'s honesty note
    /// on why `cow_copies`/`arc_clones` read 0 there is unaffected by this
    /// axis existing).
    pub collection_global: bool,
}

// ── 2. Synthetic story generator — corpus PCG idiom ─────────────────────

/// Tiny deterministic PRNG (PCG-style LCG step), the same idiom
/// `compile_bench.rs`/`editor_session_bench.rs` use for their synthetic
/// corpora: no wall-clock or OS entropy anywhere, so generated content is
/// byte-identical on every run, on every machine.
struct Lcg(u64);

impl Lcg {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "range is always small (word/sentence-length picks); truncation is intended"
    )]
    fn pick(&mut self, lo: usize, hi: usize) -> usize {
        lo + (self.next() as usize) % (hi - lo + 1)
    }
}

const WORDS: [&str; 20] = [
    "lantern", "harbor", "signal", "vault", "ember", "cipher", "meadow", "static", "orchard",
    "beacon", "drift", "hollow", "ledger", "murmur", "quarry", "relay", "sable", "tundra",
    "vesper", "wharf",
];

fn sentence(rng: &mut Lcg, min_words: usize, max_words: usize) -> String {
    let n = rng.pick(min_words, max_words);
    let mut words = Vec::with_capacity(n);
    for i in 0..n {
        let w = WORDS[rng.pick(0, WORDS.len() - 1)];
        if i == 0 {
            let mut chars = w.chars();
            let first = chars.next().map(|c| c.to_ascii_uppercase());
            words.push(first.into_iter().chain(chars).collect::<String>());
        } else {
            words.push(w.to_string());
        }
    }
    let mut s = words.join(" ");
    s.push('.');
    s
}

/// Generate one flow's story: an endless loop over a single knot ending in
/// a sticky choice back to itself. Every active flow's "turn" is therefore
/// always the same shape (text → `Choices`, auto-picked) — deliberately
/// simple so `flow_count` scaling is the only variable under test.
///
/// `collection_global` (#911, deliverable 3) layers in a `live`/`history`
/// array pair — see [`ScenarioConfig::collection_global`]'s doc for the
/// share-then-mutate mechanism this exercises.
pub fn generate_story(turn_weight: TurnWeight, seed: u64, collection_global: bool) -> String {
    let mut rng = Lcg::new(seed);
    let mut s = String::from("// Synthetic scenario story — generated, deterministic (#900).\n");

    let needs_counter = matches!(turn_weight, TurnWeight::Medium | TurnWeight::Heavy);
    if needs_counter {
        s.push_str("VAR turn_count = 0\n");
    }
    if collection_global {
        s.push_str("VAR live = 0\nVAR history = 0\n");
        s.push_str("~ {\n    live = #[0, 0, 0, 0]\n    history = #[]\n}\n");
    }
    s.push_str("-> loop_knot\n\n=== loop_knot ===\n");

    let sentences = match turn_weight {
        TurnWeight::Light => 1,
        TurnWeight::Medium => 3,
        TurnWeight::Heavy => 6,
    };
    for _ in 0..sentences {
        s.push_str(&sentence(&mut rng, 5, 10));
        s.push('\n');
    }
    if needs_counter {
        s.push_str("~ turn_count = turn_count + 1\n");
        s.push_str("Turn number {turn_count}.\n");
    }
    if matches!(turn_weight, TurnWeight::Heavy) {
        s.push_str("{turn_count mod 2 == 0: An even beat settles.|An odd beat lingers.}\n");
    }
    if collection_global {
        // Every turn: share `live` into `history` (a collection-typed
        // global read — `arc_clones` moves), then mutate `live` in place
        // while `history` still holds last turn's share (a
        // `Value::array_make_mut` COW — `cow_copies` moves). Steady state
        // after the first turn: one shared owner (`history`'s latest
        // entry) exists whenever `live` is next mutated.
        s.push_str("~ {\n    push(history, live)\n    live[0] = len(history)\n}\n");
    }
    s.push_str("+ [Continue]\n    -> loop_knot\n");
    s
}

/// Marker `M` type for the scenario's `BrinkFlow<M>`/`BrinkContext<M>`/
/// `BrinkGlobals<M>` — a single synthetic story per scenario run, so one
/// marker type suffices.
#[derive(Debug, Clone, Copy, Default)]
struct ScenarioFlow;

/// Present on the fixed fraction of flows that are spawned parked and
/// never advanced — see the module-level Collect honesty note.
#[derive(Component)]
struct ScenarioParked;

/// Inert background entity representing unrelated ECS `World` content —
/// see the module-level `world_size` honesty note.
#[derive(Component)]
struct ScenarioWorldFiller;

#[derive(Component, Default)]
struct WorldFillerTicks(u32);

#[derive(Resource, Default)]
struct ActiveFlowSet(Vec<Entity>);

#[derive(Resource, Default)]
struct PhaseClock {
    collect: Duration,
    step: Duration,
    apply: Duration,
}

#[derive(Resource, Default)]
struct FrameCounters {
    turns_completed: u64,
    /// A `Step` call landing anywhere other than `Choices` (or erroring)
    /// is unexpected for this always-looping template — counted rather
    /// than silently ignored, per the "flag silent data drops" house rule.
    flow_anomalies: u64,
}

#[derive(Resource, Default)]
struct FrameIndex(u64);

#[derive(Resource)]
struct ScenarioProgram {
    program: Program,
    line_tables: Vec<Vec<brink_format::LineEntry>>,
}

// ── 3. Frame-loop systems — Collect / Step / Apply ──────────────────────

fn tick_world_filler(mut fillers: Query<&mut WorldFillerTicks, With<ScenarioWorldFiller>>) {
    for mut f in &mut fillers {
        f.0 = f.0.wrapping_add(1);
    }
}

fn collect_active_flows(
    mut clock: ResMut<PhaseClock>,
    mut active: ResMut<ActiveFlowSet>,
    flows: Query<Entity, (With<BrinkFlow<ScenarioFlow>>, Without<ScenarioParked>)>,
) {
    let start = Instant::now();
    active.0.clear();
    active.0.extend(flows.iter());
    clock.collect = start.elapsed();
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "bevy system params are always taken by value — Res<T>/ResMut<T> are themselves the borrow, not a value to be reborrowed"
)]
fn step_active_flows(
    mut clock: ResMut<PhaseClock>,
    mut counters: ResMut<FrameCounters>,
    active: Res<ActiveFlowSet>,
    program: Res<ScenarioProgram>,
    mut globals: ResMut<BrinkGlobals<ScenarioFlow>>,
    mut flows: Query<(
        &mut BrinkFlow<ScenarioFlow>,
        &mut BrinkContext<ScenarioFlow>,
    )>,
    mut commands: Commands,
) {
    let start = Instant::now();
    for &entity in &active.0 {
        let Ok((mut flow, mut ctx)) = flows.get_mut(entity) else {
            counters.flow_anomalies += 1;
            continue;
        };
        let mut view = flow_context_view(&mut globals, &mut ctx);
        let advanced = flow.advance_until_terminal(
            &program.program,
            &program.line_tables,
            &mut view,
            &FallbackHandler,
            entity,
            &mut commands,
        );
        match advanced {
            Ok(Advance::Line(Line::Choices { .. })) => {
                if flow.choose(&mut view, 0).is_ok() {
                    counters.turns_completed += 1;
                } else {
                    counters.flow_anomalies += 1;
                }
            }
            Ok(_) | Err(_) => counters.flow_anomalies += 1,
        }
    }
    clock.step = start.elapsed();
}

fn apply_bookkeeping(mut clock: ResMut<PhaseClock>, mut frame_index: ResMut<FrameIndex>) {
    let start = Instant::now();
    // Placeholder: today's writes are immediate (see the module docs'
    // Apply honesty note) — this bump is the only real per-frame Apply
    // work that exists yet.
    frame_index.0 += 1;
    clock.apply = start.elapsed();
}

// ── 4. RSS + #821 counters ──────────────────────────────────────────────

/// Best-effort current resident-set-size read, in kilobytes, via `ps -o
/// rss=` on the current process — same idiom as
/// `crates/brink-runtime/benches/runtime.rs`'s `current_rss_kb` (issue
/// #821 Workstream C): `#538`'s `heap_size` estimators aren't landed, so
/// this coarse whole-process proxy is what's available. `None` on any
/// failure rather than panicking — diagnostic-only, never load-bearing.
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

// ── 5. Scenario driver ───────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ScenarioError {
    #[error("scenario config must have flow_count >= 1")]
    NoFlows,
}

#[derive(Debug, Clone)]
pub struct ScenarioResult {
    pub name: String,
    pub flow_count: usize,
    pub active_fraction: f64,
    pub world_size: usize,
    pub turn_weight: TurnWeight,
    pub frames: usize,
    pub seed: u64,
    pub frame_p50_ms: f64,
    pub frame_p99_ms: f64,
    pub collect_p50_us: f64,
    pub step_p50_us: f64,
    pub apply_p50_us: f64,
    pub turns_per_sec: f64,
    pub turns_completed: u64,
    pub flow_anomalies: u64,
    pub rss_before_kb: Option<u64>,
    pub rss_after_kb: Option<u64>,
    pub rss_delta_kb: Option<i64>,
    pub cow_copies: Option<u64>,
    pub arc_clones: Option<u64>,
}

#[expect(
    clippy::cast_precision_loss,
    reason = "percentile math over sample counts well under 2^53; exactness isn't required for a percentile index"
)]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "p is always in [0.0, 1.0] and sorted is non-empty here, so the rounded index is always a small non-negative in-bounds value"
)]
fn percentile(samples: &[Duration], p: f64) -> Duration {
    if samples.is_empty() {
        return Duration::ZERO;
    }
    let mut sorted: Vec<Duration> = samples.to_vec();
    sorted.sort_unstable();
    let idx = (((sorted.len() - 1) as f64) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[expect(
    clippy::cast_precision_loss,
    reason = "active-fraction math is a UI-facing ratio, not an exactness-critical count"
)]
pub fn active_count(flow_count: usize, active_fraction: f64) -> usize {
    let count = ((flow_count as f64) * active_fraction).round();
    #[expect(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "active_fraction is always in [0.0, 1.0] and flow_count > 0 here, so count is always a small non-negative value"
    )]
    let count = count as usize;
    count.min(flow_count)
}

/// A linked `(Program, line tables)` pair, ready to drive.
type CompiledStory = (Program, Vec<Vec<brink_format::LineEntry>>);

/// Compile the generated scenario story into a linked [`CompiledStory`]
/// pair — shared by the serial ([`run_scenario`]) and batch
/// ([`run_batch_scenario`]) drivers so both time the exact same compiled
/// story for a given config.
///
/// Brink dialect (docs/t1b-surface-spec.md §1): the `collection_global`
/// axis's `~ { … }` blocks, `#[…]` array literals, `push`/`len`, and
/// postfix indexing are brink-extension syntax the default `StrictInk`
/// dialect rejects at compile time (`E051`). The gate is purely a
/// diagnostic (`dialect_gate.rs`: "LIR lowering doesn't consult the
/// dialect at all") — it never changes codegen for a story that uses
/// none of that syntax, so enabling it unconditionally here doesn't
/// affect the scalar-only baseline configs' compiled output or timings.
fn compile_scenario_story(
    config: &ScenarioConfig,
) -> Result<CompiledStory, Box<dyn std::error::Error>> {
    let source = generate_story(config.turn_weight, config.seed, config.collection_global);
    let options = brink_compiler::AnalysisOptions {
        dialect: brink_compiler::Dialect::Brink,
        ..Default::default()
    };
    let output = brink_compiler::compile_with_options(
        "scenario.ink",
        |path| {
            if path == "scenario.ink" {
                Ok(source.clone())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("unexpected include: {path}"),
                ))
            }
        },
        options,
    )?;
    Ok(brink_runtime::link(&output.data)?)
}

/// Shared measurement tail for both scenario drivers: drive `config.frames`
/// frames of a fully set-up `app`, sampling the per-frame wall clock and the
/// driver's [`PhaseClock`], then fold counters/RSS/percentiles into a
/// [`ScenarioResult`]. The driver's own systems are responsible for writing
/// [`PhaseClock`] and [`FrameCounters`] each frame — a phase a driver never
/// writes (batch mode's `collect`, folded inside `advance_batch`) samples as
/// zero, reported as such rather than silently omitted.
fn measure_app(mut app: App, config: &ScenarioConfig) -> ScenarioResult {
    #[cfg(feature = "bench-counters")]
    brink_runtime::bench_counters::reset();

    let rss_before = current_rss_kb();
    let mut frame_samples = Vec::with_capacity(config.frames);
    let mut collect_samples = Vec::with_capacity(config.frames);
    let mut step_samples = Vec::with_capacity(config.frames);
    let mut apply_samples = Vec::with_capacity(config.frames);

    for _ in 0..config.frames {
        let frame_start = Instant::now();
        app.update();
        frame_samples.push(frame_start.elapsed());
        let clock = app.world().resource::<PhaseClock>();
        collect_samples.push(clock.collect);
        step_samples.push(clock.step);
        apply_samples.push(clock.apply);
    }
    let rss_after = current_rss_kb();
    let rss_delta_kb = rss_after
        .zip(rss_before)
        .map(|(a, b)| a.cast_signed() - b.cast_signed());

    #[cfg(feature = "bench-counters")]
    let (cow_copies, arc_clones) = {
        let snap = brink_runtime::bench_counters::snapshot();
        (Some(snap.cow_copies), Some(snap.arc_clones))
    };
    #[cfg(not(feature = "bench-counters"))]
    let (cow_copies, arc_clones) = (None, None);

    let frame_counters = app.world().resource::<FrameCounters>();
    let turns_completed = frame_counters.turns_completed;
    let flow_anomalies = frame_counters.flow_anomalies;

    let total_wall: Duration = frame_samples.iter().sum();
    #[expect(
        clippy::cast_precision_loss,
        reason = "turns_completed is well under 2^53 at these scenario sizes; throughput math doesn't need bit-exactness"
    )]
    let turns_per_sec = if total_wall.is_zero() {
        0.0
    } else {
        (turns_completed as f64) / total_wall.as_secs_f64()
    };

    ScenarioResult {
        name: config.name.clone(),
        flow_count: config.flow_count,
        active_fraction: config.active_fraction,
        world_size: config.world_size,
        turn_weight: config.turn_weight,
        frames: config.frames,
        seed: config.seed,
        frame_p50_ms: percentile(&frame_samples, 0.50).as_secs_f64() * 1000.0,
        frame_p99_ms: percentile(&frame_samples, 0.99).as_secs_f64() * 1000.0,
        collect_p50_us: percentile(&collect_samples, 0.50).as_secs_f64() * 1_000_000.0,
        step_p50_us: percentile(&step_samples, 0.50).as_secs_f64() * 1_000_000.0,
        apply_p50_us: percentile(&apply_samples, 0.50).as_secs_f64() * 1_000_000.0,
        turns_per_sec,
        turns_completed,
        flow_anomalies,
        rss_before_kb: rss_before,
        rss_after_kb: rss_after,
        rss_delta_kb,
        cow_copies,
        arc_clones,
    }
}

pub fn run_scenario(config: &ScenarioConfig) -> Result<ScenarioResult, Box<dyn std::error::Error>> {
    if config.flow_count == 0 {
        return Err(Box::new(ScenarioError::NoFlows));
    }

    let (program, line_tables) = compile_scenario_story(config)?;

    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(ActiveFlowSet::default());
    app.insert_resource(PhaseClock::default());
    app.insert_resource(FrameCounters::default());
    app.insert_resource(FrameIndex::default());

    let active = active_count(config.flow_count, config.active_fraction);
    let mut shared_world = None;
    for i in 0..config.flow_count {
        let (flow_instance, world) = FlowInstance::new_at_root(&program);
        if shared_world.is_none() {
            shared_world = Some(world);
        }
        let mut entity = app.world_mut().spawn((
            BrinkFlow::<ScenarioFlow>::new(flow_instance),
            BrinkContext::<ScenarioFlow>::default(),
        ));
        if i >= active {
            entity.insert(ScenarioParked);
        }
    }
    // Guaranteed `Some` — the loop above ran at least once (`flow_count >=
    // 1`, checked above) and sets it on the first iteration.
    let Some(shared_world) = shared_world else {
        return Err(Box::new(ScenarioError::NoFlows));
    };
    app.insert_resource(BrinkGlobals::<ScenarioFlow>::new(shared_world));
    app.insert_resource(ScenarioProgram {
        program,
        line_tables,
    });

    for _ in 0..config.world_size {
        app.world_mut()
            .spawn((ScenarioWorldFiller, WorldFillerTicks::default()));
    }

    app.add_systems(
        Update,
        (
            tick_world_filler,
            collect_active_flows,
            step_active_flows,
            apply_bookkeeping,
        )
            .chain(),
    );

    Ok(measure_app(app, config))
}

// ── 6. Batch-mode scenario driver (BH-2, #914) ──────────────────────────

/// The registered [`advance_batch`] system the batch driver's exclusive
/// wrapper runs (and times) each frame via `World::run_system` — so the
/// measured span covers the *whole* batch turn (Collect → Step → Apply,
/// including the flush of the commands the batch queued), exactly the unit
/// `BH-3`'s parallel Step will be compared on.
#[derive(Resource)]
struct BatchDriver(SystemId);

/// Run one batch turn ([`advance_batch`]) and record its wall clock into
/// [`PhaseClock::step`]. Batch mode fuses Collect/Step/Apply inside
/// `advance_batch`, so the harness cannot time them separately without
/// reaching into `bevy-brink` internals — the fused turn is reported in the
/// `step` column and `collect` samples as zero (see the batch preamble in
/// `report.rs` / `docs/bevy-bench.md`).
fn run_batch_turn(world: &mut EcsWorld) {
    let id = world.resource::<BatchDriver>().0;
    let start = Instant::now();
    let result = world.run_system(id);
    let elapsed = start.elapsed();
    world.resource_mut::<PhaseClock>().step = elapsed;
    if result.is_err() {
        // A failed `run_system` (unregistered/removed system) would silently
        // zero the whole batch turn — surface it as an anomaly instead.
        world.resource_mut::<FrameCounters>().flow_anomalies += 1;
    }
}

/// Host-side auto-pick: after the batch turn, every active flow sits on its
/// looping template's sticky choice — pick choice 0 so the next frame's batch
/// turn drives a fresh turn, mirroring the serial driver's in-Step auto-pick.
/// Recorded into [`PhaseClock::apply`] (the between-turn host phase batch
/// mode adds; serial mode's `apply` is a placeholder bump — see the batch
/// preamble for the column mapping).
#[expect(
    clippy::type_complexity,
    reason = "bevy query tuples with a marker filter are inherently wide; a type alias would only move the width somewhere less readable"
)]
fn choose_batch_flows(
    mut clock: ResMut<PhaseClock>,
    mut counters: ResMut<FrameCounters>,
    mut globals: ResMut<BrinkGlobals<ScenarioFlow>>,
    mut flows: Query<
        (
            &mut BrinkFlow<ScenarioFlow>,
            &mut BrinkContext<ScenarioFlow>,
        ),
        With<BrinkProgram<ScenarioFlow>>,
    >,
) {
    let start = Instant::now();
    for (mut flow, mut ctx) in &mut flows {
        let mut view = flow_context_view(&mut globals, &mut ctx);
        if flow.choose(&mut view, 0).is_ok() {
            counters.turns_completed += 1;
        } else {
            counters.flow_anomalies += 1;
        }
    }
    clock.apply = start.elapsed();
}

/// Batch-mode counterpart of [`run_scenario`]: the same generated story,
/// axes, seed, and measurement tail, but flows advance through
/// [`advance_batch`] (BH-2's frame-start-consistent batch driver, §12.4)
/// instead of the serial per-flow `advance_until_terminal` loop.
///
/// Differences from the serial driver, stated honestly:
///
/// - **Plugin path.** `advance_batch` is the plugin-hosted entry point — its
///   query needs the [`BrinkProgram`]/[`BrinkLocale`](bevy_brink::BrinkLocale)
///   asset handles a real host wires up, so this driver runs under
///   [`BrinkPlugin`] + `AssetPlugin` with the story inserted as real assets
///   (the serial driver bypasses assets with a plain resource). The plugin's
///   other per-frame systems are all gated no-ops in this scenario.
/// - **Active:parked.** `advance_batch` steps every flow its query matches,
///   so "parked" here means *spawned without the `BrinkStory` bundle* — the
///   flow entity exists in the ECS world but never enters the batch. Same
///   static-partition honesty caveat as the serial Collect note.
/// - **Phase columns.** Collect/Step/Apply are fused inside `advance_batch`;
///   the whole batch turn (including its command flush) lands in the `step`
///   column, the host auto-pick pass lands in `apply`, and `collect` reads 0.
/// - **Per-flow snapshot clone.** Each stepped flow clones the frame-start
///   world (BH-2's documented serial cost; §12.2 "borrow, don't copy" is the
///   BH-3 optimization) — expect batch `step` to sit above serial `step` at
///   every flow count until BH-3 lands.
pub fn run_batch_scenario(
    config: &ScenarioConfig,
) -> Result<ScenarioResult, Box<dyn std::error::Error>> {
    if config.flow_count == 0 {
        return Err(Box::new(ScenarioError::NoFlows));
    }

    let (program, line_tables) = compile_scenario_story(config)?;

    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(AssetPlugin::default());
    app.add_plugins(BrinkPlugin::<ScenarioFlow>::default());
    app.insert_resource(PhaseClock::default());
    app.insert_resource(FrameCounters::default());

    // Flow instances are created against `program` before it moves into the
    // asset store (the batch driver reads it back out of `Assets` each turn).
    let active = active_count(config.flow_count, config.active_fraction);
    let mut shared_world = None;
    let mut instances = Vec::with_capacity(config.flow_count);
    for _ in 0..config.flow_count {
        let (flow_instance, world) = FlowInstance::new_at_root(&program);
        if shared_world.is_none() {
            shared_world = Some(world);
        }
        instances.push(flow_instance);
    }
    // Guaranteed `Some` — the loop above ran at least once (`flow_count >=
    // 1`, checked above) and sets it on the first iteration.
    let Some(shared_world) = shared_world else {
        return Err(Box::new(ScenarioError::NoFlows));
    };

    let initial_context = shared_world.clone();
    let program_handle = app
        .world_mut()
        .resource_mut::<Assets<ProgramAsset>>()
        .add(ProgramAsset {
            program,
            initial_context,
            // No capability manifest/registry in this scenario — BH-1's
            // access bookkeeping records `None` per flow, which is the
            // honest "nothing wired" value, not a stub.
            effect_rows: Vec::new(),
        });
    let tables_handle = app
        .world_mut()
        .resource_mut::<Assets<LineTablesAsset>>()
        .add(LineTablesAsset {
            tables: line_tables,
        });

    for (i, flow_instance) in instances.into_iter().enumerate() {
        let mut entity = app.world_mut().spawn((
            BrinkFlow::<ScenarioFlow>::new(flow_instance),
            BrinkContext::<ScenarioFlow>::default(),
        ));
        if i < active {
            entity.insert(BrinkStory::<ScenarioFlow>::new(
                program_handle.clone(),
                tables_handle.clone(),
            ));
        } else {
            entity.insert(ScenarioParked);
        }
    }
    app.insert_resource(BrinkGlobals::<ScenarioFlow>::new(shared_world));

    for _ in 0..config.world_size {
        app.world_mut()
            .spawn((ScenarioWorldFiller, WorldFillerTicks::default()));
    }

    let batch_id = app
        .world_mut()
        .register_system(advance_batch::<ScenarioFlow>);
    app.insert_resource(BatchDriver(batch_id));
    app.add_systems(
        Update,
        (tick_world_filler, run_batch_turn, choose_batch_flows).chain(),
    );

    Ok(measure_app(app, config))
}
