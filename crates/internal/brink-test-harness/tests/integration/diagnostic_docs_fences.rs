//! DD-1 (issue #2021): every `ink`/`brink` fence under `docs/diagnostics/*.md`
//! is compiled through the real compile path, and the page's own diagnostic
//! code is asserted present or absent per the fence's own tag.
//!
//! # The gap this closes
//!
//! `diagnostic_docs_validation.rs` only checks fence *tags* (a native-only
//! code's page must not use an `ink` fence) — it never compiles the fenced
//! source. `book_fences.rs` (BW-5) compile-checks fenced code, but only
//! under `docs/book/src/**`; `docs/diagnostics/*.md` is not part of the
//! mdBook (absent from `docs/book/src/SUMMARY.md`), so BW-5 never reaches
//! it either. Every `EXXX.md` explainer's minimal repro was therefore
//! unverified prose that could silently rot — stop parsing, stop producing
//! the diagnostic it claims to, or start producing a different one — with
//! nothing catching it. This file is that gate.
//!
//! # Placeholder pages were skipped; there are none left
//!
//! Issue #1623 (still open) is the *content* pass. It used to be visible
//! here as scaffold text: most `docs/diagnostics/*.md` files carried the
//! scaffold's literal placeholder in their `## Example` section
//! (`PLACEHOLDER_MARKER` below) rather than a real repro. This gate is
//! *enforcement*, not content, and issue #2021 rules that enforcement
//! should exist before or alongside that pass — so a placeholder page was
//! skipped wholesale rather than failing, which is why standing this gate
//! up needed no hand-editing of 150+ pages.
//!
//! **Those placeholders were all cleared in #3169**, so the skip path now
//! matches nothing and the census assertion is INVERTED: it requires zero
//! placeholder pages, and a non-zero count means someone re-introduced
//! scaffold text rather than writing (or leaving empty) a section. The skip
//! itself is kept so that reintroduction is caught rather than silently
//! failing a fence check.
//!
//! As #1623 fills a page in with a real repro, its fences must carry a
//! real DD-1 tag — the closed-taxonomy
//! check below (`Kind::Malformed` for a bare `ink`/`brink` fence) is exactly
//! what stops a freshly-written repro from landing untagged.
//!
//! # Fence taxonomy (DD-1 — the info string is the contract)
//!
//! Unlike BW-5's book taxonomy, a diagnostics-doc fence is never "just
//! narration" — every `ink`/`brink` fence on one of these pages exists to
//! either demonstrate the page's own code firing, or to contrast against it
//! (a fix, an adjacent shape that stays clean). So DD-1's taxonomy tags
//! *that*, not compile-success/failure the way BW-5's `ink,error(Exxx)`
//! does — a repro can be a hard compile error, a compile-clean warning, or
//! (as `E169`'s confinement check demonstrates) a warning-tier code that
//! happens to always co-occur with an unrelated hard error in a config-free
//! single-file compile. Checking "does the page's own code appear in the
//! diagnostics this fence produced, whichever way compilation came out"
//! covers all three uniformly, so DD-1 needs no error-vs-warning split at
//! the tag level the way BW-5 does.
//!
//! | Info string | Contract |
//! |---|---|
//! | ```` ```ink,fires(Exxx) ```` / ```` ```brink,fires(Exxx) ```` | compile the fence; the page's own code `Exxx` (redundant with the filename, checked for consistency) must appear somewhere in the diagnostics produced — as a `CompileOutput::warnings` entry on a clean compile, or inside `CompileError::Diagnostics` on a failed one |
//! | ```` ```ink,contrast ```` / ```` ```brink,contrast ```` | compile the fence; the page's own code must be **absent** from the diagnostics produced, regardless of whether the compile as a whole succeeded or failed for an unrelated reason |
//! | ```` ```ink,ignore ```` / ```` ```brink,ignore ```` | skipped — a fragment/pseudocode illustration, not a standalone program (mirrors BW-5's identical marker) |
//! | ```` ```ink,skip(reason) ```` / ```` ```brink,skip(reason) ```` | skipped, with a mandatory non-empty justification — for a repro this harness genuinely cannot represent as one in-memory fence: multi-file project config (`brink.toml`, e.g. `E169`'s conventions-module confinement) or a host-capability manifest (`E164`/`E165`/`E173`'s markup vocabulary). Every skip is enumerated in the test's own failure-message-free summary output so a reviewer can audit the list; issue #2021 explicitly allows this over building a multi-file/manifest fence convention this issue never asked for. |
//!
//! Any other `ink…`/`brink…`-flavored info string — including a **bare**
//! `ink`/`brink` fence with no tag at all — is a test failure: the taxonomy
//! is closed on purpose, exactly like BW-5's, so a freshly-added repro can
//! never silently land unchecked. A `text`/`jsonc`/`toml`/untagged fence
//! (a diagnostic-message dump, a host-manifest JSON illustration, a
//! `brink.toml` snippet) is not `ink`/`brink`-prefixed at all and is simply
//! not part of what this gate compiles — DD-1 only ever asserts about
//! `ink`/`brink` fences.
//!
//! `docs/diagnostics/*.md` fences are retagged this way, not left bare —
//! this is the "minimal doc churn" marking convention issue #2021 asks for
//! when none already exists (there was none: every real-content page's
//! `Example`/`Fix` fences carried a bare `ink`/`brink` tag before this PR).
//! The convention is also recorded in `docs/compiler-spec.md`'s "Diagnostic
//! Codes" section, alongside the doc-file-existence rule
//! `diagnostic_docs_validation.rs` already documents there.
//!
//! # Execution markers
//!
//! Reuses `brink_test_harness::fence`'s `<!-- fence: … -->` marker comment
//! (shared with BW-5 — see that module's doc). DD-1 additionally consults
//! `types=strict` / `types=gradual` (`Markers::types`) for the handful of
//! codes gated by TM-3's effective type policy (e.g. `E174`, which only
//! fires under `types = strict` and would never fire under a native
//! project's ungated default) — a docs/diagnostics fence compiles
//! standalone with no `brink.toml` to derive an effective policy from, so a
//! fence that needs one names it explicitly. `seed=`/`choices=` are BW-5-only
//! (no diagnostics-doc fence runs a story); `compile-only` is meaningless
//! here (every fence here is compile-only by construction) and unused.
//!
//! # Native (`brink`) fences compile through the real production driver
//!
//! An `ink`-tagged fence compiles in-memory, exactly like BW-5's `compile()`
//! (`brink_compiler::compile_with_options("main.ink", …, Dialect::Brink)`).
//! A `brink`-tagged fence needs the real native discovery path
//! (`brink_driver::Driver::discover_native`, which walks a real `RealFs`
//! root) — so it is written to a fresh, single-file scratch directory
//! (`ScratchDir`, mirroring `corpus.rs`'s own `ScratchFile` precedent: no
//! new `tempfile` dependency) and compiled from disk via the same
//! `brink_compiler::compile_with_options` production entry point, dispatched
//! to the native driver arm by its own `.brink` extension
//! (`brink_driver::is_native`) — the identical dispatch a real `brink
//! compile scene.brink` CLI invocation goes through.
//!
//! # Guardrails
//!
//! No fence here ever runs a story (`Story::continue_single` et al.) — every
//! check is compile-only, so there is no VM step budget to bound and no
//! infinite-output hazard BW-5's `run_story` guards against. A census
//! assertion at the bottom keeps the extractor honest the same way BW-5's
//! does.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use brink_compiler::{AnalysisOptions, CompileError, DiagnosticCode, Dialect, TypePolicy};
use brink_test_harness::fence::{collect_markdown, extract_fences};

fn docs_diagnostics_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("docs")
        .join("diagnostics")
}

/// The literal placeholder text `docs/diagnostics/*.md`'s scaffold ships in
/// every not-yet-written `## Example` section (issue #1623). A whole file
/// carrying this text is skipped wholesale — see the module doc's
/// "Placeholder pages are skipped, not tagged" section.
const PLACEHOLDER_MARKER: &str = "[Minimal example demonstrating this diagnostic]";

/// What a DD-1 fence tag commits its fence to. See the module doc's fence
/// taxonomy table.
enum Kind {
    /// `ink,fires(Exxx)` / `brink,fires(Exxx)` — `Exxx` must appear in the
    /// diagnostics this fence's compile produces. `native` selects the
    /// compile path (`brink`-tagged fences discover via the real native
    /// driver from a scratch file; `ink`-tagged fences compile in-memory).
    Fires { code: String, native: bool },
    /// `ink,contrast` / `brink,contrast` — the page's own code must be
    /// absent from the diagnostics this fence's compile produces.
    Contrast { native: bool },
    /// `ink,ignore` / `brink,ignore` — a fragment/pseudocode illustration.
    Ignore,
    /// `ink,skip(reason)` / `brink,skip(reason)` — cannot be represented as
    /// one in-memory fence (multi-file project config, a host-capability
    /// manifest); the reason is enumerated in the test's summary output.
    SkipReason(String),
    /// Not an `ink`/`brink` fence at all (`text`, `jsonc`, `toml`, a bare
    /// diagnostic-message dump with no info string) — outside what this
    /// gate compiles.
    Other,
    /// An `ink…`/`brink…`-flavored info string outside the closed taxonomy
    /// above, including a bare `ink`/`brink` fence with no tag.
    Malformed(String),
}

fn valid_code_shape(code: &str) -> bool {
    code.len() == 4 && code.starts_with('E') && code[1..].chars().all(|c| c.is_ascii_digit())
}

fn classify(info: &str) -> Kind {
    let (native, rest) = if let Some(r) = info.strip_prefix("brink") {
        (true, r)
    } else if let Some(r) = info.strip_prefix("ink") {
        (false, r)
    } else {
        return Kind::Other;
    };
    let Some(tag) = rest.strip_prefix(',') else {
        return if rest.is_empty() {
            Kind::Malformed(format!(
                "bare `{info}` fence — the docs/diagnostics taxonomy is closed: tag every \
                 ink/brink fence `,fires(Exxx)`, `,contrast`, `,ignore`, or `,skip(reason)`"
            ))
        } else {
            // Some other info string that merely happens to start with
            // "ink"/"brink" as a text prefix (none exist in practice —
            // guarded rather than assumed).
            Kind::Other
        };
    };
    if tag == "ignore" {
        return Kind::Ignore;
    }
    if tag == "contrast" {
        return Kind::Contrast { native };
    }
    if let Some(code) = tag.strip_prefix("fires(").and_then(|s| s.strip_suffix(')')) {
        return if valid_code_shape(code) {
            Kind::Fires {
                code: code.to_owned(),
                native,
            }
        } else {
            Kind::Malformed(format!(
                "malformed diagnostic code `{code}` in `fires(...)`"
            ))
        };
    }
    if let Some(reason) = tag.strip_prefix("skip(").and_then(|s| s.strip_suffix(')')) {
        return if reason.trim().is_empty() {
            Kind::Malformed("`skip(...)` requires a non-empty justification".to_owned())
        } else {
            Kind::SkipReason(reason.to_owned())
        };
    }
    Kind::Malformed(format!("unrecognized docs/diagnostics fence tag `{info}`"))
}

/// A unique, single-file scratch directory for one native fence compile —
/// `discover_native` walks the whole directory, so each fence needs its own
/// (no cross-fence file leakage) and no `brink.toml` anywhere in its
/// ancestry (so `native_source_root_with_warnings` resolves the root to the
/// scratch directory itself, the single-file-project default). Mirrors
/// `corpus.rs`'s own `ScratchFile` precedent: `std::env::temp_dir()` rather
/// than a new `tempfile` dependency (not already a workspace dep).
struct ScratchDir(PathBuf);

static NEXT_SCRATCH_ID: AtomicU64 = AtomicU64::new(0);

impl ScratchDir {
    fn new() -> Result<Self, String> {
        let id = NEXT_SCRATCH_ID.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "brink-diagnostic-docs-fences-{}-{id}",
            std::process::id(),
        ));
        std::fs::create_dir_all(&dir).map_err(|e| format!("create scratch dir: {e}"))?;
        Ok(Self(dir))
    }

    fn write_brink(&self, body: &str) -> Result<PathBuf, String> {
        let path = self.0.join("main.brink");
        std::fs::write(&path, body).map_err(|e| format!("write scratch main.brink: {e}"))?;
        Ok(path)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Compile an `ink`-tagged fence in-memory, always under `Dialect::Brink`
/// (matching BW-5's `compile()` — the dialect default every `.brink`
/// project runs under, whether the fence's own surface syntax is
/// ink-compatible or not).
fn compile_ink(body: &str, types: Option<TypePolicy>) -> Result<Vec<DiagnosticCode>, String> {
    let opts = AnalysisOptions {
        dialect: Dialect::Brink,
        types,
        ..AnalysisOptions::default()
    };
    combine_diagnostics(brink_compiler::compile_with_options(
        "main.ink",
        |_| Ok(body.to_owned()),
        opts,
    ))
}

/// Compile a `brink`-tagged fence through the real production driver, from
/// a fresh single-file scratch directory (see [`ScratchDir`]). Dialect is
/// irrelevant for a native entry — `is_native` (derived from the `.brink`
/// extension) bypasses the ink-only T1b dialect gate entirely — so this
/// leaves `AnalysisOptions::dialect` at its default and only ever threads
/// through `types`, for the handful of codes gated by TM-3's effective type
/// policy.
fn compile_brink(body: &str, types: Option<TypePolicy>) -> Result<Vec<DiagnosticCode>, String> {
    let scratch = ScratchDir::new()?;
    let path = scratch.write_brink(body)?;
    let opts = AnalysisOptions {
        types,
        ..AnalysisOptions::default()
    };
    let entry = path.to_string_lossy().into_owned();
    combine_diagnostics(brink_compiler::compile_with_options(
        &entry,
        |p| {
            std::fs::read_to_string(p)
                .map_err(|e| std::io::Error::new(e.kind(), format!("{p}: {e}")))
        },
        opts,
    ))
}

/// Flatten a compile result into the codes it produced either way — a clean
/// compile's warnings, or a failed compile's merged errors+warnings — since
/// DD-1's contract only ever asks "does the page's own code appear",
/// regardless of which side of `Result` it landed on (see the module doc's
/// fence taxonomy section for why this is sound: a warning-tier code that
/// always co-occurs with an unrelated hard error, `E169` alongside a
/// standalone `@[convention]` handler being the concrete example, must not
/// need a `warns`-vs-`error` tag distinction the compiler's own severity
/// table doesn't stably support at fence granularity).
fn combine_diagnostics(
    result: Result<brink_compiler::CompileOutput, CompileError>,
) -> Result<Vec<DiagnosticCode>, String> {
    match result {
        Ok(out) => Ok(out.warnings.into_iter().map(|d| d.code).collect()),
        Err(CompileError::Diagnostics(diags)) => Ok(diags.into_iter().map(|d| d.code).collect()),
        Err(other) => Err(format!("unexpected compile error shape: {other:?}")),
    }
}

fn compile_fence(
    body: &str,
    native: bool,
    types: Option<TypePolicy>,
) -> Result<Vec<DiagnosticCode>, String> {
    if native {
        compile_brink(body, types)
    } else {
        compile_ink(body, types)
    }
}

#[test]
fn every_fence_in_docs_diagnostics_checks_out() {
    let files = collect_markdown(&docs_diagnostics_dir()).expect("walk docs/diagnostics");
    assert!(
        !files.is_empty(),
        "no markdown files under docs/diagnostics"
    );

    let mut errors: Vec<String> = Vec::new();
    let mut skip_log: Vec<String> = Vec::new();
    let (mut fired, mut contrasted, mut ignored, mut skipped, mut placeholder_files) =
        (0usize, 0usize, 0usize, 0usize, 0usize);

    for path in &files {
        let markdown = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let rel = path
            .strip_prefix(docs_diagnostics_dir())
            .unwrap_or(path)
            .display()
            .to_string();

        if markdown.contains(PLACEHOLDER_MARKER) {
            placeholder_files += 1;
            continue;
        }

        let Some(page_code) = path.file_stem().and_then(|s| s.to_str()) else {
            errors.push(format!("{rel}: non-UTF-8 filename"));
            continue;
        };
        if !valid_code_shape(page_code) {
            // Not an `Exxx.md` page (e.g. a README) — nothing to check here;
            // `diagnostic_docs_validation.rs` owns the file-naming contract.
            continue;
        }

        let mut extract_errors = Vec::new();
        let fences = extract_fences(&markdown, &mut extract_errors, &rel);
        errors.extend(extract_errors);

        for fence in &fences {
            let at = format!("{rel}:{}", fence.line);
            let types = fence.markers.types;
            match classify(&fence.info) {
                Kind::Fires { code, native } => {
                    fired += 1;
                    if code != page_code {
                        errors.push(format!(
                            "{at}: fence tagged `fires({code})` on {page_code}'s own page — the \
                             tagged code must match the page (a page's fences only ever \
                             demonstrate its own diagnostic)"
                        ));
                        continue;
                    }
                    match compile_fence(&fence.body, native, types) {
                        Ok(codes) => {
                            if !codes.iter().any(|c| c.as_str() == code) {
                                errors.push(format!(
                                    "{at}: fence tagged `fires({code})` but {code} did not \
                                     appear in the diagnostics produced — got {codes:?}\n\
                                     --- source ---\n{}",
                                    fence.body
                                ));
                            }
                        }
                        Err(e) => errors.push(format!("{at}: {e}\n--- source ---\n{}", fence.body)),
                    }
                }
                Kind::Contrast { native } => {
                    contrasted += 1;
                    match compile_fence(&fence.body, native, types) {
                        Ok(codes) => {
                            if codes.iter().any(|c| c.as_str() == page_code) {
                                errors.push(format!(
                                    "{at}: fence tagged `contrast` but {page_code} appeared in \
                                     the diagnostics produced anyway — got {codes:?}\n\
                                     --- source ---\n{}",
                                    fence.body
                                ));
                            }
                        }
                        Err(e) => errors.push(format!("{at}: {e}\n--- source ---\n{}", fence.body)),
                    }
                }
                Kind::Ignore => ignored += 1,
                Kind::SkipReason(reason) => {
                    skipped += 1;
                    skip_log.push(format!("{at}: {reason}"));
                }
                Kind::Malformed(msg) => errors.push(format!("{at}: {msg}")),
                Kind::Other => {}
            }
        }
    }

    // Every skip is named in the test's own output (via `--nocapture`, or
    // in the failure message below on a hard failure) so a reviewer can
    // audit the list without grepping the tree by hand.
    if !skip_log.is_empty() {
        println!("DD-1 skip(reason) fences ({}):", skip_log.len());
        for line in &skip_log {
            println!("  {line}");
        }
    }

    assert!(
        errors.is_empty(),
        "{} docs/diagnostics fence failure(s):\n\n{}",
        errors.len(),
        errors
            .iter()
            .enumerate()
            .fold(String::new(), |mut s, (n, e)| {
                let _ = write!(s, "[{}] {e}\n\n", n + 1);
                s
            })
    );

    // The scaffold placeholders are GONE (#3169) — all 157 stub pages were
    // cleared, plus 13 more that had the placeholder sitting in front of
    // prose someone had since written around it. So this count is now an
    // inverted guard: it must stay at zero, and a non-zero value means
    // someone re-introduced scaffold text rather than writing a page.
    //
    // It used to assert `>= 100`, because the skip path was load-bearing
    // while the backlog existed. Keeping that floor would now require the
    // backlog to come BACK.
    assert_eq!(
        placeholder_files, 0,
        "scaffold placeholder text is back in {placeholder_files} page(s) — write the \
         section or leave it empty, but do not ship `[Detailed explanation…]` to authors"
    );

    // Census guard: if the extractor ever silently found far fewer fences,
    // that is a regression in this file, not a lighter set of docs. The
    // floors went UP when the placeholders were cleared — those pages stop
    // being skipped wholesale, so every fence on them is checked now.
    let checked = fired + contrasted;
    assert!(
        checked >= 50 && fired >= 30 && contrasted >= 20,
        "fence census too small — extractor regression? \
         fired={fired} contrasted={contrasted} ignored={ignored} skipped={skipped}"
    );
}

#[test]
fn classify_closes_the_taxonomy() {
    assert!(matches!(
        classify("ink,fires(E035)"),
        Kind::Fires { native: false, .. }
    ));
    assert!(matches!(
        classify("brink,fires(E156)"),
        Kind::Fires { native: true, .. }
    ));
    assert!(matches!(
        classify("ink,contrast"),
        Kind::Contrast { native: false }
    ));
    assert!(matches!(
        classify("brink,contrast"),
        Kind::Contrast { native: true }
    ));
    assert!(matches!(classify("ink,ignore"), Kind::Ignore));
    assert!(matches!(classify("brink,ignore"), Kind::Ignore));
    assert!(matches!(
        classify("brink,skip(needs brink.toml)"),
        Kind::SkipReason(_)
    ));
    assert!(matches!(classify("text"), Kind::Other));
    assert!(matches!(classify("jsonc"), Kind::Other));
    assert!(matches!(classify("toml"), Kind::Other));
    assert!(matches!(classify(""), Kind::Other));
    assert!(matches!(classify("ink"), Kind::Malformed(_)));
    assert!(matches!(classify("brink"), Kind::Malformed(_)));
    assert!(matches!(classify("ink,fires(63)"), Kind::Malformed(_)));
    assert!(matches!(classify("brink,skip()"), Kind::Malformed(_)));
    assert!(matches!(classify("ink,bogus"), Kind::Malformed(_)));
}
