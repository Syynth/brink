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

/// The keystroke bill through the write path the host **actually** uses.
///
/// `interaction_cost_over_real_stories` writes with `update_file`, which is
/// `timed_update_and_analyze` — the source write PLUS an eager
/// `refresh_analysis`. That is not the keystroke path. The host splices
/// through `apply_edits_document`, whose doc comment is explicit that it
/// writes the source input *without* the fused eager analysis: "consumers
/// pull what they need … the diagnostics bundle is computed when the
/// debounced compile path asks, not per keystroke".
///
/// So the earlier `ide.analyze` row is work the editor defers to the 500 ms
/// compile, and counting it per keystroke overstates the bill. This measures
/// both paths side by side on the same document so the difference is the
/// eager analysis and nothing else.
///
/// It also settles what the incremental machinery does and does not cover.
/// The analysis half **is** per-knot incremental (#3084): `raw_lowered_query`
/// rides the segment road, `segment_lowered_query` parses only its own
/// segment's text, and a knot-interior edit re-lowers one knot. But that
/// same query's doc says the road "deliberately does NOT read `parse_query`
/// … IDE consumers that want the whole-file tree still pull `parse_query`
/// themselves" — and `inlay_hints`/`argument_widgets` are exactly those
/// consumers (`hints.rs` pulls `syntax_root`, i.e. `db.parse(id)`, a
/// whole-file parse keyed on the file). So the queries that dominate the
/// keystroke bill are the ones that opt out of the incremental road.
#[derive(Clone, Copy)]
enum Write {
    EagerPush,
    SpliceAtEnd,
    SpliceMidKnot,
}

#[test]
#[ignore = "measurement, not an assertion: wall-clock numbers, run explicitly"]
fn the_real_keystroke_write_path() {
    const KEYSTROKES: usize = 20;
    #[expect(clippy::cast_precision_loss, reason = "20 iterations")]
    let n = KEYSTROKES as f64;

    for (label, rel) in [("SMALL", SMALL), ("LARGE", LARGE)] {
        let src = read(rel);
        let doc_len = u32::try_from(src.len()).expect("len fits");
        println!(
            "\n════ {label}: {} bytes, {} lines ════",
            src.len(),
            src.lines().count()
        );

        for mode in [Write::EagerPush, Write::SpliceAtEnd, Write::SpliceMidKnot] {
            let mut session = EditorSession::new();
            session.set_perf_enabled(true);
            session.update_file("story.ink", &src);
            assert!(session.set_active_file("story.ink"));
            let doc = session.open_document("story.ink");
            keystroke_sweep(&session, doc, doc_len);
            session.perf_reset();

            for i in 0..KEYSTROKES {
                match mode {
                    Write::EagerPush => {
                        // The probe's original path: full push + eager analyze.
                        let mut edited = src.clone();
                        let _ = writeln!(edited, "\n// {}", "x".repeat(i + 1));
                        session.update_file("story.ink", &edited);
                    }
                    // The host's path: a one-character insert spliced in, no
                    // eager analysis. Position matters to the segment road,
                    // so both ends of its range are measured: appending
                    // dirties the last segment only (its cheapest case),
                    // while typing mid-document dirties an interior knot and
                    // shifts every segment after it.
                    Write::SpliceAtEnd | Write::SpliceMidKnot => {
                        let at = if matches!(mode, Write::SpliceAtEnd) {
                            doc_len.saturating_add(u32::try_from(i).unwrap_or(0))
                        } else {
                            doc_len / 2
                        };
                        let edits = format!("[{{\"from\":{at},\"to\":{at},\"insert\":\"x\"}}]");
                        let ok = session.apply_edits_document(doc, &edits);
                        assert!(ok, "apply_edits_document must accept the splice");
                    }
                }
                keystroke_sweep(&session, doc, doc_len);
            }

            let path = match mode {
                Write::EagerPush => "update_file (probe's original: push + eager analyze)",
                Write::SpliceAtEnd => "apply_edits_document, appending (host path, last segment)",
                Write::SpliceMidKnot => {
                    "apply_edits_document, mid-document (host path, interior knot)"
                }
            };
            print_table(path, &session.perf_counters_json(), n);
        }
    }
}

/// Where the keystroke bill actually comes from: **whole-project type
/// inference, re-run on every edit.** Not the hint walk, not the parse.
///
/// The chain: the hint walk resolves each `~ temp` to its inferred type via
/// `db.infer_body(def)`; `infer_body_query` opens with
/// `scc_membership_query(db, project)`, the call-graph SCC partition for the
/// **entire project**, keyed on the project. Its own doc calls that
/// deliberate — "SCC membership is inherently a global graph property, not
/// narrowable per-def" (Ruling 2c) — and FG-2 gives the layer below it a
/// per-SCC cutoff so an *unchanged* graph backdates. The cutoff protects
/// downstream memos from re-solving; it does not stop the graph itself from
/// being rebuilt.
///
/// Measured on a 100 KB story, after a single one-character insert at the
/// END of the file — an edit that cannot change any call edge:
///
/// ```text
///   db.type_inference()     16.30 ms
///   inlayHints, 1st          5.09 ms
///   inlayHints, 2nd          0.91 ms
/// ```
///
/// Pulling the inference aggregation first absorbs most of what the hint
/// call was being blamed for, and the hints then cost ~1 ms — which is what
/// they cost warm, and what they actually are. So the per-keystroke figure
/// is dominated by an inference layer that does not participate in the
/// per-knot incremental road at all: `raw_lowered_query` re-lowers one knot,
/// and then the call graph over every def in the project is rebuilt anyway.
///
/// This is the thing to fix. Viewport-scoping the hints or optimising the
/// walk both address the ~1 ms, not the ~16 ms.
///
/// (A second, independent cost sits in the walk: `collect_inferred_type_hint`
/// finds a temp's `SymbolInfo` with `analysis.index.symbols.values().find(…)`
/// — a linear scan of every symbol in the project, per temp hint. It is not
/// what this test measures, but it is why the residual first-call cost above
/// is 5 ms rather than 1.)
#[test]
#[ignore = "measurement, not an assertion: wall-clock numbers, run explicitly"]
fn what_the_first_pull_after_an_edit_pays_for() {
    const N: usize = 10;
    #[expect(clippy::cast_precision_loss, reason = "10 iterations")]
    let n = N as f64;

    let src = read(LARGE);
    let doc_len = u32::try_from(src.len()).expect("len fits");
    let mut session = EditorSession::new();
    session.set_perf_enabled(true);
    session.update_file("story.ink", &src);
    assert!(session.set_active_file("story.ink"));
    let doc = session.open_document("story.ink");

    let (mut first, mut second, mut third) = (0.0, 0.0, 0.0);
    for i in 0..N {
        let at = doc_len.saturating_add(u32::try_from(i).unwrap_or(0));
        let edits = format!("[{{\"from\":{at},\"to\":{at},\"insert\":\"x\"}}]");
        assert!(session.apply_edits_document(doc, &edits), "splice accepted");

        // Pull the type-inference aggregation FIRST, before any hint call.
        // If the hints' first-call cost is really the inference layer being
        // invalidated, this absorbs it and the hint calls all come back cheap.
        let t0 = crate::perf::now_ms();
        std::hint::black_box(session.session.db().type_inference());
        let t1 = crate::perf::now_ms();
        std::hint::black_box(session.inlay_hints_doc(doc, 0, doc_len));
        let t2 = crate::perf::now_ms();
        std::hint::black_box(session.inlay_hints_doc(doc, 0, doc_len));
        let t3 = crate::perf::now_ms();
        first += t1 - t0;
        second += t2 - t1;
        third += t3 - t2;
    }

    println!("\nafter ONE one-character edit at end of file (mean of {N}):");
    println!("  db.type_inference()   {:>7.2} ms", first / n);
    println!("  inlayHints, 1st       {:>7.2} ms", second / n);
    println!("  inlayHints, 2nd       {:>7.2} ms", third / n);
}

/// Does the FG-2.1 per-file firewall actually hold, and what does it leave?
///
/// `call_graph_query`'s doc says each `call_edges_query` "is validated
/// without re-executing unless *that specific def's* declaring file changed
/// — so an edit in file X only pays for X's own defs". If that holds, the
/// keystroke cost of `type_inference` should fall roughly linearly as the
/// same knots are spread over more files, because an edit then invalidates
/// only one file's share of the defs.
///
/// This builds the SAME set of knots as 1, 2, 4, 8 and 16 files (`INCLUDE`d
/// from an entry), edits ONE of them, and prices `db.type_inference()`.
#[test]
#[ignore = "measurement, not an assertion: wall-clock numbers, run explicitly"]
fn does_the_per_file_firewall_hold() {
    const KNOTS: usize = 240;
    const N: usize = 10;
    // A knot that calls the next one, so the call graph has real edges and
    // the SCC partition has something to do.
    fn knot(i: usize) -> String {
        format!(
            "=== knot_{i} ===\n\
             ~ temp local_{i} = {i}\n\
             The value is {{local_{i}}}.\n\
             {{ local_{i} > 0: -> knot_{next} | -> DONE }}\n",
            next = i + 1
        )
    }

    #[expect(clippy::cast_precision_loss, reason = "10 iterations")]
    let n = N as f64;
    println!("\n{KNOTS} knots, same content, spread over N files:");
    println!(
        "{:>7} {:>14} {:>16}",
        "files", "knots/file", "type_inference"
    );

    for files in [1_usize, 2, 4, 8, 16] {
        let per_file = KNOTS / files;
        let mut session = EditorSession::new();
        session.set_perf_enabled(true);

        let mut entry = String::new();
        for f in 0..files {
            let _ = writeln!(entry, "INCLUDE part{f}.ink");
        }
        entry.push_str("-> knot_0\n");

        for f in 0..files {
            let mut text = String::new();
            for k in (f * per_file)..((f + 1) * per_file) {
                text.push_str(&knot(k));
            }
            // The last knot diverts to a knot that does not exist; end it.
            session.update_file(&format!("part{f}.ink"), &text);
        }
        session.update_file("main.ink", &entry);
        assert!(session.set_active_file("main.ink"));
        let _ = session.session.db().type_inference();

        // Edit ONE file, repeatedly, and price the inference pull.
        let target = "part0.ink";
        let base = session
            .session
            .file_id(target)
            .and_then(|id| session.session.source(id).map(str::to_owned))
            .unwrap_or_default();
        let mut total = 0.0;
        for i in 0..N {
            let mut edited = base.clone();
            let _ = writeln!(edited, "// {}", "x".repeat(i + 1));
            session.update_file(target, &edited);
            let t0 = crate::perf::now_ms();
            std::hint::black_box(session.session.db().type_inference());
            total += crate::perf::now_ms() - t0;
        }
        println!("{files:>7} {per_file:>14} {:>13.2} ms", total / n);
    }
}

/// Within ONE file, how does the invalidation cost scale with the number of
/// knots? The per-file firewall (see `does_the_per_file_firewall_hold`)
/// means an edit pays for its own file's defs — this asks what that bill
/// looks like as the file grows, which is what decides whether narrowing the
/// firewall to per-segment is worth the work.
#[test]
#[ignore = "measurement, not an assertion: wall-clock numbers, run explicitly"]
fn within_one_file_how_does_invalidation_scale() {
    const N: usize = 10;
    fn knot(i: usize) -> String {
        format!(
            "=== knot_{i} ===\n\
             ~ temp local_{i} = {i}\n\
             The value is {{local_{i}}}.\n\
             {{ local_{i} > 0: -> knot_{next} | -> DONE }}\n",
            next = i + 1
        )
    }

    #[expect(clippy::cast_precision_loss, reason = "10 iterations")]
    let n = N as f64;
    println!("\none file, edited once, N knots in it:");
    println!("{:>7} {:>16} {:>16}", "knots", "type_inference", "per knot");
    for knots in [15_usize, 30, 60, 120, 240, 480] {
        let mut session = EditorSession::new();
        session.set_perf_enabled(true);
        let mut text = String::new();
        for k in 0..knots {
            text.push_str(&knot(k));
        }
        session.update_file("story.ink", &text);
        assert!(session.set_active_file("story.ink"));
        let _ = session.session.db().type_inference();

        let mut total = 0.0;
        for i in 0..N {
            let mut edited = text.clone();
            let _ = writeln!(edited, "// {}", "x".repeat(i + 1));
            session.update_file("story.ink", &edited);
            let t0 = crate::perf::now_ms();
            std::hint::black_box(session.session.db().type_inference());
            total += crate::perf::now_ms() - t0;
        }
        #[expect(clippy::cast_precision_loss, reason = "knot counts are small")]
        let k = knots as f64;
        println!(
            "{knots:>7} {:>13.2} ms {:>13.3} ms",
            total / n,
            total / n / k
        );
    }
}
