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
//! 5. **Warm recompile under `types = strict`** (issue #632 / FG-3): the
//!    same one-line-edit warm loop as (3), but with `dialect = brink` +
//!    `types = strict` set on the persistent db before the edit loop —
//!    TM-3 (#619, PR #656) wired `finish_analysis` to run `strict::check` +
//!    `annotations::mismatches` through the memoized `type_inference_query`
//!    under this policy, so this is FG-3's first slice with a *measurable*
//!    warm-path consumer: `analysis_query`'s decomposition (per-file
//!    `validate`/`dialect_gate`/annotation-content contributors, split off from the
//!    whole-project passes) should keep this row from scaling with project
//!    size the way the pre-FG-3 bundled `analysis_query` did. Reported
//!    alongside (3)'s gradual-mode numbers as the before/after comparison
//!    the issue asks for — same project shape, same edit, only the
//!    `AnalysisOptions` differ.
//! 6. **`ProjectDb`-driven incremental recompile** (issue #838, FG-4
//!    follow-up): (3)/(5) drive `brink_driver::Driver`, which calls
//!    `brink_ir::lir::lower_to_program` directly — the legacy one-shot path
//!    that bypasses the salsa `ProjectDb` per-knot chunk memos FG-4d (#837)
//!    added. This row drives `brink_db::ProjectDb` directly instead (open
//!    project → pull `story_data()` → single-knot body edit → re-pull),
//!    exercising `story_data_query` → `lir_lowering_query`'s per-knot
//!    `lir_knot_chunk_query` memos for real. Fixed at
//!    [`PROJECTDB_WARM_RUNS`] runs regardless of `--runs` — the "10-run
//!    protocol" #672 lane E calls for so this row is conclusive on its own.
//! 7. **Diagnostic-heavy synthetic project** (issue #663, Wave-22
//!    reconciliation follow-up to FG-3 / PR #661): (2)-(6)'s project has no
//!    brink-extension syntax, no annotation content, and almost no
//!    diagnostics, so `validate`/`dialect_gate`/`annotations::check`'s
//!    per-file contributor split (FG-3) is only proven by pointer-identity
//!    tests, never by a wall-clock number a human can read off this bench.
//!    [`generate_diag_project`] is the same 50-file × 20-knot shape, but
//!    knot templates cycle through the original four plus three new ones:
//!    a `~ { … }` block exercising every `dialect_gate`-recognized
//!    expression form at once (`#[…]`/`#{…}` sigils, postfix indexing, a
//!    struct construction literal, field access, an unresolved stdlib
//!    call), an all-fallback choice set (`validate`'s E034), and content
//!    unreachable after a divert (`validate`'s E033) — plus, every
//!    [`DIAG_FN_STRIDE`]'th file, one annotated `function` knot
//!    (`int`/`string`/`list<L>`/`fn(T…): R` params + return, all
//!    *recognized* names) for `annotations::check`'s content-resolution
//!    walk. E033/E034 are warnings (never block compilation); the
//!    extension constructs and annotations are only `E051` errors under
//!    `StrictInk` — so this project compiles clean under `Dialect::Brink`
//!    (rows 7a/7b/7c below) and is reused, unmodified, as the "both
//!    dialects" comparison under `Dialect::StrictInk` (row 7d) — that leg
//!    is necessarily analyze-only (no LIR/codegen): a strict-ink project
//!    with brink-extension content is *supposed* to fail compilation there,
//!    which is exactly the point (real `E051`s, not zero). Row 7e is a
//!    small dedicated fixture with deliberately unrecognized annotation
//!    names, kept separate from 7a-7d's project because it must produce
//!    real `E061`s (an error), which would otherwise break that project's
//!    "compiles clean" invariant.
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

use brink_analyzer::{AnalysisOptions, Dialect, TypePolicy};
use brink_db::ProjectDb;
use brink_driver::Driver;
use brink_ir::FileId;

const DEFAULT_RUNS: usize = 5;

/// Synthetic project shape: `SYN_FILES` included files, `SYN_KNOTS` knots each.
const SYN_FILES: usize = 50;
const SYN_KNOTS: usize = 20;
/// The file that receives the one-line warm edit, and the knot inside it.
const EDIT_FILE: usize = 25;
const EDIT_KNOT: usize = 10;

/// Run count for [`bench_synthetic_warm_projectdb`] (#838), fixed
/// independent of `--runs` — the "10-run protocol" #672 lane E calls for
/// ("... so rows ... get conclusive answers") applied to this new row.
const PROJECTDB_WARM_RUNS: usize = 10;

/// Every `DIAG_FN_STRIDE`th file in [`generate_diag_project`] gets one extra
/// annotated `function` knot appended — dense enough for `annotations::check`
/// (~10 functions × 4 annotated slots each across the 50-file project) to be
/// non-trivial, sparse enough that the project stays dominated by ordinary
/// content rather than becoming a degenerate all-annotations fixture.
const DIAG_FN_STRIDE: usize = 5;

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
    bench_synthetic_warm_strict(&project, runs)?;
    bench_synthetic_warm_projectdb(&project)?;

    let diag_project = generate_diag_project();
    verify_diag_compiles(&diag_project)?;
    bench_diag_cold(&diag_project, runs)?;
    bench_diag_warm(&diag_project, runs)?;
    bench_diag_warm_strict(&diag_project, runs)?;
    bench_diag_dialect_gate_strict(&diag_project, runs)?;
    bench_diag_unknown_annotations(runs)?;

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

/// Recursively find directories containing `story.ink`, via the shared
/// [`brink_source_tree::Walk`] (issue #1433) — deterministic order, and the
/// ignored-directory policy applied by construction rather than by a
/// hand-written recursion that has to remember it. The caller sorts.
fn collect_story_ink(dir: &Path, out: &mut Vec<PathBuf>) {
    if dir.join("story.ink").is_file() {
        out.push(dir.to_path_buf());
    }
    for entry in brink_source_tree::Walk::new(dir).flatten() {
        if entry.is_dir() && entry.path().join("story.ink").is_file() {
            out.push(entry.into_path());
        }
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
    let Some(program) = program else {
        return Err(
            "unexpected LIR lowering failure during staged run: residual-extension backstop \
             fired (E053) — a T1b brink-extension HIR node reached LIR lowering"
                .to_string(),
        );
    };
    let lir_lower = ms(start);

    let start = Instant::now();
    let data = brink_codegen_inkb::emit(&program)
        .map_err(|e| format!("unexpected codegen failure during staged run: {e}"))?;
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

// ── 5. Warm recompile under types = strict (issue #632 / FG-3) ────────

/// Same warm one-line-edit loop as [`bench_synthetic_warm`], but with
/// `dialect = brink` + `types = strict` set on the persistent db before the
/// edit loop starts — the before/after comparison the issue requires,
/// isolating the `AnalysisOptions` policy as the only variable (same
/// synthetic project, same edit, same stage seams). The generated project
/// has no knot params/temps/function-return annotations, so strict mode's
/// escape checks (`E065`/`E066`) find nothing to flag and the project still
/// compiles clean — this measures the *cost* of running the strict path,
/// not a diagnostics-heavy detour.
///
/// TM-3 (#619, PR #656) is the "real consumer" this issue names: under
/// `types = strict`, `finish_analysis` runs `strict::check` +
/// `annotations::mismatches` via the memoized `type_inference_query`, so
/// `reanalyze_ide_strict` is the row that shows whether the FG-3
/// decomposition (per-file `validate`/`dialect_gate`/annotation-content
/// contributors, split off from the whole-project passes `analysis_query`
/// used to bundle them with) keeps a one-file edit's cost from scaling with
/// the other `SYN_FILES - 1` untouched files.
fn bench_synthetic_warm_strict(
    project: &BTreeMap<String, String>,
    runs: usize,
) -> Result<(), String> {
    let mut driver = Driver::new();
    driver
        .discover("main.ink", read_from(project))
        .map_err(|e| format!("warm setup discovery failed: {e}"))?;
    let entry = driver
        .db()
        .file_id("main.ink")
        .ok_or_else(|| "warm setup: main.ink missing after discovery".to_string())?;
    driver.set_analysis_options(AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Strict),
        ..AnalysisOptions::default()
    });
    let mut db = driver.into_db();

    let edit_path = format!("file_{EDIT_FILE:02}.ink");
    let mut update = Vec::with_capacity(runs);
    let mut reanalyze = Vec::with_capacity(runs);
    let mut analyze = Vec::with_capacity(runs);
    let mut diagnostics = Vec::with_capacity(runs);
    let mut full = Vec::with_capacity(runs);

    // Offset the revision counter from `bench_synthetic_warm`'s so both
    // benches never happen to compare identical edit content.
    let mut revision: u64 = 1000;
    for _ in 0..runs {
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
        reanalyze.push(update_ms + stages.analyze);
        full.push(update_ms + stages.total());

        db = d.into_db();
    }

    row(
        "synthetic_warm_strict.update_file",
        "1-line edit, per-knot HIR cache",
        &update,
    );
    row(
        "synthetic_warm_strict.reanalyze_ide",
        "update_file + analyze, types=strict",
        &reanalyze,
    );
    row(
        "synthetic_warm_strict.stage.analyze",
        "cross-file analysis incl. strict::check",
        &analyze,
    );
    row(
        "synthetic_warm_strict.stage.diagnostics",
        "collect+partition diagnostics",
        &diagnostics,
    );
    row(
        "synthetic_warm_strict.full_recompile",
        "update_file .. codegen (StoryData)",
        &full,
    );
    Ok(())
}

// ── 6. ProjectDb-driven incremental recompile (issue #838) ────────────

/// Drives `brink_db::ProjectDb` directly — no `brink_driver::Driver` in the
/// loop — so the warm re-pull actually exercises the salsa incremental
/// layer FG-4d (#837) added: `story_data()` → `story_data_query` →
/// `lir_lowering_query`'s per-`DefinitionId` chunk memos
/// (`lir_knot_chunk_query`) + link, instead of (3)/(5)'s
/// `brink_ir::lir::lower_to_program` one-shot call.
///
/// Protocol: open the project (`set_file` every source, `set_entry`), pull
/// `story_data()` once untimed (the "open project" cold compile — not part
/// of the reported numbers), then loop [`PROJECTDB_WARM_RUNS`] times over
/// {single-knot body edit via `update_file`, re-pull via `story_data()`}.
/// Each edit changes only [`EDIT_KNOT`] of [`EDIT_FILE`], same shape as (3)'s
/// one-line edit, so this is a direct comparison point for the phase-0
/// (#498) `synthetic_warm.full_recompile` baseline and (3)'s
/// `synthetic_warm.full_recompile` row above — same project, same edit,
/// only the entry point (`ProjectDb::story_data()` vs `Driver` +
/// `lower_to_program`) differs.
///
/// Reported rows:
/// - `update_file` — the per-knot-cached re-parse/re-lower of one file
///   (same cost (3) already pays; reported again here so this bench is
///   self-contained and doesn't require cross-referencing (3)'s row)
/// - `story_data_repull` — the number issue #838 exists to produce: a warm
///   `story_data()` pull after the edit, running through the per-knot chunk
///   memos. This is the FG-4d win (or its absence) made visible.
/// - `full` — `update_file` + `story_data_repull`, the apples-to-apples
///   comparison against #498's and (3)'s `full_recompile` rows.
fn bench_synthetic_warm_projectdb(project: &BTreeMap<String, String>) -> Result<(), String> {
    let mut db = ProjectDb::new();
    for (path, source) in project {
        db.set_file(path, source.clone());
    }
    db.set_entry("main.ink")
        .ok_or_else(|| "warm(projectdb) setup: set_entry(main.ink) failed".to_string())?;

    // Untimed "open project" pull: gets the db past its first (cold) compile
    // so the loop below measures re-pulls, not the initial one.
    let opened = db.story_data().ok_or_else(|| {
        "warm(projectdb) setup: story_data unavailable after set_entry".to_string()
    })?;
    if !opened.errors.is_empty() {
        return Err(format!(
            "warm(projectdb) setup: synthetic project failed to compile: {} error(s)",
            opened.errors.len()
        ));
    }

    let edit_path = format!("file_{EDIT_FILE:02}.ink");
    let mut update = Vec::with_capacity(PROJECTDB_WARM_RUNS);
    let mut repull = Vec::with_capacity(PROJECTDB_WARM_RUNS);
    let mut full = Vec::with_capacity(PROJECTDB_WARM_RUNS);

    // Offset from bench_synthetic_warm's/bench_synthetic_warm_strict's
    // revision counters so no two benches ever compare identical edit
    // content.
    let mut revision: u64 = 2000;
    for _ in 0..PROJECTDB_WARM_RUNS {
        revision += 1;
        let edited = generate_file(EDIT_FILE, revision);

        let start = Instant::now();
        db.update_file(&edit_path, edited);
        let update_ms = ms(start);

        let start = Instant::now();
        let product = db.story_data().ok_or_else(|| {
            "warm(projectdb): story_data unavailable after update_file".to_string()
        })?;
        let repull_ms = ms(start);
        if !product.errors.is_empty() {
            return Err(format!(
                "warm(projectdb): edit produced {} unexpected compile error(s)",
                product.errors.len()
            ));
        }

        update.push(update_ms);
        repull.push(repull_ms);
        full.push(update_ms + repull_ms);
    }

    row(
        "synthetic_warm_projectdb.update_file",
        "1-line edit, single knot body (#838)",
        &update,
    );
    row(
        "synthetic_warm_projectdb.story_data_repull",
        "story_data() re-pull: FG-4d per-knot chunk memos + link",
        &repull,
    );
    row(
        "synthetic_warm_projectdb.full",
        "update_file + story_data() re-pull (vs #498 full_recompile)",
        &full,
    );
    Ok(())
}

// ── 7. Diagnostic-heavy synthetic project (issue #663) ───────────────

/// Same 50-file × 20-knot shape as [`generate_project`], generated by the
/// same seeded-per-file [`Lcg`] discipline (a one-line edit to file N never
/// changes any other file), but knot templates cycle through *seven* shapes
/// instead of four: the original text/mutation/choices/conditional mix,
/// plus a brink-extension-heavy `~ { … }` block, an all-fallback choice set,
/// and unreachable-after-divert content. Declares one project-wide `STRUCT`
/// and `LIST` (needed for the struct-literal/`list<L>` content above and the
/// annotated function knots below) once, in `diag_main.ink`.
///
/// Every [`DIAG_FN_STRIDE`]th file also gets one annotated `function` knot —
/// `int`/`string`/`list<Signals>`/`fn(int): string` param + return
/// annotations, every name recognized so `annotations::check` resolves them
/// without flagging `E061` (the whole point: this project must compile
/// clean under `Dialect::Brink` — see the module doc's item 7 for why
/// unknown-name `E061` content lives in a separate dedicated fixture,
/// [`generate_unknown_annotation_project`]).
fn generate_diag_project() -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    for f in 0..SYN_FILES {
        files.insert(format!("diag_file_{f:02}.ink"), generate_diag_file(f, 0));
    }

    let mut main =
        String::from("// Diagnostic-heavy synthetic project (#663) — generated, deterministic.\n");
    main.push_str("STRUCT Beacon = #{\n    level: int,\n    label: string,\n}\n\n");
    main.push_str("LIST Signals = active, idle, alarm\n\n");
    for f in 0..SYN_FILES {
        let _ = writeln!(main, "INCLUDE diag_file_{f:02}.ink");
    }
    main.push_str("VAR diag_main_counter = 0\n");
    main.push_str("The opening line of the diagnostic-heavy synthetic project. # generated\n");
    main.push_str("~ diag_main_counter = diag_main_counter + 1\n");
    main.push_str("-> dk00_00\n");
    files.insert("diag_main.ink".to_string(), main);
    files
}

/// Generate one included diagnostic-heavy file. `revision > 0` inserts the
/// same one-line warm edit as [`generate_file`], at the same
/// ([`EDIT_FILE`], [`EDIT_KNOT`]) coordinates, into whichever template that
/// knot happens to carry.
fn generate_diag_file(f: usize, revision: u64) -> String {
    let mut rng = Lcg::new();
    for _ in 0..=f {
        rng.next();
    }

    let mut s = format!("// Generated diagnostic-heavy file {f:02}.\n");
    for v in 0..3 {
        let _ = writeln!(s, "VAR var_f{f:02}_{v} = {}", rng.pick(0, 9));
    }
    s.push('\n');

    for k in 0..SYN_KNOTS {
        let _ = writeln!(s, "=== dk{f:02}_{k:02} ===");
        if revision > 0 && f == EDIT_FILE && k == EDIT_KNOT {
            let _ = writeln!(s, "A revised line, edit number {revision}.");
        }
        let next = if k + 1 < SYN_KNOTS {
            format!("-> dk{f:02}_{:02}", k + 1)
        } else {
            "-> DONE".to_string()
        };
        match (f * SYN_KNOTS + k) % 7 {
            0 => {
                // Text-heavy knot (same shape as generate_file's arm 0).
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
            3 => {
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
            4 => push_diag_extension_block(&mut s, f, &next),
            5 => push_diag_fallback_choice(&mut s, &mut rng, &next),
            _ => push_diag_unreachable_after_divert(&mut s, &mut rng, &next),
        }
        s.push('\n');
    }

    if f.is_multiple_of(DIAG_FN_STRIDE) {
        push_diag_annotated_function(&mut s, f);
    }

    s
}

/// Brink-extension block: `~ { … }` containing a `#[…]` array sigil, a
/// `#{…}` map sigil, postfix indexing, an unresolved stdlib call (`len`), a
/// struct construction literal, and field access — every `dialect_gate`-
/// recognized expression form `dialect_gate`'s module doc lists except
/// `#fn(…)`/`ref`/module directives, which have their own dedicated corpora
/// already and aren't per-knot-body shaped.
///
/// Field access is deliberately `beacons[0].level` (an `Index` base), not
/// `beacon.level` (a bare temp base) — the latter lowers to a multi-segment
/// `Path` rather than `Expr::FieldAccess`, which under `types = strict`
/// resolves through `ty_of_def` to the *base's own* `Ty::Struct(..)` instead
/// of `Ty::Unknown`, so unifying it against an `int` sibling in the same
/// arithmetic expression folds to `Ty::Conflicted` (`E066`) — a real
/// `infer::body` gap discovered while building this fixture, reported
/// separately (issue #663's scope is bench tooling, not analyzer fixes).
/// The `Index`-based form here goes through the real `Expr::FieldAccess`
/// arm, which correctly returns `Ty::Unknown` — safe to unify with anything.
fn push_diag_extension_block(s: &mut String, f: usize, next: &str) {
    s.push_str("~ {\n");
    s.push_str("    temp local_items = #[1, 2, 3]\n");
    s.push_str("    temp local_map = #{\"first\": 1, \"second\": 2}\n");
    s.push_str("    temp idx = local_items[0]\n");
    s.push_str("    temp count = len(local_items)\n");
    s.push_str(
        "    temp beacons = #[Beacon#{level: 1, label: \"a\"}, Beacon#{level: 2, label: \"b\"}]\n",
    );
    s.push_str("    temp picked = beacons[0].level\n");
    let _ = writeln!(
        s,
        "    var_f{f:02}_0 = var_f{f:02}_0 + idx + count + local_map[\"first\"] + picked"
    );
    s.push_str("}\n");
    let _ = writeln!(s, "The beacon counter now reads {{var_f{f:02}_0}}.");
    s.push_str(next);
    s.push('\n');
}

/// All-fallback choice set — `validate::validate`'s E034 (warning, never
/// blocks compilation).
fn push_diag_fallback_choice(s: &mut String, rng: &mut Lcg, next: &str) {
    s.push_str(&sentence(rng, 4, 8));
    s.push('\n');
    s.push_str("* ->\n");
    let _ = writeln!(s, "    {}", sentence(rng, 3, 6));
    let _ = writeln!(s, "- {}", sentence(rng, 3, 6));
    s.push_str(next);
    s.push('\n');
}

/// Unreachable-after-divert — `validate::validate`'s E033 (warning, never
/// blocks compilation). The divert transfers control before the trailing
/// sentence, which is why this is *safe* dead code, not a broken divert
/// chain.
fn push_diag_unreachable_after_divert(s: &mut String, rng: &mut Lcg, next: &str) {
    s.push_str(&sentence(rng, 4, 8));
    s.push('\n');
    s.push_str(next);
    s.push('\n');
    s.push_str(&sentence(rng, 3, 6));
    s.push('\n');
}

/// One annotated `function` knot per [`DIAG_FN_STRIDE`]'th file —
/// `int`/`string`/`list<Signals>`/`fn(int): string` param + return
/// annotations, every name recognized (see [`generate_diag_project`]'s doc
/// for why unrecognized names live in a separate fixture).
fn push_diag_annotated_function(s: &mut String, f: usize) {
    let _ = writeln!(
        s,
        "=== function diag_sig_{f:02}(count: int, label: string, items: list<Signals>, \
         transform: fn(int): string): string ==="
    );
    s.push_str("~ {\n");
    s.push_str("    if count > 0 {\n");
    s.push_str("        return label\n");
    s.push_str("    }\n");
    s.push_str("    return \"none\"\n");
    s.push_str("}\n\n");
}

/// The diagnostic-heavy project must still compile clean under
/// `Dialect::Brink` (E033/E034 warnings expected and fine — see the module
/// doc's item 7) — a project full of *errors* would measure the diagnostic
/// path, not the compile path, same rationale as
/// [`verify_synthetic_compiles`].
fn verify_diag_compiles(project: &BTreeMap<String, String>) -> Result<(), String> {
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    match brink_compiler::compile_with_options("diag_main.ink", read_from(project), options) {
        Ok(output) => {
            println!(
                "compile_bench | diag.verify | ok warnings={}",
                output.warnings.len()
            );
            if output.warnings.is_empty() {
                return Err(
                    "diag project compiled with zero warnings — expected E033/E034 from its \
                     structural-edge-case knots, so validate isn't being exercised"
                        .to_string(),
                );
            }
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
            Err(format!("diag project failed to compile: {detail}"))
        }
    }
}

fn bench_diag_cold(project: &BTreeMap<String, String>, runs: usize) -> Result<(), String> {
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        brink_compiler::compile_with_options("diag_main.ink", read_from(project), options.clone())
            .map_err(|e| format!("diag compile failed mid-benchmark: {e}"))?;
        samples.push(ms(start));
    }
    row(
        "diag_cold.compile",
        "brink dialect, gradual types",
        &samples,
    );
    Ok(())
}

/// Warm one-line-edit loop under `Dialect::Brink`, same shape as
/// [`bench_synthetic_warm`] — the real per-file `validate`/`dialect_gate`/
/// `annotations::check` load this project variant exists to produce, under
/// the dialect the project actually compiles clean with.
fn bench_diag_warm(project: &BTreeMap<String, String>, runs: usize) -> Result<(), String> {
    let mut driver = Driver::new();
    driver
        .discover("diag_main.ink", read_from(project))
        .map_err(|e| format!("diag warm setup discovery failed: {e}"))?;
    let entry = driver
        .db()
        .file_id("diag_main.ink")
        .ok_or_else(|| "diag warm setup: diag_main.ink missing after discovery".to_string())?;
    driver.set_analysis_options(AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    });
    let mut db = driver.into_db();

    let edit_path = format!("diag_file_{EDIT_FILE:02}.ink");
    let mut update = Vec::with_capacity(runs);
    let mut reanalyze = Vec::with_capacity(runs);
    let mut analyze = Vec::with_capacity(runs);
    let mut diagnostics = Vec::with_capacity(runs);
    let mut lir_lower = Vec::with_capacity(runs);
    let mut codegen = Vec::with_capacity(runs);
    let mut full = Vec::with_capacity(runs);

    // Offset from every other bench's revision counter so no two benches
    // ever compare identical edit content.
    let mut revision: u64 = 3000;
    for _ in 0..runs {
        revision += 1;
        let edited = generate_diag_file(EDIT_FILE, revision);
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
        "diag_warm.update_file",
        "1-line edit, per-knot HIR cache",
        &update,
    );
    row(
        "diag_warm.reanalyze_ide",
        "update_file + analyze, brink dialect",
        &reanalyze,
    );
    row(
        "diag_warm.stage.analyze",
        "validate + dialect_gate + annotations::check",
        &analyze,
    );
    row(
        "diag_warm.stage.diagnostics",
        "collect+partition (E033/E034 warnings present)",
        &diagnostics,
    );
    row(
        "diag_warm.stage.lir_lower",
        "HIR normalize + LIR lowering",
        &lir_lower,
    );
    row("diag_warm.stage.codegen", "LIR -> StoryData emit", &codegen);
    row(
        "diag_warm.full_recompile",
        "update_file .. codegen (StoryData)",
        &full,
    );
    Ok(())
}

/// Same warm loop as [`bench_diag_warm`], but with `types = strict` —
/// "plus a strict-mode variant with annotation density" (issue #663): unlike
/// [`bench_synthetic_warm_strict`]'s annotation-free project, this project's
/// `DIAG_FN_STRIDE`-spaced annotated function knots give `strict::check` /
/// `annotations::mismatches` real per-file annotation content to consume
/// through the memoized `type_inference_query`, not zero-annotation
/// boilerplate.
fn bench_diag_warm_strict(project: &BTreeMap<String, String>, runs: usize) -> Result<(), String> {
    let mut driver = Driver::new();
    driver
        .discover("diag_main.ink", read_from(project))
        .map_err(|e| format!("diag strict warm setup discovery failed: {e}"))?;
    let entry = driver.db().file_id("diag_main.ink").ok_or_else(|| {
        "diag strict warm setup: diag_main.ink missing after discovery".to_string()
    })?;
    driver.set_analysis_options(AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Strict),
        ..AnalysisOptions::default()
    });
    let mut db = driver.into_db();

    let edit_path = format!("diag_file_{EDIT_FILE:02}.ink");
    let mut update = Vec::with_capacity(runs);
    let mut reanalyze = Vec::with_capacity(runs);
    let mut analyze = Vec::with_capacity(runs);
    let mut diagnostics = Vec::with_capacity(runs);
    let mut full = Vec::with_capacity(runs);

    let mut revision: u64 = 4000;
    for _ in 0..runs {
        revision += 1;
        let edited = generate_diag_file(EDIT_FILE, revision);
        let mut d = Driver::from_db(db);

        let start = Instant::now();
        d.db_mut().update_file(&edit_path, edited);
        let update_ms = ms(start);

        let stages = staged_back_half(&mut d, entry)?;

        update.push(update_ms);
        analyze.push(stages.analyze);
        diagnostics.push(stages.diagnostics);
        reanalyze.push(update_ms + stages.analyze);
        full.push(update_ms + stages.total());

        db = d.into_db();
    }

    row(
        "diag_warm_strict.update_file",
        "1-line edit, per-knot HIR cache",
        &update,
    );
    row(
        "diag_warm_strict.reanalyze_ide",
        "update_file + analyze, types=strict, annotation-dense",
        &reanalyze,
    );
    row(
        "diag_warm_strict.stage.analyze",
        "cross-file analysis incl. strict::check + annotations::mismatches",
        &analyze,
    );
    row(
        "diag_warm_strict.stage.diagnostics",
        "collect+partition diagnostics",
        &diagnostics,
    );
    row(
        "diag_warm_strict.full_recompile",
        "update_file .. codegen (StoryData)",
        &full,
    );
    Ok(())
}

/// The "both dialects" half of the `dialect_gate` comparison: the *same*
/// diag project, unmodified, compiled under the default `Dialect::StrictInk`
/// instead of `Dialect::Brink`. Every brink-extension construct and every
/// annotation this project contains is now a real `E051` — analyze-only (no
/// LIR/codegen: a strict-ink project with brink-extension content is
/// *supposed* to fail compilation here, which is exactly what's measured).
fn bench_diag_dialect_gate_strict(
    project: &BTreeMap<String, String>,
    runs: usize,
) -> Result<(), String> {
    let mut analyze_ms = Vec::with_capacity(runs);
    let mut diagnostics_ms = Vec::with_capacity(runs);
    let mut error_count = 0usize;

    for run in 0..runs {
        // Independent cold Driver per run (default AnalysisOptions =
        // StrictInk dialect, gradual types) — no persistent-db warm reuse
        // here, this leg only needs to show the gate firing for real.
        let mut driver = Driver::new();
        driver
            .discover("diag_main.ink", read_from(project))
            .map_err(|e| format!("diag strict-gate discovery failed: {e}"))?;
        let entry = driver
            .db()
            .file_id("diag_main.ink")
            .ok_or_else(|| "diag strict-gate: diag_main.ink missing after discovery".to_string())?;

        let start = Instant::now();
        let analysis = driver.analyze().clone();
        analyze_ms.push(ms(start));

        let start = Instant::now();
        let report = driver.collect_diagnostics(&analysis, Some(entry));
        diagnostics_ms.push(ms(start));

        if run == 0 {
            error_count = report.errors.len();
        }
    }

    row(
        "diag_dialect_gate_strict.analyze_cold",
        &format!("strict-ink dialect, E051 errors={error_count}"),
        &analyze_ms,
    );
    row(
        "diag_dialect_gate_strict.diagnostics_cold",
        "collect+partition (all E051-class)",
        &diagnostics_ms,
    );

    if error_count == 0 {
        return Err(
            "diag strict-gate: expected E051 diagnostics from brink-extension content under \
             strict-ink, found none — dialect_gate isn't being exercised"
                .to_string(),
        );
    }
    Ok(())
}

/// Dedicated fixture with deliberately unrecognized type-annotation names —
/// kept separate from [`generate_diag_project`] because it must produce real
/// `E061`s (an error), which would otherwise break that project's "compiles
/// clean" invariant. `annotations::check` only runs under `Dialect::Brink`
/// (see its module doc), so every case here is also exercised for real.
fn generate_unknown_annotation_project() -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    let body = "\
// Dedicated fixture (issue #663): deliberately unknown type names so
// annotations::check's E061 path fires for real.
Root text, never reached by anything below. # generated
-> DONE

=== function bogus_leaf(x: Frobnicator): int ===
~ {
    return 0
}

=== function bogus_generic(items: list<NotDeclared>): int ===
~ {
    return 0
}

=== function bogus_fn_component(cb: fn(Bogus): AlsoBogus): int ===
~ {
    return 0
}
"
    .to_string();
    files.insert("unknown_annotations.ink".to_string(), body);
    files
}

fn bench_diag_unknown_annotations(runs: usize) -> Result<(), String> {
    let project = generate_unknown_annotation_project();
    let mut analyze_ms = Vec::with_capacity(runs);
    let mut error_count = 0usize;

    for run in 0..runs {
        let mut driver = Driver::new();
        driver.set_analysis_options(AnalysisOptions {
            dialect: Dialect::Brink,
            ..AnalysisOptions::default()
        });
        driver
            .discover("unknown_annotations.ink", read_from(&project))
            .map_err(|e| format!("unknown-annotation fixture discovery failed: {e}"))?;
        let entry = driver
            .db()
            .file_id("unknown_annotations.ink")
            .ok_or_else(|| {
                "unknown-annotation fixture: entry file missing after discovery".to_string()
            })?;

        let start = Instant::now();
        let analysis = driver.analyze().clone();
        analyze_ms.push(ms(start));

        let report = driver.collect_diagnostics(&analysis, Some(entry));
        if run == 0 {
            error_count = report.errors.len();
        }
    }

    row(
        "diag_unknown_annotations.analyze",
        &format!("brink dialect, E061 errors={error_count}"),
        &analyze_ms,
    );

    if error_count == 0 {
        return Err(
            "expected E061 diagnostics from unknown annotation names, found none — \
             annotations::check isn't being exercised"
                .to_string(),
        );
    }
    Ok(())
}
