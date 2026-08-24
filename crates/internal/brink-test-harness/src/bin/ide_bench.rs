//! Editor-road latency bench (measure-first ruling, `docs/decision-log.md`
//! 2026-08-24).
//!
//! `compile_bench` (#498) profiles the **db road** (`ProjectDb` query
//! pulls) and `editor_session_bench` (#529) its memory growth — but the
//! studio editor's per-keystroke path is the **off-db analyze road**:
//! `IdeSession::update_and_analyze` = update → `snapshot()` (clones every
//! file's `HirFile` + `SymbolManifest` + the module map) → whole-project
//! `IdeSnapshot::analyze` → `apply_analysis` (wipes the projection cache).
//! Nothing measured that road natively until this bin. It answers, with
//! numbers, the desktop-perf hypotheses:
//!
//! 1. **Startup is O(N²)**: `ProjectSession.initialize` calls
//!    `updateFile` per file, and each one runs a full-project analysis —
//!    the `ide_init.analyze_each.*` curve vs the `ide_init.analyze_once.*`
//!    counterfactual (seed every file with `update_source`, analyze once).
//! 2. **Per-keystroke cost at studio scale**: one-line edit →
//!    `update_and_analyze`, median + per-phase split.
//! 3. **The #2885 revision-stamp question**: `IdeSession::compile` writes
//!    `set_analysis_options` unconditionally; if that stamps the salsa
//!    revision, a repeat compile with ZERO edits is priced like the first.
//!    `ide_compile.first` vs `ide_compile.repeat_no_edit` is the direct
//!    experiment (native twin of `EditorSession::perf_compile_probe`).
//! 4. **Per-compile query pulls**: the story-graph build the studio runs
//!    after every successful compile.
//!
//! The synthetic project intentionally mirrors `compile_bench`'s shape
//! (50 files × 20 knots, four content templates, fixed-seed LCG — see that
//! bin's generator) so numbers sit next to
//! `docs/compile-time-profile-findings.md`; it is re-declared here rather
//! than shared because the two bins version their fixtures independently
//! (a shape tweak for one must not silently move the other's baseline).
//!
//! ```sh
//! cargo run --release -p brink-test-harness --bin ide_bench [-- --runs N]
//! ```
#![expect(
    clippy::print_stdout,
    reason = "benchmark harness: the printed table is the product (same stance as compile_bench)"
)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::Instant;

use brink_ide::session::IdeSession;

const DEFAULT_RUNS: usize = 5;

/// Studio-scale shape, mirroring `compile_bench`.
const SYN_FILES: usize = 50;
const SYN_KNOTS: usize = 20;
/// The file receiving the one-line warm edit, and the knot inside it.
const EDIT_FILE: usize = 25;
const EDIT_KNOT: usize = 10;

/// The startup-curve points (`<= SYN_FILES` each).
const INIT_CURVE: [usize; 3] = [10, 25, 50];

/// Knots in the large-file variant (`large.ink`, ~8k lines at ~8 lines per
/// knot) — the reported symptom is typing/scrolling in one big file, so the
/// bench needs the file-size axis, not just the file-count axis. Mirrors
/// the `?fixture=perf` playground fixture's `large.ink`
/// (`packages/brink-studio/src/perf-fixture.ts`).
const LARGE_KNOTS: usize = 900;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runs = parse_runs()?;

    println!("ide_bench | runs={runs} (median)");
    println!("ide_bench | metric | detail | median_ms | runs_ms");

    let project = generate_project();
    verify_analyzes_clean(&project)?;

    bench_init_curve(&project);
    bench_keystroke(&project, runs);
    bench_keystroke_phases(&project, runs);
    bench_compile_repeat(&project, runs)?;
    bench_story_graph(&project, runs);

    // The file-SIZE axis: same project plus one ~8k-line file, keystrokes
    // landing in the big file. The reported desktop symptom lives here.
    let large_project = add_large_file(project.clone(), 0);
    bench_keystroke_large(&large_project, runs);
    bench_compile_repeat_large(&large_project, runs)?;

    Ok(())
}

// ── Benches ──────────────────────────────────────────────────────────

/// Startup: the `ProjectSession.initialize` shape (`update_and_analyze`
/// per file — analysis N times over a growing project) vs the batched
/// counterfactual (`update_source` per file, one analysis at the end).
fn bench_init_curve(project: &BTreeMap<String, String>) {
    for n in INIT_CURVE {
        let subset: Vec<(&String, &String)> = project.iter().take(n).collect();

        let start = Instant::now();
        let mut session = IdeSession::new();
        for (path, source) in &subset {
            session.update_and_analyze(path, (*source).clone());
        }
        let each = ms(start);

        let start = Instant::now();
        let mut session = IdeSession::new();
        for (path, source) in &subset {
            session.update_source(path, (*source).clone());
        }
        let result = session.snapshot().analyze();
        session.apply_analysis(result);
        let once = ms(start);

        row(
            &format!("ide_init.analyze_each.files_{n:02}"),
            "update_and_analyze per file (initialize() shape)",
            &[each],
        );
        row(
            &format!("ide_init.analyze_once.files_{n:02}"),
            "update_source per file + one analysis (counterfactual)",
            &[once],
        );
    }
}

/// One keystroke at studio scale: a one-line edit to a single knot, then
/// the full `update_and_analyze` the editor pays synchronously.
fn bench_keystroke(project: &BTreeMap<String, String>, runs: usize) {
    let mut session = seeded_session(project);
    let path = edit_path();
    let mut samples = Vec::with_capacity(runs);
    for revision in 1..=runs as u64 {
        let edited = generate_file(EDIT_FILE, revision);
        let start = Instant::now();
        session.update_and_analyze(&path, edited);
        samples.push(ms(start));
    }
    row(
        "ide_keystroke.update_and_analyze",
        &format!(
            "one-line edit, files={SYN_FILES} knots={}",
            SYN_FILES * SYN_KNOTS
        ),
        &samples,
    );
}

/// The same keystroke, split into the four phases `update_and_analyze`
/// composes — the same decomposition `crates/brink-web`'s perf counters
/// report in-browser (`ide.updateSource` / `ide.snapshotClone` /
/// `ide.analyze` / `ide.applyAnalysis`).
fn bench_keystroke_phases(project: &BTreeMap<String, String>, runs: usize) {
    let mut session = seeded_session(project);
    let path = edit_path();
    let mut update = Vec::with_capacity(runs);
    let mut snapshot = Vec::with_capacity(runs);
    let mut analyze = Vec::with_capacity(runs);
    let mut apply = Vec::with_capacity(runs);
    for revision in 1..=runs as u64 {
        // Offset revisions past bench_keystroke's so every edit is fresh.
        let edited = generate_file(EDIT_FILE, 1000 + revision);

        let start = Instant::now();
        session.update_source(&path, edited);
        update.push(ms(start));

        let start = Instant::now();
        let snap = session.snapshot();
        snapshot.push(ms(start));

        let start = Instant::now();
        let result = snap.analyze();
        analyze.push(ms(start));

        let start = Instant::now();
        session.apply_analysis(result);
        apply.push(ms(start));
    }
    row(
        "ide_keystroke.phase.update_source",
        "splice + relower edited file",
        &update,
    );
    row(
        "ide_keystroke.phase.snapshot",
        "clone every HirFile + manifest + module map",
        &snapshot,
    );
    row(
        "ide_keystroke.phase.analyze",
        "whole-project off-db analyze_with_modules",
        &analyze,
    );
    row(
        "ide_keystroke.phase.apply",
        "store result, wipe projection cache",
        &apply,
    );
}

/// `compile()` twice with zero edits between (#2885): warm memoization
/// would make the repeat near-free; an unconditional revision stamp prices
/// it like the first.
fn bench_compile_repeat(project: &BTreeMap<String, String>, runs: usize) -> Result<(), String> {
    let mut session = seeded_session(project);
    let options = session.analysis_options();

    let start = Instant::now();
    session
        .compile("main.ink", &options)
        .map_err(|e| format!("first compile failed: {e}"))?;
    let first = ms(start);

    let mut repeats = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        session
            .compile("main.ink", &options)
            .map_err(|e| format!("repeat compile failed: {e}"))?;
        repeats.push(ms(start));
    }
    row("ide_compile.first", "cold story_data pull", &[first]);
    row(
        "ide_compile.repeat_no_edit",
        "zero edits between; warm iff salsa memos survive compile()",
        &repeats,
    );
    Ok(())
}

/// The story-graph build the studio triggers after every successful
/// compile (`EditorSession::story_graph` minus DTO/JSON conversion).
fn bench_story_graph(project: &BTreeMap<String, String>, runs: usize) {
    let session = seeded_session(project);
    // The graph needs an analysis result; seeding used update_and_analyze,
    // so one is present.
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let Some(analysis) = session.analysis() else {
            row(
                "ide_query.story_graph",
                "SKIPPED: no analysis available",
                &[],
            );
            return;
        };
        let db = session.db();
        let start = Instant::now();
        let files: Vec<(brink_ir::FileId, &brink_ir::HirFile)> = db
            .file_ids()
            .filter_map(|id| db.hir(id).map(|hir| (id, hir)))
            .collect();
        let graph = brink_ide::story_graph::story_graph(analysis, &files);
        samples.push(ms(start));
        // Keep the graph alive through the timer so the build isn't
        // optimized away.
        std::hint::black_box(&graph);
    }
    row(
        "ide_query.story_graph",
        "whole-project graph build",
        &samples,
    );
}

/// One keystroke landing in the ~8k-line file, phases split — the direct
/// native twin of the reported symptom. `update_source` here re-lowers the
/// whole big file per keystroke; `snapshot`/`analyze` show how the
/// project-wide phases scale with one dominant file.
fn bench_keystroke_large(project: &BTreeMap<String, String>, runs: usize) {
    let mut session = seeded_session(project);
    let path = "large.ink".to_owned();
    let large_lines = project.get(&path).map_or(0, |s| s.lines().count());
    let mut total = Vec::with_capacity(runs);
    let mut update = Vec::with_capacity(runs);
    let mut snapshot = Vec::with_capacity(runs);
    let mut analyze = Vec::with_capacity(runs);
    let mut apply = Vec::with_capacity(runs);
    for revision in 1..=runs as u64 {
        let edited = generate_large_file(revision);

        let start = Instant::now();
        session.update_source(&path, edited);
        update.push(ms(start));

        let start = Instant::now();
        let snap = session.snapshot();
        snapshot.push(ms(start));

        let start = Instant::now();
        let result = snap.analyze();
        analyze.push(ms(start));

        let start = Instant::now();
        session.apply_analysis(result);
        apply.push(ms(start));

        total.push(
            update[update.len() - 1]
                + snapshot[snapshot.len() - 1]
                + analyze[analyze.len() - 1]
                + apply[apply.len() - 1],
        );
    }
    let detail = format!("one-line edit in large.ink ({large_lines} lines)");
    row("ide_large.update_and_analyze", &detail, &total);
    row(
        "ide_large.phase.update_source",
        "splice + relower the ~8k-line file",
        &update,
    );
    row(
        "ide_large.phase.snapshot",
        "clone every HirFile + manifest + module map",
        &snapshot,
    );
    row(
        "ide_large.phase.analyze",
        "whole-project off-db analyze_with_modules",
        &analyze,
    );
    row(
        "ide_large.phase.apply",
        "store result, wipe projection cache",
        &apply,
    );
}

/// Compile cost with the big file in the project — first vs repeat.
fn bench_compile_repeat_large(
    project: &BTreeMap<String, String>,
    runs: usize,
) -> Result<(), String> {
    let mut session = seeded_session(project);
    let options = session.analysis_options();

    let start = Instant::now();
    session
        .compile("main.ink", &options)
        .map_err(|e| format!("large first compile failed: {e}"))?;
    let first = ms(start);

    let mut repeats = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        session
            .compile("main.ink", &options)
            .map_err(|e| format!("large repeat compile failed: {e}"))?;
        repeats.push(ms(start));
    }
    row(
        "ide_large.compile.first",
        "cold story_data pull, large.ink included",
        &[first],
    );
    row(
        "ide_large.compile.repeat_no_edit",
        "zero edits between",
        &repeats,
    );
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────

/// A session seeded the way the studio actually seeds one (per-file
/// `update_and_analyze`), left warm.
fn seeded_session(project: &BTreeMap<String, String>) -> IdeSession {
    let mut session = IdeSession::new();
    for (path, source) in project {
        session.update_and_analyze(path, source.clone());
    }
    session
}

fn edit_path() -> String {
    format!("file_{EDIT_FILE:02}.ink")
}

fn verify_analyzes_clean(project: &BTreeMap<String, String>) -> Result<(), String> {
    let mut session = IdeSession::new();
    for (path, source) in project {
        session.update_source(path, source.clone());
    }
    let result = session.snapshot().analyze();
    let errors = result
        .diagnostics
        .iter()
        .filter(|d| d.code.severity() == brink_ir::hir::Severity::Error)
        .count();
    if errors == 0 {
        println!(
            "ide_bench | synthetic.verify | ok diagnostics={}",
            result.diagnostics.len()
        );
        Ok(())
    } else {
        Err(format!("synthetic project has {errors} analysis errors"))
    }
}

fn parse_runs() -> Result<usize, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => Ok(DEFAULT_RUNS),
        [flag, value] if flag == "--runs" => value
            .parse::<usize>()
            .map_err(|e| format!("--runs {value}: {e}"))
            .and_then(|n| {
                if n == 0 {
                    Err("--runs must be >= 1".to_string())
                } else {
                    Ok(n)
                }
            }),
        other => Err(format!(
            "unsupported arguments: {} (only --runs N is supported)",
            other.join(" ")
        )),
    }
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn row(metric: &str, detail: &str, samples: &[f64]) {
    let med = median(samples);
    let list = samples
        .iter()
        .map(|ms| format!("{ms:.1}"))
        .collect::<Vec<_>>()
        .join(", ");
    println!("ide_bench | {metric:<38} | {detail:<52} | {med:>10.1} | [{list}]");
}

fn median(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        f64::midpoint(sorted[mid - 1], sorted[mid])
    } else {
        sorted[mid]
    }
}

/// Add `large.ink` (~8k lines) to a copy of the synthetic project and
/// INCLUDE it from `main.ink`.
fn add_large_file(
    mut project: BTreeMap<String, String>,
    revision: u64,
) -> BTreeMap<String, String> {
    project.insert("large.ink".to_owned(), generate_large_file(revision));
    if let Some(main) = project.get_mut("main.ink") {
        let insert_at = main.find("VAR main_counter").unwrap_or(0);
        main.insert_str(insert_at, "INCLUDE large.ink\n");
    }
    project
}

/// The large-file symptom reproducer: one file, ~8k lines, same four
/// content templates. `revision > 0` inserts one extra line into a middle
/// knot — the warm-edit lever.
fn generate_large_file(revision: u64) -> String {
    let mut rng = Lcg::new();
    let mut s = String::from("// large.ink — the large-file symptom reproducer (~8k lines).\n");
    s.push_str("VAR large_0 = 0\nVAR large_1 = 5\n\n");
    for k in 0..LARGE_KNOTS {
        let _ = writeln!(s, "=== big_{k:03} ===");
        if revision > 0 && k == LARGE_KNOTS / 2 {
            let _ = writeln!(s, "A revised line, edit number {revision}.");
        }
        let next = if k + 1 < LARGE_KNOTS {
            format!("-> big_{:03}", k + 1)
        } else {
            "-> DONE".to_string()
        };
        match k % 4 {
            0 => {
                for _ in 0..rng.pick(3, 5) {
                    s.push_str(&sentence(&mut rng, 5, 11));
                    s.push('\n');
                }
                s.push_str("-> DONE\n");
            }
            1 => {
                s.push_str(&sentence(&mut rng, 4, 8));
                s.push('\n');
                s.push_str("~ large_0 = large_0 + 1\n");
                s.push_str("The counter reads {large_0} at this point.\n");
                s.push_str(&next);
                s.push('\n');
            }
            2 => {
                s.push_str(&sentence(&mut rng, 4, 8));
                s.push('\n');
                let _ = writeln!(s, "* [{}]", sentence(&mut rng, 2, 4));
                let _ = writeln!(s, "    {}", sentence(&mut rng, 4, 8));
                let _ = writeln!(s, "* [{}]", sentence(&mut rng, 2, 4));
                let _ = writeln!(s, "    {} # aside", sentence(&mut rng, 4, 8));
                let _ = writeln!(s, "- {}", sentence(&mut rng, 3, 6));
                s.push_str(&next);
                s.push('\n');
            }
            _ => {
                let _ = writeln!(
                    s,
                    "{{large_1 > 4: {}|{}}}",
                    sentence(&mut rng, 3, 5),
                    sentence(&mut rng, 3, 5)
                );
                s.push_str(&next);
                s.push('\n');
            }
        }
        s.push('\n');
    }
    s
}

// ── Synthetic project (mirrors compile_bench's generator — see header) ──

struct Lcg(u64);

impl Lcg {
    fn new() -> Self {
        Self(0x5EED_0498_CAFE_F00D)
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn pick(&mut self, lo: usize, hi: usize) -> usize {
        lo + usize::try_from(self.next()).unwrap_or(0) % (hi - lo + 1)
    }
}

const WORDS: [&str; 24] = [
    "lantern", "harbor", "signal", "vault", "ember", "cipher", "meadow", "static", "orchard",
    "beacon", "drift", "hollow", "ledger", "murmur", "quarry", "relay", "sable", "tundra",
    "vesper", "wharf", "zenith", "gable", "isthmus", "keel",
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

fn generate_project() -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    for f in 0..SYN_FILES {
        files.insert(format!("file_{f:02}.ink"), generate_file(f, 0));
    }

    let mut main = String::from("// Synthetic studio-scale project — generated, deterministic.\n");
    for f in 0..SYN_FILES {
        let _ = writeln!(main, "INCLUDE file_{f:02}.ink");
    }
    main.push_str("VAR main_counter = 0\n");
    main.push_str("The opening line of the synthetic project. # generated\n");
    main.push_str("~ main_counter = main_counter + 1\n");
    main.push_str("-> k00_00\n");
    files.insert("main.ink".to_string(), main);
    files
}

fn generate_file(f: usize, revision: u64) -> String {
    let mut rng = Lcg::new();
    for _ in 0..=f {
        rng.next();
    }

    let mut s = format!("// Generated file {f:02}.\n");
    for v in 0..3 {
        let _ = writeln!(s, "VAR var_f{f:02}_{v} = {}", rng.pick(0, 9));
    }
    s.push('\n');

    for k in 0..SYN_KNOTS {
        let _ = writeln!(s, "=== k{f:02}_{k:02} ===");
        if revision > 0 && f == EDIT_FILE && k == EDIT_KNOT {
            let _ = writeln!(s, "A revised line, edit number {revision}.");
        }
        let next = if k + 1 < SYN_KNOTS {
            format!("-> k{f:02}_{:02}", k + 1)
        } else {
            "-> DONE".to_string()
        };
        match (f * SYN_KNOTS + k) % 4 {
            0 => {
                for _ in 0..rng.pick(3, 5) {
                    s.push_str(&sentence(&mut rng, 5, 11));
                    s.push('\n');
                }
                s.push_str("-> DONE\n");
            }
            1 => {
                s.push_str(&sentence(&mut rng, 4, 8));
                s.push('\n');
                let _ = writeln!(s, "~ var_f{f:02}_0 = var_f{f:02}_0 + 1");
                let _ = writeln!(s, "The counter reads {{var_f{f:02}_0}} at this point.");
                s.push_str(&next);
                s.push('\n');
            }
            2 => {
                s.push_str(&sentence(&mut rng, 4, 8));
                s.push('\n');
                let _ = writeln!(s, "* [{}]", sentence(&mut rng, 2, 4));
                let _ = writeln!(s, "    {}", sentence(&mut rng, 4, 8));
                let _ = writeln!(s, "* [{}]", sentence(&mut rng, 2, 4));
                let _ = writeln!(s, "    {} # aside", sentence(&mut rng, 4, 8));
                let _ = writeln!(s, "- {}", sentence(&mut rng, 3, 6));
                s.push_str(&next);
                s.push('\n');
            }
            _ => {
                let _ = writeln!(
                    s,
                    "{{var_f{f:02}_1 > 4: {}|{}}}",
                    sentence(&mut rng, 3, 5),
                    sentence(&mut rng, 3, 5)
                );
                s.push_str(&next);
                s.push('\n');
            }
        }
        s.push('\n');
    }
    s
}
