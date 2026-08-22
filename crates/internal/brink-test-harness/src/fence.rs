//! Shared markdown fence-extraction machinery (issue #2021).
//!
//! Originally lived only in `tests/book_fences.rs` (BW-5); split out here so
//! a second walker — `tests/diagnostic_docs_fences.rs` (DD-1, docs/diagnostics/*.md)
//! — can reuse the exact same extractor rather than re-implementing fence
//! parsing a second time. Both consumers define their own fence-tag taxonomy
//! (`Kind` enum + `classify`) locally, since BW-5's book taxonomy and DD-1's
//! diagnostics-doc taxonomy mean different things by the same info string in
//! places (`ink` alone is meaningful prose narration in the book; it is a
//! closed-taxonomy violation in a diagnostics doc, which always tags whether
//! a fence fires or contrasts). Only the mechanical parts — walking markdown
//! files, splitting fenced blocks out of one file's text, and parsing the
//! `<!-- fence: … -->` marker comment — are shared.

use std::path::{Path, PathBuf};

use brink_compiler::TypePolicy;
use brink_source_tree::Walk;

/// Recursively collect every `.md` file under `dir`, sorted by path for
/// deterministic report order. Routed through the shared [`Walk`] (issue
/// #1433) — `brink-test-harness` already depends on `brink-source-tree` for
/// corpus/bench case discovery, so there is no new dependency to justify
/// here.
///
/// Returns `Err` on a walk failure rather than panicking — this module is
/// ordinary library code (`src/fence.rs`), not a `tests/*.rs` integration
/// test crate, so it gets none of `clippy.toml`'s "in tests" carve-outs and
/// `clippy::panic` has no carve-out anywhere in this workspace (see
/// `CLAUDE.md`'s Rules section). Callers (always test files) `.expect()` the
/// result, which test code may do freely.
pub fn collect_markdown(dir: &Path) -> Result<Vec<PathBuf>, String> {
    Walk::new(dir)
        .map(|entry| entry.map_err(|e| format!("walk {}: {e}", dir.display())))
        .filter(|entry| {
            entry.as_ref().is_ok_and(|entry| {
                entry.is_file() && entry.path().extension().is_some_and(|ext| ext == "md")
            })
        })
        .map(|entry| entry.map(brink_source_tree::WalkEntry::into_path))
        .collect()
}

/// Execution markers parsed from a `<!-- fence: … -->` comment.
///
/// `types` (issue #2021, DD-1) is consulted only by `diagnostic_docs_fences.rs`
/// — a fence whose diagnostic is gated by TM-3's effective type policy (e.g.
/// `E174`, strict-only) names the policy it needs explicitly, since a
/// docs/diagnostics fence compiles standalone with no project config to
/// derive one from. `book_fences.rs` never sets this marker, so it stays
/// `None` for every book fence.
#[derive(Default)]
pub struct Markers {
    pub seed: Option<i32>,
    /// 1-based choice picks, in order.
    pub choices: Vec<usize>,
    pub compile_only: bool,
    pub types: Option<TypePolicy>,
}

/// One fenced code block: info string, body, source line, and any markers.
pub struct Fence {
    /// 1-based line number of the opening fence, for failure reports.
    pub line: usize,
    /// The info string (`ink`, `ink,error(E063)`, `text`, …), trimmed.
    pub info: String,
    pub body: String,
    pub markers: Markers,
}

pub fn parse_markers(comment: &str, errors: &mut Vec<String>, at: &str) -> Markers {
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
        } else if let Some(v) = token.strip_prefix("types=") {
            match v {
                "strict" => m.types = Some(TypePolicy::Strict),
                "gradual" => m.types = Some(TypePolicy::Gradual),
                _ => errors.push(format!(
                    "{at}: bad types marker `{token}` (expected `types=strict` or `types=gradual`)"
                )),
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
pub fn extract_fences(markdown: &str, errors: &mut Vec<String>, file: &str) -> Vec<Fence> {
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
