//! A reading off the interaction counters (`crate::perf`), over real
//! stories, with the query set the **host actually fires** rather than an
//! invented one.
//!
//! `#[ignore]`d: this is a measurement, not an assertion. Wall-clock on a
//! shared runner would be a flaky test and a false ratchet — what CI
//! enforces is `perf_coverage.rs` (every query is *timed*), not how long any
//! of them takes. Run it by hand:
//!
//! ```text
//! cargo test --release -p brink-web --lib perf_probe -- --ignored --nocapture
//! ```
//!
//! **Use `--release`.** A debug build is ~8x slower here and reorders
//! nothing, so it inflates every row without changing the ranking.
//!
//! **The numbers are native, not wasm.** `crate::perf::now_ms` has a
//! `std::time::Instant` arm off wasm32, so the counters work in the `--lib`
//! test build — the only build that can run them, since the crate is
//! `cdylib`-only with no integration-test road. Native timings are a *lower
//! bound* on the browser: same work, no wasm translation, no JS boundary.
//! Rank the phases against each other with them; do not quote them as what
//! an author feels.
//!
//! ## The query set is not a guess
//!
//! An earlier version of this probe fired every query once per keystroke.
//! That measured a host that does not exist, and it put `codeActions` at the
//! top of the per-keystroke bill when the host only reaches it from `Mod-.`.
//! The sets below follow the TypeScript wiring:
//!
//! - **Per keystroke, synchronously, in the transaction**: the source write,
//!   `lineContexts`, `semanticTokens`, `foldingRanges`, `hirSpans`,
//!   `argumentWidgets`, `inlayHints`.
//! - **Only above `DEFER_LINE_THRESHOLD` (1000 lines)** does the host stop
//!   doing most of that per keystroke: `packages/ink-editor/src/deferred-refresh.ts`
//!   maps the existing decorations through the change and rebuilds content on
//!   a 120 ms quiet timer instead, and `semanticTokens` swaps to its `fast`
//!   variant. **A document under 1000 lines pays the full synchronous bill on
//!   every keystroke.** That threshold is why this probe measures two
//!   fixtures rather than one — the large story is on the *cheap* side of it.
//! - **Ranged queries are not viewport-scoped.** `inlayHints` and
//!   `argumentWidgets` are both called `(0, doc.length)`; nothing computes a
//!   viewport range. So the whole-document range below is the host's
//!   behaviour, not this harness overreaching.
//! - **500 ms debounce**: `compileProject`, and riding its fan-out,
//!   `projectOutline` / `storyGraph` / `draftPaths` — none of them gated on
//!   the panel being open.
//! - **On demand only**: `codeActions` and `fixesAt` (`Mod-.`), `hover`
//!   (pointer dwell), `completions` (only when `matchBefore(/[\w.]+/)`
//!   matches, so not on space or punctuation).

use std::fmt::Write as _;

use super::EditorSession;

/// `tests/tier3/misc/TheIntercept/story.ink` — ~100 KB, 1686 lines of real
/// authored ink (inkle's own sample). **Above** the 1000-line threshold, so
/// the host defers most of its decoration rebuilds.
const LARGE: &str = "../../tests/tier3/misc/TheIntercept/story.ink";

/// ~27 KB, 335 lines of real authored ink. **Below** the threshold, so the
/// host rebuilds everything synchronously on every keystroke — the case the
/// deferral machinery does not cover, and the one an author is most likely
/// to be typing in.
const SMALL: &str =
    "../../tests/tests_github/MattWoelk__DiscordStoryBot/stories/christmas_2021/christmas.ink";

fn read(rel: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    let text = std::fs::read_to_string(&path);
    assert!(text.is_ok(), "read {}: {:?}", path.display(), text.err());
    text.expect("just asserted above")
}

fn report_rows(json: &str) -> Vec<(String, u64, f64, f64)> {
    let v: serde_json::Value = serde_json::from_str(json).expect("counter JSON");
    let mut rows: Vec<(String, u64, f64, f64)> = v
        .as_object()
        .expect("object")
        .iter()
        .map(|(k, r)| {
            (
                k.clone(),
                r["count"].as_u64().unwrap_or(0),
                r["totalMs"].as_f64().unwrap_or(0.0),
                r["maxMs"].as_f64().unwrap_or(0.0),
            )
        })
        .collect();
    rows.sort_by(|a, b| b.2.total_cmp(&a.2));
    rows
}

fn print_table(title: &str, json: &str, divisor: f64) -> f64 {
    println!("\n--- {title} ---");
    println!(
        "{:<24} {:>6} {:>10} {:>9}",
        "counter", "calls", "mean ms", "max ms"
    );
    let rows = report_rows(json);
    let mut total = 0.0;
    for (name, count, sum, max) in &rows {
        if *count == 0 {
            continue;
        }
        total += sum;
        println!("{name:<24} {count:>6} {:>10.2} {max:>9.2}", sum / divisor);
    }
    println!("{:<24} {:>6} {:>10.2}", "TOTAL", "", total / divisor);
    total / divisor
}

/// Exactly what a `docChanged` transaction runs synchronously, for a
/// document under the 1000-line deferral threshold.
fn keystroke_sweep(session: &EditorSession, doc: u32, doc_len: u32) {
    let _ = session.line_contexts_doc(doc);
    let _ = session.semantic_tokens_doc(doc);
    let _ = session.folding_ranges_doc(doc);
    let _ = session.hir_spans_doc(doc);
    // Both ranged queries are called `(0, doc.length)` by the host.
    let _ = session.argument_widgets_doc(doc, 0, doc_len);
    let _ = session.inlay_hints_doc(doc, 0, doc_len);
}

/// The 500 ms-debounced fan-out: the compile, then the panels that ride its
/// result whether or not they are visible.
fn compile_sweep(session: &mut EditorSession, entry: &str) {
    let _ = session.compile_project(entry);
    let _ = session.project_outline();
    let _ = session.story_graph();
    let _ = session.draft_glob_report();
}

fn measure(label: &str, rel: &str) {
    const KEYSTROKES: usize = 20;
    #[expect(clippy::cast_precision_loss, reason = "20 iterations")]
    let n = KEYSTROKES as f64;

    let src = read(rel);
    println!(
        "\n════ {label}: {} bytes, {} lines {}════",
        src.len(),
        src.lines().count(),
        if src.lines().count() >= 1000 {
            "(ABOVE the 1000-line threshold — host defers) "
        } else {
            "(below the threshold — host is synchronous) "
        }
    );

    let mut session = EditorSession::new();
    session.set_perf_enabled(true);
    session.perf_reset();

    session.update_file("story.ink", &src);
    assert!(session.set_active_file("story.ink"));
    // The host drives the keystroke path through a document handle
    // (`DocHandle`), not the bare session entry points.
    let doc = session.open_document("story.ink");
    let doc_len = u32::try_from(src.len()).expect("len fits");
    let cursor = doc_len / 2;
    keystroke_sweep(&session, doc, doc_len);
    print_table("cold open", &session.perf_counters_json(), 1.0);

    // Typing: each iteration sends a *different* source, as a keystroke
    // does. Re-sending identical text would measure salsa's early-return.
    session.perf_reset();
    for i in 0..KEYSTROKES {
        let mut edited = src.clone();
        let _ = writeln!(edited, "\n// {}", "x".repeat(i + 1));
        session.update_file("story.ink", &edited);
        keystroke_sweep(&session, doc, doc_len);
    }
    let per_key = print_table(
        "per keystroke (mean of 20)",
        &session.perf_counters_json(),
        n,
    );
    println!(
        "  → {:.1} keystrokes/sec before the main thread is saturated",
        1000.0 / per_key.max(f64::MIN_POSITIVE)
    );

    // The debounced compile fan-out, priced once.
    session.perf_reset();
    compile_sweep(&mut session, "story.ink");
    print_table(
        "compile fan-out (500 ms debounce, 1 pass)",
        &session.perf_counters_json(),
        1.0,
    );

    // #2885: does salsa memoization survive between two compiles with no
    // edit in between? `IdeSession::compile` writes `set_analysis_options`
    // unconditionally; if that write is what dirties the db, the second
    // compile costs the same as the first and every editor compile is
    // cold-priced.
    let probe = session.perf_compile_probe("story.ink");
    println!("  back-to-back compile, no edit between [first_ms, second_ms]: {probe}");

    // On-demand queries, priced separately: real costs, but paid on a
    // keypress the author chose, not on every character.
    session.perf_reset();
    let _ = session.code_actions(cursor);
    let _ = session.fixes_at(cursor);
    let _ = session.hover(cursor);
    let _ = session.completions(cursor);
    print_table(
        "on demand (Mod-. / dwell / word char)",
        &session.perf_counters_json(),
        1.0,
    );
}

#[test]
#[ignore = "measurement, not an assertion: wall-clock numbers, run explicitly"]
fn interaction_cost_over_real_stories() {
    measure("SMALL", SMALL);
    measure("LARGE", LARGE);
}

/// What the keystroke path actually pays for, as opposed to which counter
/// the bill lands on.
///
/// The per-keystroke sweep puts `ide.inlayHints` at ~11.5 ms on a 100 KB
/// story, but the scaling table shows the same query at ~1 ms when it is
/// called twice with no edit in between. This pulls its inputs by hand,
/// in order, after a real edit, to find where the other ~11 ms goes.
///
/// **The obvious answer is wrong, and this test is what ruled it out.** The
/// hypothesis was the whole-file ink parse (`syntax_root`), which every edit
/// invalidates and which `line_contexts` avoids by going through the
/// assembled per-segment query (#3064 B3/B5). It is real — ~4 ms — but
/// forcing it first barely moves the hint number. With `syntax_root`,
/// `analysis()` and `host_values()` all pulled warm beforehand,
/// `inlay_hints` still costs ~12.6 ms. So the cost is not a shared input at
/// all: it is inside the hint walk, which is cheap to repeat on an
/// unchanged document and expensive after any edit. That points at
/// per-node work in `brink_ide::inlay_hints` whose memoization is
/// revision-scoped, and it is where a fix would have to look — not at the
/// parse, and not at the query's own call site.
#[test]
#[ignore = "measurement, not an assertion: wall-clock numbers, run explicitly"]
fn what_an_edit_invalidates() {
    const N: usize = 10;
    #[expect(clippy::cast_precision_loss, reason = "10 iterations")]
    let n = N as f64;

    let src = read(LARGE);
    let mut session = EditorSession::new();
    session.set_perf_enabled(true);
    session.update_file("story.ink", &src);
    assert!(session.set_active_file("story.ink"));
    let doc = session.open_document("story.ink");
    let doc_len = u32::try_from(src.len()).expect("len fits");

    let (mut analyze, mut root, mut hints) = (0.0, 0.0, 0.0);
    let (mut analysis, mut host) = (0.0, 0.0);

    for i in 0..N {
        let mut edited = src.clone();
        let _ = writeln!(edited, "\n// {}", "x".repeat(i + 1));

        let t0 = crate::perf::now_ms();
        session.update_file("story.ink", &edited);
        let t1 = crate::perf::now_ms();
        // The whole-file ink parse. `line_contexts` does NOT pull this (it
        // goes through the assembled per-segment query, #3064 B3/B5), which
        // is why it stays cheap while its neighbour does not.
        let file_id = session.session.file_id("story.ink").expect("file id");
        std::hint::black_box(session.session.syntax_root(file_id));
        let t2 = crate::perf::now_ms();
        std::hint::black_box(session.session.analysis());
        let t3 = crate::perf::now_ms();
        std::hint::black_box(session.session.host_values());
        let t4 = crate::perf::now_ms();
        std::hint::black_box(session.inlay_hints_doc(doc, 0, doc_len));
        let t5 = crate::perf::now_ms();

        analyze += t1 - t0;
        root += t2 - t1;
        analysis += t3 - t2;
        host += t4 - t3;
        hints += t5 - t4;
    }

    println!(
        "\nafter each edit, pulled in this order (mean of {N}) — each row is\n\
         priced with every row above it already warm:"
    );
    println!("  update_file (write + analyze)  {:>7.2} ms", analyze / n);
    println!("  syntax_root  (whole-file parse) {:>6.2} ms", root / n);
    println!("  analysis()   (project bundle)   {:>6.2} ms", analysis / n);
    println!("  host_values()                   {:>6.2} ms", host / n);
    println!("  inlayHints   (everything warm)  {:>6.2} ms", hints / n);
}

/// Would viewport-scoping `inlayHints` actually help? **Partly — and the
/// remainder is the more interesting half.**
///
/// The host asks for `(0, doc.length)`; a real viewport is ~50 lines. On a
/// 100 KB story, asking for 2% of the document costs ~4.3 ms against
/// ~15.8 ms for all of it: a 3.7x saving, not the ~50x a
/// purely range-proportional walk would give.
///
/// So the cost decomposes into two parts that want two different fixes:
///
/// - **~11.5 ms range-proportional** — recovered by having the host pass its
///   viewport instead of the whole document. A TypeScript change.
/// - **~4.3 ms range-INDEPENDENT floor** — paid even for 2% of the file, and
///   paid on every keystroke. Whatever this is, a narrower range never
///   removes it; it lives in `brink_ide::inlay_hints` (or an input it pulls
///   that the earlier probe did not isolate) and is the harder, more
///   valuable target of the two.
///
/// Measured after a real edit, since that is the state that costs — the
/// same call is ~1 ms on an unchanged document either way. Each half of the
/// comparison gets its own edit so neither is measured on a document the
/// other has already warmed.
#[test]
#[ignore = "measurement, not an assertion: wall-clock numbers, run explicitly"]
fn does_range_scoping_help_inlay_hints() {
    const N: usize = 10;
    #[expect(clippy::cast_precision_loss, reason = "10 iterations")]
    let n = N as f64;

    let src = read(LARGE);
    let doc_len = u32::try_from(src.len()).expect("len fits");
    // ~50 lines around the middle, which is what a viewport actually spans.
    let mid = doc_len / 2;
    let vp_start = mid.saturating_sub(1000);
    let vp_end = mid.saturating_add(1000);

    let mut session = EditorSession::new();
    session.set_perf_enabled(true);
    session.update_file("story.ink", &src);
    assert!(session.set_active_file("story.ink"));
    let doc = session.open_document("story.ink");

    let mut whole = 0.0;
    let mut viewport = 0.0;
    for i in 0..N {
        // Each half gets its OWN edit, so neither is measured on a document
        // the other has already warmed.
        let mut a = src.clone();
        let _ = writeln!(a, "\n// {}", "x".repeat(i + 1));
        session.update_file("story.ink", &a);
        let t0 = crate::perf::now_ms();
        std::hint::black_box(session.inlay_hints_doc(doc, 0, doc_len));
        whole += crate::perf::now_ms() - t0;

        let mut b = src.clone();
        let _ = writeln!(b, "\n// {}", "y".repeat(i + 1));
        session.update_file("story.ink", &b);
        let t1 = crate::perf::now_ms();
        std::hint::black_box(session.inlay_hints_doc(doc, vp_start, vp_end));
        viewport += crate::perf::now_ms() - t1;
    }

    println!("\ninlayHints after an edit, on a {doc_len}-byte story (mean of {N}):");
    println!("  whole document (0..{doc_len})  {:>7.2} ms", whole / n);
    println!(
        "  ~50-line viewport ({vp_start}..{vp_end}) {:>5.2} ms",
        viewport / n
    );
    let floor = viewport / n;
    let proportional = (whole - viewport) / n;
    println!(
        "  ratio {:.2}x over {:.1}% of the document\n\
         \x20 -> ~{:.1} ms is range-proportional (a host viewport recovers it)\n\
         \x20 -> ~{:.1} ms is a range-INDEPENDENT floor, paid every keystroke\n\
         \x20    whatever the range; only the walk itself can remove that.",
        whole / viewport.max(f64::MIN_POSITIVE),
        100.0 * f64::from(vp_end - vp_start) / f64::from(doc_len),
        proportional,
        floor
    );
}

/// How the two whole-document ranged queries scale with document size.
///
/// The host calls `inlayHints` and `argumentWidgets` as `(0, doc.length)` —
/// never a viewport range — so their cost is a function of the whole file.
/// The sweep above showed `ide.inlayHints` at 0.10 ms on a 27 KB story and
/// 11.55 ms on a 100 KB one: 115x the time for 3.7x the bytes. If that is
/// real growth rather than two unlike stories, the 1000-line deferral
/// threshold does not protect the documents that most need protecting — a
/// file just under it pays the full cost synchronously, on every keystroke.
///
/// Real stories from the corpus, not synthesised text: a file of repeated
/// knots would measure a shape no author writes.
#[test]
#[ignore = "measurement, not an assertion: wall-clock numbers, run explicitly"]
fn whole_document_query_scaling() {
    const N: usize = 10;
    const FILES: &[&str] = &[
        "../../tests/tests_github/Phauks__Tales-of-London/source/events/event_random_hub.ink",
        "../../tests/tests_github/Boyquotes__signal_creek/assets/ink/hallway/bobatea.ink",
        "../../tests/tests_github/alobacheva__Tsiolkov-Sky/scene_2.ink",
        "../../tests/tests_github/Jonkeevy__INK-FUNCTION-LIBRARY/FUNC_NPCgenerator_Demo.ink",
        "../../tests/tests_patched/Boyquotes__signal_creek/assets/ink/hallway/wertoys.ink",
        "../../tests/tests_patched/Boyquotes__signal_creek/assets/ink/hallway/cafetables.ink",
        "../../tests/tests_github/Phauks__Tales-of-London/source/hubs/your_lodgings.ink",
        "../../tests/tests_patched/yannlemos__Sky-Caravan-Ink/Various/Intro.ink",
        LARGE,
    ];

    #[expect(clippy::cast_precision_loss, reason = "10 iterations")]
    let n = N as f64;
    println!(
        "\n{:>8} {:>7} {:>12} {:>12}   {}",
        "bytes", "lines", "inlayHints", "argWidgets", "story"
    );
    for rel in FILES {
        let src = read(rel);
        let mut session = EditorSession::new();
        session.set_perf_enabled(true);
        session.update_file("story.ink", &src);
        assert!(session.set_active_file("story.ink"));
        let doc = session.open_document("story.ink");
        let doc_len = u32::try_from(src.len()).expect("len fits");

        // Warm first, then measure: a cold pull would price the parse and
        // lowering that a mid-typing call has already paid.
        let _ = session.inlay_hints_doc(doc, 0, doc_len);
        let _ = session.argument_widgets_doc(doc, 0, doc_len);
        session.perf_reset();
        for _ in 0..N {
            let _ = session.inlay_hints_doc(doc, 0, doc_len);
            let _ = session.argument_widgets_doc(doc, 0, doc_len);
        }
        let rows = report_rows(&session.perf_counters_json());
        let get = |name: &str| rows.iter().find(|r| r.0 == name).map_or(0.0, |r| r.2 / n);
        println!(
            "{:>8} {:>7} {:>10.2}ms {:>10.2}ms   {}",
            src.len(),
            src.lines().count(),
            get("ide.inlayHints"),
            get("ide.argumentWidgets"),
            std::path::Path::new(rel)
                .file_name()
                .map_or_else(|| rel.to_string(), |f| f.to_string_lossy().into_owned())
        );
    }
}

/// Why `ide.codeActions` costs what it does. Separate from the sweep above
/// because it is an on-demand query, not part of the keystroke bill — but it
/// is the single most expensive thing the editor can be asked for, and
/// almost none of the cost is the answer.
#[test]
#[ignore = "measurement, not an assertion: wall-clock numbers, run explicitly"]
fn code_actions_decomposition() {
    const N: usize = 20;
    #[expect(clippy::cast_precision_loss, reason = "20 iterations")]
    let n = N as f64;

    let src = read(LARGE);

    let (mut parse_ms, mut fmt_ms, mut whole_ms) = (0.0, 0.0, 0.0);
    for _ in 0..N {
        let t0 = crate::perf::now_ms();
        std::hint::black_box(brink_syntax::parse(&src));
        let t1 = crate::perf::now_ms();
        std::hint::black_box(brink_fmt::format(&src, &brink_fmt::FormatConfig::default()));
        let t2 = crate::perf::now_ms();
        std::hint::black_box(brink_ide::code_actions::code_actions(&src, src.len() / 2));
        whole_ms += crate::perf::now_ms() - t2;
        parse_ms += t1 - t0;
        fmt_ms += t2 - t1;
    }
    println!("\ncode_actions on TheIntercept (mean of {N}):");
    println!("  whole query           {:>7.2} ms", whole_ms / n);
    println!(
        "  ├─ re-parse           {:>7.2} ms ({:.0}%)  — the db already holds a memoized tree",
        parse_ms / n,
        100.0 * parse_ms / whole_ms
    );
    println!(
        "  └─ whole-doc reformat {:>7.2} ms ({:.0}%)  — to decide one knot's \"Format knot\" offer",
        fmt_ms / n,
        100.0 * fmt_ms / whole_ms
    );

    // The offer test is `source.get(a..b) != formatted.get(a..b)` — the same
    // byte offsets in both texts. That compares like with like only if
    // formatting preserves offsets, which is precisely what a formatter does
    // not promise. Record the drift so the claim is measured, not asserted.
    let formatted = brink_fmt::format(&src, &brink_fmt::FormatConfig::default());
    #[expect(
        clippy::cast_possible_wrap,
        reason = "source sizes are far under i64::MAX"
    )]
    let delta = formatted.len() as i64 - src.len() as i64;
    println!(
        "\n  formatter offset drift: {} bytes source -> {} formatted (delta {delta}); \
         the knot-range slices compared at equal offsets cannot line up past the first \
         length change",
        src.len(),
        formatted.len()
    );
}
