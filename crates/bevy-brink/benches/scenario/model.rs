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
use bevy::prelude::{
    App, Commands, Component, Entity, IntoScheduleConfigs as _, Query, Res, ResMut, Resource,
    Update, With, Without,
};
use bevy_brink::{
    Advance, BrinkContext, BrinkFlow, BrinkGlobals, FallbackHandler, FlowInstance, Line, Program,
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
pub fn generate_story(turn_weight: TurnWeight, seed: u64) -> String {
    let mut rng = Lcg::new(seed);
    let mut s = String::from("// Synthetic scenario story — generated, deterministic (#900).\n");

    let needs_counter = matches!(turn_weight, TurnWeight::Medium | TurnWeight::Heavy);
    if needs_counter {
        s.push_str("VAR turn_count = 0\n");
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

#[expect(
    clippy::too_many_lines,
    reason = "a single linear setup -> spawn -> run-frames -> measure pipeline; splitting it would scatter closely related local state across artificial helper boundaries for no clarity gain"
)]
pub fn run_scenario(config: &ScenarioConfig) -> Result<ScenarioResult, Box<dyn std::error::Error>> {
    if config.flow_count == 0 {
        return Err(Box::new(ScenarioError::NoFlows));
    }

    let source = generate_story(config.turn_weight, config.seed);
    let output = brink_compiler::compile("scenario.ink", |path| {
        if path == "scenario.ink" {
            Ok(source.clone())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("unexpected include: {path}"),
            ))
        }
    })?;
    let (program, line_tables) = brink_runtime::link(&output.data)?;

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

    Ok(ScenarioResult {
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
    })
}
