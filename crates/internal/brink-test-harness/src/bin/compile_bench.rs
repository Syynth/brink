//! Compile-time benchmark baseline (#498).
//!
//! Measures, with plain wall-clock medians, the numbers the scripting-substrate
//! phase 0 restructuring (#397/#499) is judged against (spec §6.3):
//!
//! 1. **Cold full-corpus compile** — every `tests/tier{1,2,3}/**/story.ink`
//!    through the public `brink_compiler::compile_path` entry (per-tier
//!    subtotals + grand total).
//! 2. **Studio-scale synthetic project, cold** — a deterministically generated
//!    50-file × 20-knot project compiled from memory via
//!    `brink_compiler::compile`.
//! 3. **Warm recompile after a one-line edit** — a persistent `ProjectDb`
//!    (the same db `brink-lsp` and `brink-ide` keep alive across edits) driven
//!    through the exact stage sequence of `brink-compiler`'s driver, with
//!    `update_file` (per-knot HIR cache) replacing discovery. This is the
//!    incremental layer that exists *today*; phase 0 replaces it.
//! 4. **Per-stage breakdown** for the synthetic project, cold and warm:
//!    parse+HIR / analyze / diagnostics / LIR / codegen, timed at the same
//!    seams the production driver already has.
//!
//! Stability over rigor: medians of N runs (default 5), fixed deterministic
//! inputs, one stable greppable row per metric. Run with:
//!
//! ```sh
//! cargo run --release -p brink-test-harness --bin compile_bench [-- --runs N]
//! ```
#![expect(
    clippy::print_stdout,
    reason = "benchmark harness: the printed table is the product (same stance as the corpus report)"
)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use brink_driver::Driver;
use brink_ir::FileId;

const DEFAULT_RUNS: usize = 5;

/// Synthetic project shape: `SYN_FILES` included files, `SYN_KNOTS` knots each.
const SYN_FILES: usize = 50;
const SYN_KNOTS: usize = 20;
/// The file that receives the one-line warm edit, and the knot inside it.
const EDIT_FILE: usize = 25;
const EDIT_KNOT: usize = 10;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runs = parse_runs()?;
    let root = workspace_root();

    println!(
        "compile_bench | runs={runs} (median) | workspace={}",
        root.display()
    );
    println!("compile_bench | metric | detail | median_ms | runs_ms");

    bench_corpus_cold(&root, runs)?;

    let project = generate_project();
    let stats = project_stats(&project);
    verify_synthetic_compiles(&project)?;

    bench_synthetic_cold(&project, &stats, runs)?;
    bench_synthetic_stages_cold(&project, runs);
    bench_synthetic_warm(&project, runs)?;
    bench_synthetic_warm_query(&project, runs)?;

    Ok(())
}

// ── CLI / environment ────────────────────────────────────────────────

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

/// Workspace root, resolved from this crate's manifest dir
/// (`crates/internal/brink-test-harness` → three levels up).
fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // brink-test-harness
    p.pop(); // internal
    p.pop(); // crates
    p
}

// ── Output ───────────────────────────────────────────────────────────

/// One stable, greppable row. `samples` are per-run wall times in ms.
fn row(metric: &str, detail: &str, samples: &[f64]) {
    let med = median(samples);
    let list = samples
        .iter()
        .map(|ms| format!("{ms:.1}"))
        .collect::<Vec<_>>()
        .join(", ");
    println!("compile_bench | {metric:<34} | {detail:<44} | {med:>10.1} | [{list}]");
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

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

// ── 1. Cold full-corpus compile ──────────────────────────────────────

fn bench_corpus_cold(root: &Path, runs: usize) -> Result<(), String> {
    let tiers = ["tier1", "tier2", "tier3"];
    let mut cases_per_tier: Vec<Vec<PathBuf>> = Vec::new();
    for tier in &tiers {
        let dir = root.join("tests").join(tier);
        let mut cases = Vec::new();
        collect_story_ink(&dir, &mut cases);
        cases.sort();
        if cases.is_empty() {
            return Err(format!("no story.ink cases found under {}", dir.display()));
        }
        cases_per_tier.push(cases);
    }

    // ok/fail per tier is deterministic — count on the first run only.
    let mut ok_fail: Vec<(usize, usize)> = vec![(0, 0); tiers.len()];
    let mut tier_ms: Vec<Vec<f64>> = vec![Vec::new(); tiers.len()];
    let mut total_ms: Vec<f64> = Vec::new();

    for run in 0..runs {
        let mut run_total = 0.0;
        for (t, cases) in cases_per_tier.iter().enumerate() {
            let start = Instant::now();
            let mut ok = 0usize;
            let mut fail = 0usize;
            for case in cases {
                match brink_compiler::compile_path(&case.join("story.ink")) {
                    Ok(_) => ok += 1,
                    Err(_) => fail += 1,
                }
            }
            let elapsed = ms(start);
            tier_ms[t].push(elapsed);
            run_total += elapsed;
            if run == 0 {
                ok_fail[t] = (ok, fail);
            }
        }
        total_ms.push(run_total);
    }

    let mut grand_cases = 0usize;
    let mut grand_ok = 0usize;
    let mut grand_fail = 0usize;
    for (t, tier) in tiers.iter().enumerate() {
        let (ok, fail) = ok_fail[t];
        grand_cases += cases_per_tier[t].len();
        grand_ok += ok;
        grand_fail += fail;
        row(
            &format!("corpus_cold.{tier}"),
            &format!("cases={} ok={ok} fail={fail}", cases_per_tier[t].len()),
            &tier_ms[t],
        );
    }
    row(
        "corpus_cold.total",
        &format!("cases={grand_cases} ok={grand_ok} fail={grand_fail}"),
        &total_ms,
    );
    Ok(())
}

/// Recursively find directories containing `story.ink`, sorted for determinism.
fn collect_story_ink(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    if dir.join("story.ink").is_file() {
        out.push(dir.to_path_buf());
    }
    let mut subdirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|ft| ft.is_dir()))
        .map(|e| e.path())
        .collect();
    subdirs.sort();
    for sub in subdirs {
        collect_story_ink(&sub, out);
    }
}

// ── 2. Studio-scale synthetic project ────────────────────────────────

/// Tiny deterministic PRNG (PCG-style LCG step) with a fixed constant seed.
/// No wall-clock or OS entropy anywhere: the generated project is
/// byte-identical on every run, on every machine.
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

/// Generate the whole synthetic project as `path → source`.
///
/// `main.ink` INCLUDEs `file_00.ink` … `file_49.ink`; each file declares a few
/// VARs and 20 knots cycling through four templates (text-only, var-mutation +
/// divert, choices + gather, inline conditional) so the content mix resembles
/// a real studio project rather than a single degenerate construct.
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

/// Generate one included file. `revision > 0` inserts one extra text line into
/// knot `EDIT_KNOT` of `EDIT_FILE` — the "one-line edit" for warm runs.
fn generate_file(f: usize, revision: u64) -> String {
    // Seed the RNG per file so a one-line edit to file N never changes any
    // other file, and never changes the rest of file N either.
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
                // Text-heavy knot.
                for _ in 0..rng.pick(3, 5) {
                    s.push_str(&sentence(&mut rng, 5, 11));
                    s.push('\n');
                }
                s.push_str("-> DONE\n");
            }
            1 => {
                // Var mutation + divert chain.
                s.push_str(&sentence(&mut rng, 4, 8));
                s.push('\n');
                let _ = writeln!(s, "~ var_f{f:02}_0 = var_f{f:02}_0 + 1");
                let _ = writeln!(s, "The counter reads {{var_f{f:02}_0}} at this point.");
                s.push_str(&next);
                s.push('\n');
            }
            2 => {
                // Choices + gather.
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
                // Inline conditional.
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

struct ProjectStats {
    files: usize,
    knots: usize,
    lines: usize,
    bytes: usize,
}

fn project_stats(project: &BTreeMap<String, String>) -> ProjectStats {
    ProjectStats {
        files: project.len(),
        knots: SYN_FILES * SYN_KNOTS,
        lines: project.values().map(|s| s.lines().count()).sum(),
        bytes: project.values().map(String::len).sum(),
    }
}

fn read_from(project: &BTreeMap<String, String>) -> impl FnMut(&str) -> Result<String, io::Error> {
    move |path: &str| {
        project.get(path).cloned().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("not in project: {path}"))
        })
    }
}

/// The synthetic project must compile clean — a project full of errors would
/// measure the diagnostic path, not the compile path.
fn verify_synthetic_compiles(project: &BTreeMap<String, String>) -> Result<(), String> {
    match brink_compiler::compile("main.ink", read_from(project)) {
        Ok(output) => {
            println!(
                "compile_bench | synthetic.verify | ok warnings={}",
                output.warnings.len()
            );
            Ok(())
        }
        Err(e) => {
            let detail = match &e {
                brink_compiler::CompileError::Diagnostics(diags) => diags
                    .iter()
                    .take(5)
                    .map(|d| format!("{}: {}", d.path, d.message))
                    .collect::<Vec<_>>()
                    .join("; "),
                other => other.to_string(),
            };
            Err(format!("synthetic project failed to compile: {detail}"))
        }
    }
}

fn bench_synthetic_cold(
    project: &BTreeMap<String, String>,
    stats: &ProjectStats,
    runs: usize,
) -> Result<(), String> {
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        brink_compiler::compile("main.ink", read_from(project))
            .map_err(|e| format!("synthetic compile failed mid-benchmark: {e}"))?;
        samples.push(ms(start));
    }
    row(
        "synthetic_cold.compile",
        &format!(
            "files={} knots={} lines={} bytes={}",
            stats.files, stats.knots, stats.lines, stats.bytes
        ),
        &samples,
    );
    Ok(())
}

// ── Stage timings via the production driver's own seams ──────────────

struct StageMs {
    parse_hir: f64,
    analyze: f64,
    diagnostics: f64,
    lir_lower: f64,
    codegen: f64,
}

impl StageMs {
    fn total(&self) -> f64 {
        self.parse_hir + self.analyze + self.diagnostics + self.lir_lower + self.codegen
    }
}

/// Run analysis → diagnostics → LIR → codegen on an already-populated driver,
/// timing each stage. This is exactly the sequence of
/// `brink-compiler/src/driver.rs::compile_lir` after discovery (including the
/// `AnalysisResult` clone the production path performs).
fn staged_back_half(driver: &mut Driver, entry: FileId) -> Result<StageMs, String> {
    let start = Instant::now();
    let analysis = driver.analyze().clone();
    let analyze = ms(start);

    let start = Instant::now();
    let report = driver.collect_diagnostics(&analysis, Some(entry));
    let diagnostics = ms(start);
    if !report.errors.is_empty() {
        return Err(format!(
            "unexpected compile errors during staged run: {} error(s)",
            report.errors.len()
        ));
    }

    let start = Instant::now();
    let (files, file_paths) = driver.lir_inputs(entry);
    let (program, _lir_warnings) = brink_ir::lir::lower_to_program(
        &files,
        &analysis.index,
        &analysis.resolutions,
        &file_paths,
    );
    let lir_lower = ms(start);

    let start = Instant::now();
    let data = brink_codegen_inkb::emit(&program);
    let codegen = ms(start);
    // Keep the output alive to here so codegen cannot be optimized out.
    std::hint::black_box(&data);

    Ok(StageMs {
        parse_hir: 0.0,
        analyze,
        diagnostics,
        lir_lower,
        codegen,
    })
}

fn bench_synthetic_stages_cold(project: &BTreeMap<String, String>, runs: usize) {
    let mut parse_hir = Vec::with_capacity(runs);
    let mut analyze = Vec::with_capacity(runs);
    let mut diagnostics = Vec::with_capacity(runs);
    let mut lir_lower = Vec::with_capacity(runs);
    let mut codegen = Vec::with_capacity(runs);
    let mut totals = Vec::with_capacity(runs);

    for _ in 0..runs {
        let mut driver = Driver::new();

        let start = Instant::now();
        if driver.discover("main.ink", read_from(project)).is_err() {
            // verify_synthetic_compiles already proved this compiles.
            return;
        }
        let discover_ms = ms(start);

        let Some(entry) = driver.db().file_id("main.ink") else {
            return;
        };
        let Ok(mut stages) = staged_back_half(&mut driver, entry) else {
            return;
        };
        stages.parse_hir = discover_ms;

        parse_hir.push(stages.parse_hir);
        analyze.push(stages.analyze);
        diagnostics.push(stages.diagnostics);
        lir_lower.push(stages.lir_lower);
        codegen.push(stages.codegen);
        totals.push(stages.total());
    }

    row(
        "synthetic_cold.stage.parse_hir",
        "discover: read+parse+lower HIR",
        &parse_hir,
    );
    row(
        "synthetic_cold.stage.analyze",
        "cross-file analysis (+result clone)",
        &analyze,
    );
    row(
        "synthetic_cold.stage.diagnostics",
        "collect+partition diagnostics",
        &diagnostics,
    );
    row(
        "synthetic_cold.stage.lir_lower",
        "HIR normalize + LIR lowering",
        &lir_lower,
    );
    row(
        "synthetic_cold.stage.codegen",
        "LIR -> StoryData emit",
        &codegen,
    );
    row("synthetic_cold.stage.total", "sum of stages", &totals);
}

// ── 3. Warm recompile after a one-line edit ──────────────────────────

/// Warm path: one persistent `ProjectDb` — the exact structure `brink-lsp`
/// and `brink-ide`'s `IdeSession` keep alive across edits — re-driven through
/// the production compile stages after `update_file` applies a one-line edit.
///
/// `update_file` is today's entire incremental layer: it re-parses the edited
/// file and re-lowers only its changed knots (green-node identity diff).
/// Everything downstream — analysis, LIR, codegen — recomputes from scratch,
/// which is precisely the situation #498 exists to document and slice C
/// (#460) exists to fix.
///
/// Reported rows:
/// - `update_file` — the per-knot-cached re-parse/re-lower of one file
/// - `reanalyze_ide` — `update_file` + analyze: what
///   `IdeSession::update_and_analyze` / the LSP do per edit
/// - `full_recompile` — all stages through codegen: a warm "rebuild
///   `StoryData`" as a studio play-after-edit would do
fn bench_synthetic_warm(project: &BTreeMap<String, String>, runs: usize) -> Result<(), String> {
    // Populate the persistent db once (untimed) via real discovery.
    let mut driver = Driver::new();
    driver
        .discover("main.ink", read_from(project))
        .map_err(|e| format!("warm setup discovery failed: {e}"))?;
    let entry = driver
        .db()
        .file_id("main.ink")
        .ok_or_else(|| "warm setup: main.ink missing after discovery".to_string())?;
    let mut db = driver.into_db();

    let edit_path = format!("file_{EDIT_FILE:02}.ink");
    let mut update = Vec::with_capacity(runs);
    let mut reanalyze = Vec::with_capacity(runs);
    let mut analyze = Vec::with_capacity(runs);
    let mut diagnostics = Vec::with_capacity(runs);
    let mut lir_lower = Vec::with_capacity(runs);
    let mut codegen = Vec::with_capacity(runs);
    let mut full = Vec::with_capacity(runs);

    let mut revision: u64 = 0;
    for _ in 0..runs {
        // Each iteration applies a *different* one-line edit so the timed
        // update always sees changed content.
        revision += 1;
        let edited = generate_file(EDIT_FILE, revision);
        let mut d = Driver::from_db(db);

        let start = Instant::now();
        d.db_mut().update_file(&edit_path, edited);
        let update_ms = ms(start);

        let stages = staged_back_half(&mut d, entry)?;

        update.push(update_ms);
        analyze.push(stages.analyze);
        diagnostics.push(stages.diagnostics);
        lir_lower.push(stages.lir_lower);
        codegen.push(stages.codegen);
        reanalyze.push(update_ms + stages.analyze);
        full.push(update_ms + stages.total());

        db = d.into_db();
    }

    row(
        "synthetic_warm.update_file",
        "1-line edit, per-knot HIR cache",
        &update,
    );
    row(
        "synthetic_warm.reanalyze_ide",
        "update_file + analyze (IdeSession path)",
        &reanalyze,
    );
    row(
        "synthetic_warm.stage.analyze",
        "cross-file analysis (+result clone)",
        &analyze,
    );
    row(
        "synthetic_warm.stage.diagnostics",
        "collect+partition diagnostics",
        &diagnostics,
    );
    row(
        "synthetic_warm.stage.lir_lower",
        "HIR normalize + LIR lowering",
        &lir_lower,
    );
    row(
        "synthetic_warm.stage.codegen",
        "LIR -> StoryData emit",
        &codegen,
    );
    row(
        "synthetic_warm.full_recompile",
        "update_file .. codegen (StoryData)",
        &full,
    );
    Ok(())
}

/// Warm path through the **query graph**: one persistent `ProjectDb`, and
/// after each one-line edit the whole rebuild is a single memoized
/// `story_data()` pull. Where `synthetic_warm.full_recompile` measures the
/// hand-driven stage sequence (always recomputing LIR + codegen from
/// scratch), this row measures what the salsa pipeline actually redoes —
/// the number slice C (#460) targets.
fn bench_synthetic_warm_query(
    project: &BTreeMap<String, String>,
    runs: usize,
) -> Result<(), String> {
    let mut driver = Driver::new();
    driver
        .discover("main.ink", read_from(project))
        .map_err(|e| format!("warm query setup discovery failed: {e}"))?;
    let mut db = driver.into_db();
    db.set_entry("main.ink")
        .ok_or_else(|| "warm query setup: main.ink missing after discovery".to_string())?;

    // Untimed warm-up pull so the first timed iteration measures an
    // incremental recompute, not the cold population of every memo.
    let warmup = db
        .story_data()
        .ok_or_else(|| "warm query setup: no entry".to_string())?;
    if warmup.story.is_none() {
        return Err("warm query setup: synthetic project failed to compile".to_string());
    }

    let edit_path = format!("file_{EDIT_FILE:02}.ink");
    let mut samples = Vec::with_capacity(runs);
    let mut revision: u64 = 1000; // distinct from bench_synthetic_warm's edits
    for _ in 0..runs {
        revision += 1;
        let edited = generate_file(EDIT_FILE, revision);

        let start = Instant::now();
        db.update_file(&edit_path, edited);
        let product = db
            .story_data()
            .ok_or_else(|| "warm query pull: no entry".to_string())?;
        if product.story.is_none() {
            return Err("warm query pull: compile failed mid-benchmark".to_string());
        }
        std::hint::black_box(product);
        samples.push(ms(start));
    }

    row(
        "synthetic_warm.story_data_pull",
        "update_file + story_data() query pull",
        &samples,
    );
    Ok(())
}
