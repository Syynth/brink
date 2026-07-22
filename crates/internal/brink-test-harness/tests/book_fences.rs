//! Book fence walker (BW-5, maintainer ruling 2026-07-19): every fenced
//! ```` ```ink ```` code block anywhere under `docs/book/src/**/*.md` must
//! compile under the brink dialect (strict types — the dialect default) and
//! behave exactly as the chapter's prose claims. This generalizes — and
//! replaces — the per-chapter `book_*_examples.rs` tests (decision-log
//! 2026-07-10: "Book code examples must be compile-checked").
//!
//! # Fence taxonomy (the info string is the contract)
//!
//! | Info string | Contract |
//! |---|---|
//! | ```` ```ink ```` | must compile with zero errors and run to completion; if a ```` ```text ```` fence immediately follows, the story's concatenated output must match it byte-for-byte (both sides `trim_end`ed) |
//! | ```` ```ink,error(Exxx) ```` | must FAIL to compile, and the diagnostics must contain code `Exxx` (BW-2 ruling: the expected code lives in the info string) |
//! | ```` ```ink,error ```` | legacy spelling — always a test failure: migrate to `ink,error(Exxx)` |
//! | ```` ```ink,ignore ```` | skipped — a fragment/pseudocode illustration that is not a standalone program (mirrors this book's `rust,ignore` convention) |
//! | ```` ```ink,proposed ```` / ```` ```brink ```` / ```` ```brink,proposed ```` | skipped — unruled future syntax, never compiled |
//!
//! Any other `ink…`-flavored info string is a test failure — the taxonomy is
//! closed on purpose, so a typo can never silently skip verification.
//!
//! # Execution markers
//!
//! An HTML comment on the line directly above the opening fence (one blank
//! line in between is allowed) tunes how a runnable fence executes:
//!
//! ```text
//! <!-- fence: seed=42 -->
//! <!-- fence: choices=1,2 -->
//! <!-- fence: compile-only -->
//! ```
//!
//! - `seed=N` — `Story::set_rng_seed(N)` before driving, so a fence that
//!   draws randomness is deterministic (the tier1-brink corpus instead seeds
//!   in-source with `~ seed(N)`, which needs no marker — prefer that when the
//!   seed is part of the example).
//! - `choices=1,2,…` — at each `Line::Choices`, pick the next number
//!   (1-based, as a player would read them). Running out of picks, or ending
//!   with picks unused, is a failure.
//! - `compile-only` — the fence must compile but is not run (for programs
//!   whose point is a compile-time warning and which don't form a complete
//!   story).
//!
//! Multiple keys may share one comment: `<!-- fence: seed=7 choices=1 -->`.
//!
//! # Guardrails
//!
//! Every execution is bounded (`STEP_LIMIT` lines across `continue_single`
//! calls; each call is itself under the VM step budget), so a divergent
//! example can never hang CI. A census assertion at the bottom keeps the
//! extractor honest — if a refactor made the walker silently find nothing,
//! the count trips.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use brink_compiler::{AnalysisOptions, Dialect};
use brink_runtime::{DotNetRng, Line, Story};

fn book_src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("docs")
        .join("book")
        .join("src")
}

/// Recursively collect every `.md` file under `dir`, sorted by path for
/// deterministic report order.
fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_markdown(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            out.push(path);
        }
    }
}

/// Execution markers parsed from a `<!-- fence: … -->` comment.
#[derive(Default)]
struct Markers {
    seed: Option<i32>,
    /// 1-based choice picks, in order.
    choices: Vec<usize>,
    compile_only: bool,
}

/// One fenced code block: info string, body, source line, and any markers.
struct Fence {
    /// 1-based line number of the opening fence, for failure reports.
    line: usize,
    /// The info string (`ink`, `ink,error(E063)`, `text`, …), trimmed.
    info: String,
    body: String,
    markers: Markers,
}

fn parse_markers(comment: &str, errors: &mut Vec<String>, at: &str) -> Markers {
    let mut m = Markers::default();
    let inner = comment
        .trim()
        .strip_prefix("<!-- fence:")
        .and_then(|s| s.strip_suffix("-->"))
        .unwrap_or_default();
    for token in inner.split_whitespace() {
        if token == "compile-only" {
            m.compile_only = true;
        } else if let Some(v) = token.strip_prefix("seed=") {
            match v.parse::<i32>() {
                Ok(n) => m.seed = Some(n),
                Err(_) => errors.push(format!("{at}: bad seed marker `{token}`")),
            }
        } else if let Some(v) = token.strip_prefix("choices=") {
            for pick in v.split(',') {
                match pick.parse::<usize>() {
                    Ok(n) if n >= 1 => m.choices.push(n),
                    _ => errors.push(format!(
                        "{at}: bad choices marker `{token}` (picks are 1-based integers)"
                    )),
                }
            }
        } else {
            errors.push(format!("{at}: unknown fence marker `{token}`"));
        }
    }
    m
}

/// Extract every fenced code block in document order. Handles fences opened
/// at an indent (list-item fences): the opening line's leading whitespace is
/// stripped from each body line, and the closing fence is a bare
/// triple-backtick at any indent. A `<!-- fence: … -->` comment directly
/// above the opening fence (one blank line allowed) attaches markers.
fn extract_fences(markdown: &str, errors: &mut Vec<String>, file: &str) -> Vec<Fence> {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut fences = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim_start();
        let Some(info) = trimmed.strip_prefix("```") else {
            i += 1;
            continue;
        };
        let open_line = i + 1;
        let indent = lines[i].len() - trimmed.len();
        let mut body = String::new();
        i += 1;
        while i < lines.len() {
            if lines[i].trim() == "```" {
                i += 1;
                break;
            }
            let stripped = if lines[i].len() >= indent && lines[i][..indent].trim().is_empty() {
                &lines[i][indent..]
            } else {
                lines[i].trim_start()
            };
            body.push_str(stripped);
            body.push('\n');
            i += 1;
        }
        // Look upward for a marker comment: the line above the fence, or one
        // above a single blank line.
        let mut markers = Markers::default();
        let mut probe = open_line.checked_sub(2);
        if let Some(p) = probe
            && lines[p].trim().is_empty()
        {
            probe = p.checked_sub(1);
        }
        if let Some(p) = probe
            && lines[p].trim().starts_with("<!-- fence:")
        {
            markers = parse_markers(lines[p], errors, &format!("{file}:{}", p + 1));
        }
        fences.push(Fence {
            line: open_line,
            info: info.trim().to_owned(),
            body,
            markers,
        });
    }
    fences
}

fn brink_opts() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    }
}

fn compile(src: &str) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    brink_compiler::compile_with_options("main.ink", |_| Ok(src.to_owned()), brink_opts())
}

/// Maximum `continue_single` calls per fence before aborting — bounds the
/// outer drive loop across calls (each call is itself under the VM's own
/// step budget). Matches the harness's `explorer.rs` convention.
const STEP_LIMIT: usize = 10_000;

/// Drive a compiled fence to completion, honoring seed/choice markers.
/// Returns the concatenated output, or a failure description.
fn run_story(src: &str, markers: &Markers) -> Result<String, String> {
    let output = match compile(src) {
        Ok(out) => out,
        Err(e) => return Err(format!("failed to compile: {e:?}")),
    };
    let (program, line_tables) =
        brink_runtime::link(&output.data).map_err(|e| format!("failed to link: {e:?}"))?;
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    if let Some(seed) = markers.seed {
        story.set_rng_seed(seed);
    }
    let mut picks = markers.choices.iter().copied();
    let mut out = String::new();
    for _ in 0..STEP_LIMIT {
        match story.continue_single() {
            Ok(Line::Text { text, .. }) => out.push_str(&text),
            Ok(Line::Done { text, .. } | Line::End { text, .. } | Line::Suspended { text, .. }) => {
                out.push_str(&text);
                let unused = picks.count();
                if unused > 0 {
                    return Err(format!(
                        "story ended with {unused} unused pick(s) from the choices marker"
                    ));
                }
                return Ok(out);
            }
            Ok(Line::Choices { text, choices, .. }) => {
                out.push_str(&text);
                let Some(pick) = picks.next() else {
                    return Err("story presented choices — linear fences must not; add a \
                         `<!-- fence: choices=… -->` marker with 1-based picks"
                        .to_owned());
                };
                if pick > choices.len() {
                    return Err(format!(
                        "choices marker pick {pick} out of range (story offered {})",
                        choices.len()
                    ));
                }
                if let Err(e) = story.choose(pick - 1) {
                    return Err(format!("choose({pick}) failed: {e:?}"));
                }
            }
            Err(e) => return Err(format!("runtime fault: {e:?}")),
        }
    }
    Err(format!(
        "exceeded {STEP_LIMIT} lines without completing (must be bounded)"
    ))
}

/// The diagnostic codes a failing compile produced (empty when it compiled).
fn error_codes(src: &str) -> Result<Vec<&'static str>, String> {
    match compile(src) {
        Ok(_) => Ok(Vec::new()),
        Err(brink_compiler::CompileError::Diagnostics(diags)) => {
            Ok(diags.iter().map(|d| d.code.as_str()).collect())
        }
        Err(other) => Err(format!("unexpected compile error shape: {other:?}")),
    }
}

/// What the info string commits a fence to.
enum Kind {
    /// ```` ```ink ```` — compile + run (+ compare against a paired text fence).
    Run,
    /// ```` ```ink,error(Exxx) ```` — must fail to compile with this code.
    Error(String),
    /// Skipped by convention (`ink,ignore`, `ink,proposed`, `brink,proposed`).
    Skip,
    /// Not an ink fence at all (`text`, `rust`, `sh`, …).
    Other,
    /// An ink-flavored info string outside the closed taxonomy.
    Malformed(String),
}

fn classify(info: &str) -> Kind {
    match info {
        "ink" => Kind::Run,
        "ink,ignore" | "ink,proposed" | "brink" | "brink,proposed" => Kind::Skip,
        "ink,error" => Kind::Malformed(
            "legacy `ink,error` fence — migrate to `ink,error(Exxx)` (BW-2 ruling: \
             the expected diagnostic code lives in the info string)"
                .to_owned(),
        ),
        _ => {
            if let Some(code) = info
                .strip_prefix("ink,error(")
                .and_then(|s| s.strip_suffix(')'))
            {
                if code.len() == 4
                    && code.starts_with('E')
                    && code[1..].chars().all(|c| c.is_ascii_digit())
                {
                    Kind::Error(code.to_owned())
                } else {
                    Kind::Malformed(format!("malformed diagnostic code `{code}` in info string"))
                }
            } else if info.starts_with("ink") || info.starts_with("brink") {
                Kind::Malformed(format!("unrecognized ink fence info string `{info}`"))
            } else {
                Kind::Other
            }
        }
    }
}

#[test]
fn every_ink_fence_in_the_book_checks_out() {
    let mut files = Vec::new();
    collect_markdown(&book_src_dir(), &mut files);
    assert!(!files.is_empty(), "no markdown files under docs/book/src");

    let mut errors: Vec<String> = Vec::new();
    let (mut ran, mut compared, mut errored, mut skipped) = (0usize, 0usize, 0usize, 0usize);

    for path in &files {
        let markdown = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let rel = path
            .strip_prefix(book_src_dir())
            .unwrap_or(path)
            .display()
            .to_string();
        let fences = extract_fences(&markdown, &mut errors, &rel);
        for (idx, fence) in fences.iter().enumerate() {
            let at = format!("{rel}:{}", fence.line);
            match classify(&fence.info) {
                Kind::Run => {
                    if fence.markers.compile_only {
                        ran += 1;
                        if let Err(e) = compile(&fence.body) {
                            errors.push(format!(
                                "{at}: compile-only fence failed to compile: {e:?}\n\
                                 --- source ---\n{}",
                                fence.body
                            ));
                        }
                        continue;
                    }
                    ran += 1;
                    match run_story(&fence.body, &fence.markers) {
                        Ok(actual) => {
                            if let Some(next) = fences.get(idx + 1)
                                && next.info == "text"
                            {
                                compared += 1;
                                if actual.trim_end() != next.body.trim_end() {
                                    errors.push(format!(
                                        "{at}: output mismatch\n--- expected ({rel}:{}) ---\n{}\n\
                                         --- actual ---\n{}\n--- source ---\n{}",
                                        next.line,
                                        next.body.trim_end(),
                                        actual.trim_end(),
                                        fence.body
                                    ));
                                }
                            }
                        }
                        Err(e) => errors.push(format!("{at}: {e}\n--- source ---\n{}", fence.body)),
                    }
                }
                Kind::Error(code) => {
                    errored += 1;
                    match error_codes(&fence.body) {
                        Ok(codes) if codes.is_empty() => errors.push(format!(
                            "{at}: `ink,error({code})` fence compiled cleanly — it must fail\n\
                             --- source ---\n{}",
                            fence.body
                        )),
                        Ok(codes) if !codes.iter().any(|c| *c == code) => errors.push(format!(
                            "{at}: expected diagnostic {code}, got {codes:?}\n--- source ---\n{}",
                            fence.body
                        )),
                        Ok(_) => {}
                        Err(e) => errors.push(format!("{at}: {e}")),
                    }
                }
                Kind::Skip => skipped += 1,
                Kind::Malformed(msg) => errors.push(format!("{at}: {msg}")),
                Kind::Other => {}
            }
        }
    }

    assert!(
        errors.is_empty(),
        "{} book fence failure(s):\n\n{}",
        errors.len(),
        errors
            .iter()
            .enumerate()
            .fold(String::new(), |mut s, (n, e)| {
                let _ = write!(s, "[{}] {e}\n\n", n + 1);
                s
            })
    );

    // Census guard: the six dialect chapters alone carry ~60 ink fences. If
    // the extractor ever silently finds (or verifies) far fewer, that is an
    // extraction bug, not a lighter book.
    let verified = ran + errored;
    assert!(
        verified >= 60 && compared >= 35 && errored >= 15,
        "fence census too small — extractor regression? \
         ran={ran} compared={compared} errored={errored} skipped={skipped}"
    );
}

// ── walker self-checks ───────────────────────────────────────────────────

#[test]
fn run_story_bounds_the_outer_drive_loop_on_infinite_output() {
    // A knot that unconditionally diverts to itself emits one line per
    // `continue_single` call forever — each call is under the VM step
    // budget, so nothing faults, but the outer loop must still be capped or
    // a non-terminating example added to a chapter would hang CI.
    let source = "-> loop\n=== loop ===\nx\n-> loop\n";
    let err = run_story(source, &Markers::default()).expect_err("must hit the outer cap");
    assert!(err.contains("exceeded"), "unexpected error: {err}");
}

#[test]
fn classify_closes_the_taxonomy() {
    assert!(matches!(classify("ink"), Kind::Run));
    assert!(matches!(classify("ink,ignore"), Kind::Skip));
    assert!(matches!(classify("ink,proposed"), Kind::Skip));
    assert!(matches!(classify("brink"), Kind::Skip));
    assert!(matches!(classify("brink,proposed"), Kind::Skip));
    assert!(matches!(classify("text"), Kind::Other));
    assert!(matches!(classify("rust,ignore"), Kind::Other));
    assert!(matches!(classify(""), Kind::Other));
    let Kind::Error(code) = classify("ink,error(E063)") else {
        panic!("ink,error(E063) must classify as Error");
    };
    assert_eq!(code, "E063");
    assert!(matches!(classify("ink,error"), Kind::Malformed(_)));
    assert!(matches!(classify("ink,error(63)"), Kind::Malformed(_)));
    assert!(matches!(classify("ink,eror(E063)"), Kind::Malformed(_)));
}
