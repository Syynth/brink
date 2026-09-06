//! Guard: every editor query the user can trigger is timed.
//!
//! `EditorSession`'s interaction surface follows one convention — a
//! `#[wasm_bindgen]` entry point (`hover`, `hover_doc`, …) delegating to a
//! private `*_impl` / `*_inner` that does the work. Anything an author can
//! make the editor do while typing runs through one of those, so timing
//! them is what makes a slow interaction attributable rather than a guess.
//!
//! This test exists because a one-time sweep rots: the next query added to
//! the editor would be invisible to the profiler and nobody would notice
//! until someone re-ran the sweep by hand. Hand enumeration has already
//! failed twice on this codebase in ways a mechanical check caught
//! immediately (`scripts/check-scripts.mjs`'s network-command scan, and the
//! fourteenth parameter-binding entry site found by a `debug_assert` rather
//! than by reading). So: assert the property, do not re-enumerate the
//! instances.
//!
//! Adding a query? Either wrap its call sites in `crate::perf::time` (the
//! normal answer) or add it to `SELF_TIMED` / `UNTIMED` below with a reason.

#![cfg(test)]

use std::collections::{BTreeMap, BTreeSet};

/// The editor source files this guard scans — the whole interaction surface.
fn editor_sources() -> Vec<(String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/editor");
    let mut out = Vec::new();
    let entries = std::fs::read_dir(&dir).expect("editor source directory is readable");
    for entry in entries {
        let path = entry.expect("directory entry").path();
        if path.extension().is_some_and(|e| e == "rs") {
            let name = path
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned();
            let text = std::fs::read_to_string(&path).expect("source file is readable");
            out.push((name, text));
        }
    }
    out
}

/// Work functions whose timing lives somewhere this line-oriented scan
/// cannot see. Empty today: every editor query is timed at its call sites,
/// which is the shape `cargo fmt` produces and the shape this guard reads.
const SELF_TIMED: &[&str] = &[];

/// Work functions deliberately left untimed, each with the reason it cannot
/// be part of an interaction's cost.
const UNTIMED: &[(&str, &str)] = &[];

/// Where a work function is called from: the source file, and a small
/// window of lines ending at the call (see [`work_functions`]).
type CallSites = BTreeMap<String, Vec<(String, String)>>;

/// Every `fn *_impl` / `fn *_inner` defined under `src/editor`, and every
/// line that calls one.
fn work_functions() -> (BTreeSet<String>, CallSites) {
    let mut defined = BTreeSet::new();
    let mut calls: CallSites = CallSites::new();
    let sources = editor_sources();

    for (_file, text) in &sources {
        for line in text.lines() {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed
                .strip_prefix("pub fn ")
                .or_else(|| trimmed.strip_prefix("fn "))
                && let Some(name) = rest.split('(').next()
                && (name.ends_with("_impl") || name.ends_with("_inner"))
            {
                defined.insert(name.to_owned());
            }
        }
    }

    for (file, text) in &sources {
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            // The definition line is not a call site.
            let trimmed = line.trim_start();
            if trimmed.starts_with("fn ")
                || trimmed.starts_with("pub fn ")
                || trimmed.starts_with("pub(super) fn ")
                || trimmed.starts_with("pub(crate) fn ")
            {
                continue;
            }
            for name in &defined {
                if line.contains(&format!("self.{name}(")) {
                    // `cargo fmt` breaks a wrapped call across lines, putting
                    // the `crate::perf::time(` opener above the call — so the
                    // evidence is a small window, not this line alone. Three
                    // lines is what the formatter's widest split produces.
                    let from = i.saturating_sub(3);
                    let window = lines[from..=i].join("\n");
                    calls
                        .entry(name.clone())
                        .or_default()
                        .push((file.clone(), window));
                }
            }
        }
    }
    (defined, calls)
}

#[test]
fn every_editor_query_is_timed() {
    let (defined, calls) = work_functions();
    let self_timed: BTreeSet<&str> = SELF_TIMED.iter().copied().collect();
    let untimed: BTreeSet<&str> = UNTIMED.iter().map(|(n, _)| *n).collect();

    let mut failures = Vec::new();
    for name in &defined {
        if self_timed.contains(name.as_str()) || untimed.contains(name.as_str()) {
            continue;
        }
        let Some(sites) = calls.get(name) else {
            // Defined but never called from another method — nothing an
            // interaction reaches, so nothing to time.
            continue;
        };
        for (file, window) in sites {
            if !window.contains("crate::perf::time") {
                let call = window.lines().next_back().unwrap_or(window).trim();
                failures.push(format!("  {file}: {call}"));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} editor query call site(s) are not wrapped in `crate::perf::time`, so an \
         interaction routed through them would be invisible to the profiler:\n{}\n\
         Wrap the call site, or add the function to `SELF_TIMED`/`UNTIMED` in \
         `perf_coverage.rs` with the reason.",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn the_untimed_allowlist_stays_honest() {
    // An allowlist entry for a function that no longer exists is a stale
    // waiver that would silently cover a future function of the same name.
    let (defined, _) = work_functions();
    for (name, reason) in UNTIMED {
        assert!(
            defined.contains(*name),
            "`{name}` is on the untimed allowlist but no longer exists — remove the entry"
        );
        assert!(
            reason.len() > 30,
            "`{name}`'s allowlist entry needs a real reason, not `{reason}`"
        );
    }
    for name in SELF_TIMED {
        assert!(
            defined.contains(*name),
            "`{name}` is listed as self-timed but no longer exists — remove the entry"
        );
    }
}

#[test]
fn the_self_timed_functions_really_do_time_themselves() {
    let sources = editor_sources();
    for name in SELF_TIMED {
        let found = sources.iter().any(|(_, text)| {
            let Some(idx) = text.find(&format!("fn {name}(")) else {
                return false;
            };
            // The `perf::time` call should be within the function's opening
            // lines, not merely somewhere in the same file.
            text[idx..]
                .lines()
                .take(6)
                .any(|l| l.contains("crate::perf::time"))
        });
        assert!(
            found,
            "`{name}` is listed as self-timed but its body does not call `crate::perf::time`"
        );
    }
}
