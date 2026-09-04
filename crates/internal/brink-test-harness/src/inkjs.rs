//! The inkjs reference oracle bridge (issue #3379,
//! `docs/program-generator-spec.md` §6).
//!
//! `tools/inkjs-oracle` is a port of `tools/ink-oracle` (the C# crawler that
//! blesses every checked-in `*.oracle.json`) onto inkjs 2.4.0 — Node-only,
//! so it runs in CI and in cloud sessions that have no `dotnet`. This module
//! spawns it and normalises its output so the harness can use it two ways:
//!
//! - **Sanction** (`tests/inkjs_sanction.rs`): replay every C# golden across
//!   tier1–3 through inkjs and demand a match. inkjs is not on `CLAUDE.md`'s
//!   trust hierarchy; this is what earns it standing as a proxy for the
//!   rank-2 reference. The goldens stay C#-blessed — the inkjs tool never
//!   writes next to one.
//! - **Differential** (`crates/internal/brink-gen/tests/inkjs_differential.rs`):
//!   for generated stories, `brink(P)` against `inkjs(P)` via
//!   [`crate::diff_oracle`], the same comparison the corpus ratchet uses.
//!
//! Both are opt-in behind [`ENABLE_ENV`]`=1` because they need `node` and an
//! `npm ci` in `tools/inkjs-oracle`; a plain `cargo test --workspace` skips
//! them loudly (one line per test) rather than failing.
//!
//! # What the normalisation forgives, and why
//!
//! Measured over the full corpus (410 oracle cases, 2026-09-04) the raw
//! inkjs output matched 396 goldens byte for byte. Every one of the 14
//! remaining differences was one of exactly two presentational artefacts of
//! the *reference implementation*, not a semantic divergence:
//!
//! 1. **Error message dressing.** The C# tool ran without an `onError`
//!    handler for most goldens, so a runtime error arrives wrapped as
//!    `Ink had 1 error. It is strongly suggested that you assign an error
//!    handler to story.onError. The first issue was: RUNTIME ERROR: …`, and
//!    the quoted source path is the maintainer's absolute path at
//!    generation time. inkjs reports the bare `RUNTIME ERROR: 'story.ink'
//!    line N: …`, and spells its missing-external prefix `Error:` where C#
//!    says `ERROR:`. [`normalize_error`] reduces both to the same string.
//! 2. **Float printing.** ink's C# runtime is single-precision;
//!    inkjs runs on doubles. `2/3` prints `0.6666667` there and
//!    `0.6666666666666666` here. [`normalize_floats`] reprints every decimal
//!    token in a `text` as the shortest round-trip `f32`, which is what C#
//!    (and brink's own runtime) print already.
//!
//! Neither normalisation can hide a wrong branch, a wrong value, a missing
//! step or a wrong choice list — those stay exact. A future divergence that
//! survives both is a real finding and belongs in
//! `tests/inkjs_sanction.rs`'s `KNOWN_DIVERGENCES` with a reason, never in a
//! third normaliser.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value as JsonValue;

use crate::episode::{Episode, Outcome, StepOutcome};
use crate::oracle::{OracleEpisode, OracleOutcome, OracleStepOutcome, oracle_episode_files};

/// Set to `1` to run the inkjs-backed tests. Anything else (or unset) makes
/// them skip with a one-line notice.
pub const ENABLE_ENV: &str = "BRINK_INKJS_ORACLE";

/// Whether the inkjs-backed tests should run in this process.
pub fn enabled() -> bool {
    std::env::var(ENABLE_ENV).is_ok_and(|v| v == "1")
}

/// `tools/inkjs-oracle` in this checkout.
pub fn oracle_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tools")
        .join("inkjs-oracle")
}

/// Why an inkjs oracle run could not produce episodes.
#[derive(Debug, thiserror::Error)]
pub enum InkjsError {
    /// `npm ci` has not been run in `tools/inkjs-oracle`.
    #[error(
        "inkjs oracle is not installed: {0} is missing — run `npm ci` in tools/inkjs-oracle \
         (CI's `inkjs-oracle` job does this; see docs/program-generator-spec.md §6)"
    )]
    NotInstalled(PathBuf),
    /// `node` could not be started at all.
    #[error("failed to spawn `node`: {0}")]
    Spawn(std::io::Error),
    /// The oracle ran and reported failure (a compile error, an explore
    /// failure, or a usage error); `stderr` carries its diagnostics.
    #[error("inkjs oracle failed ({status}):\n{stderr}")]
    Failed { status: String, stderr: String },
    /// The oracle exited 0 but its output could not be read back.
    #[error("{0}")]
    Load(String),
}

/// Fail fast with a pointer at `npm ci` if the package is not installed.
pub fn ensure_installed() -> Result<(), InkjsError> {
    let marker = oracle_dir()
        .join("node_modules")
        .join("inkjs")
        .join("package.json");
    if marker.is_file() {
        Ok(())
    } else {
        Err(InkjsError::NotInstalled(marker))
    }
}

fn node() -> Command {
    let mut cmd = Command::new("node");
    cmd.arg(oracle_dir().join("oracle.mjs"));
    cmd
}

/// Crawl one story into `out_dir` (`node oracle.mjs <ink> --output-dir
/// <out_dir>`) and load the episodes back. `out_dir` is cleared of stale
/// `*.oracle.json` by the tool itself.
pub fn explore_file(ink_path: &Path, out_dir: &Path) -> Result<Vec<OracleEpisode>, InkjsError> {
    ensure_installed()?;
    let output = node()
        .arg(ink_path)
        .arg("--output-dir")
        .arg(out_dir)
        .output()
        .map_err(InkjsError::Spawn)?;
    if !output.status.success() {
        return Err(InkjsError::Failed {
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    crate::oracle::load_oracle_episodes_from_dir(out_dir).map_err(InkjsError::Load)
}

/// The tool's own per-case log from a [`crawl`].
#[derive(Debug)]
pub struct CrawlLog {
    /// Everything the tool printed to stderr: one `OK`/`COMPILE FAILED`/
    /// `EXPLORE FAILED` line per `story.ink`, plus the closing tally.
    pub stderr: String,
    /// Whether every story crawled cleanly. `false` is not an error here —
    /// the corpus deliberately holds compile-error probes with no golden —
    /// so a caller decides per case whether a missing output matters.
    pub all_succeeded: bool,
}

/// Crawl every `story.ink` under `tests_dir` into `out_root/<relative case
/// dir>/` (`node oracle.mjs --crawl <tests_dir> --output-root <out_root>`).
/// One process for a whole tier — about two seconds for tier1 — instead of
/// one per case.
pub fn crawl(tests_dir: &Path, out_root: &Path) -> Result<CrawlLog, InkjsError> {
    ensure_installed()?;
    let output = node()
        .arg("--crawl")
        .arg(tests_dir)
        .arg("--output-root")
        .arg(out_root)
        .output()
        .map_err(InkjsError::Spawn)?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    match output.status.code() {
        Some(0) => Ok(CrawlLog {
            stderr,
            all_succeeded: true,
        }),
        // Exit 1 is the tool's "some stories failed" — reported per line.
        Some(1) if stderr.contains("Done:") => Ok(CrawlLog {
            stderr,
            all_succeeded: false,
        }),
        _ => Err(InkjsError::Failed {
            status: output.status.to_string(),
            stderr,
        }),
    }
}

/// Every `*.oracle.json` in `dir` as a raw JSON tree, in
/// [`oracle_episode_files`] order.
pub fn load_episode_values(dir: &Path) -> Result<Vec<JsonValue>, String> {
    let mut out = Vec::new();
    for path in oracle_episode_files(dir)? {
        let content =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let value: JsonValue =
            serde_json::from_str(&content).map_err(|e| format!("parse {}: {e}", path.display()))?;
        out.push(value);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Normalisation
// ---------------------------------------------------------------------------

/// The wrapper the C# runtime puts around an error when no `onError`
/// handler is attached (`Story.cs`); everything after it is the message.
const NO_HANDLER_WRAPPER: &str = "The first issue was: ";

/// Reduce an ink runtime error message to the part both implementations
/// agree on: strip the no-handler wrapper, reduce a quoted source path to
/// its file name, and spell the leading severity word the C# way.
pub fn normalize_error(message: &str) -> String {
    let mut msg = match message.find(NO_HANDLER_WRAPPER) {
        Some(at) => &message[at + NO_HANDLER_WRAPPER.len()..],
        None => message,
    }
    .to_owned();

    msg = strip_quoted_paths(&msg);

    if let Some(rest) = msg.strip_prefix("Error: ") {
        msg = format!("ERROR: {rest}");
    }
    msg
}

/// `'/abs/path/to/story.ink'` → `'story.ink'` for every single-quoted
/// `.ink` path in `msg`.
fn strip_quoted_paths(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    let mut rest = msg;
    while let Some(open) = rest.find('\'') {
        out.push_str(&rest[..=open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('\'') else {
            out.push_str(after);
            rest = "";
            break;
        };
        let quoted = &after[..close];
        let is_ink_path = quoted.contains('/')
            && Path::new(quoted)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("ink"));
        if is_ink_path {
            let base = quoted.rsplit('/').next().unwrap_or(quoted);
            out.push_str(base);
        } else {
            out.push_str(quoted);
        }
        out.push('\'');
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// A decimal token with this many significant digits or more cannot be the
/// shortest round-trip print of an `f32` (which needs at most 9), so it is
/// a double-precision artefact by construction.
const F32_MAX_SIGNIFICANT_DIGITS: usize = 9;

/// Reprint every decimal token (`digits.digits`) that carries MORE
/// significant digits than an `f32` can (`0.6666666666666666`, sixteen) as
/// the shortest round-trip `f32` (`0.6666667`), so double-precision output
/// compares equal to single-precision output. Anything an `f32` could have
/// printed is left exactly as written — `0.0` stays `0.0`, `2.50` stays
/// `2.50` — so the rewrite can only ever erase the double-vs-single
/// artefact, never a real difference in how a value was printed. Integers
/// and dotted runs with more than one dot (`1.2.3`) are never touched.
///
/// Idempotent, so it is safe (and, for symmetry, expected) to apply to
/// both sides of a comparison.
pub fn normalize_floats(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(|c: char| c.is_ascii_digit()) {
        out.push_str(&rest[..start]);
        let run_end = rest[start..]
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .map_or(rest.len(), |n| start + n);
        let token = &rest[start..run_end];
        out.push_str(&reprint_decimal(token));
        rest = &rest[run_end..];
    }
    out.push_str(rest);
    out
}

fn reprint_decimal(token: &str) -> String {
    let dots = token.matches('.').count();
    if dots != 1 || token.starts_with('.') || token.ends_with('.') {
        return token.to_owned();
    }
    let significant = token
        .chars()
        .filter(char::is_ascii_digit)
        .skip_while(|c| *c == '0')
        .count();
    if significant <= F32_MAX_SIGNIFICANT_DIGITS {
        return token.to_owned();
    }
    match token.parse::<f32>() {
        Ok(v) => format!("{v}"),
        Err(_) => token.to_owned(),
    }
}

/// Apply [`normalize_error`] and [`normalize_floats`] in place to a raw
/// episode tree: every `"text"` string (steps and choice records) and the
/// episode-level `"Error"` string. Nothing else is touched.
pub fn normalize_episode_value(value: &mut JsonValue) {
    match value {
        JsonValue::Object(map) => {
            for (key, v) in map.iter_mut() {
                match (key.as_str(), &mut *v) {
                    ("text", JsonValue::String(s)) => *s = normalize_floats(s),
                    ("Error", JsonValue::String(s)) => *s = normalize_error(s),
                    _ => normalize_episode_value(v),
                }
            }
        }
        JsonValue::Array(items) => items.iter_mut().for_each(normalize_episode_value),
        _ => {}
    }
}

/// The typed counterpart of [`normalize_episode_value`], for an episode the
/// differential will hand to [`crate::diff_oracle`].
pub fn normalize_oracle_episode(ep: &mut OracleEpisode) {
    for step in &mut ep.steps {
        step.text = normalize_floats(&step.text);
        if let OracleStepOutcome::Choices { choices } = &mut step.outcome {
            for c in &mut choices.presented {
                c.text = normalize_floats(&c.text);
            }
        }
    }
    match &mut ep.outcome {
        OracleOutcome::Error { error } => *error = normalize_error(error),
        OracleOutcome::InputsExhausted { data } => {
            for c in &mut data.remaining_choices {
                c.text = normalize_floats(&c.text);
            }
        }
        OracleOutcome::Terminal(_) => {}
    }
}

/// [`normalize_oracle_episode`]'s counterpart for brink's own explored
/// episode, so the differential rewrites both sides by the same rule.
pub fn normalize_brink_episode(ep: &mut Episode) {
    for step in &mut ep.steps {
        step.text = normalize_floats(&step.text);
        if let StepOutcome::Choices { presented, .. } = &mut step.outcome {
            for c in presented {
                c.text = normalize_floats(&c.text);
            }
        }
    }
    if let Outcome::InputsExhausted { remaining_choices } = &mut ep.outcome {
        for c in remaining_choices {
            c.text = normalize_floats(&c.text);
        }
    }
}

/// The first point at which two JSON trees differ, as a `/`-joined path
/// plus both values — `None` when they are equal.
pub fn first_difference(expected: &JsonValue, actual: &JsonValue) -> Option<String> {
    let mut path = Vec::new();
    first_difference_at(expected, actual, &mut path)
}

fn first_difference_at(
    expected: &JsonValue,
    actual: &JsonValue,
    path: &mut Vec<String>,
) -> Option<String> {
    let render = |path: &[String], e: &JsonValue, a: &JsonValue| {
        Some(format!(
            "at /{}:\n    expected (C#):  {e}\n    actual (inkjs): {a}",
            path.join("/")
        ))
    };
    match (expected, actual) {
        (JsonValue::Object(e), JsonValue::Object(a)) => {
            for (k, ev) in e {
                path.push(k.clone());
                let found = match a.get(k) {
                    Some(av) => first_difference_at(ev, av, path),
                    None => Some(format!("at /{}: missing in inkjs output", path.join("/"))),
                };
                path.pop();
                if found.is_some() {
                    return found;
                }
            }
            for k in a.keys() {
                if !e.contains_key(k) {
                    path.push(k.clone());
                    let msg = format!("at /{}: extra key in inkjs output", path.join("/"));
                    path.pop();
                    return Some(msg);
                }
            }
            None
        }
        (JsonValue::Array(e), JsonValue::Array(a)) => {
            for (i, (ev, av)) in e.iter().zip(a).enumerate() {
                path.push(i.to_string());
                let found = first_difference_at(ev, av, path);
                path.pop();
                if found.is_some() {
                    return found;
                }
            }
            if e.len() == a.len() {
                None
            } else {
                Some(format!(
                    "at /{}: length {} (C#) vs {} (inkjs)",
                    path.join("/"),
                    e.len(),
                    a.len()
                ))
            }
        }
        (JsonValue::Number(e), JsonValue::Number(a)) => {
            // `1.0` and `1` are the same value to both runtimes.
            let same = match (e.as_f64(), a.as_f64()) {
                (Some(x), Some(y)) => x.to_bits() == y.to_bits(),
                _ => e == a,
            };
            if same {
                None
            } else {
                render(path, expected, actual)
            }
        }
        _ => {
            if expected == actual {
                None
            } else {
                render(path, expected, actual)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_wrapper_path_and_prefix_are_normalised() {
        let csharp = "Ink had 1 error. It is strongly suggested that you assign an error \
                      handler to story.onError. The first issue was: RUNTIME ERROR: \
                      '/Users/someone/code/brink/tests/tier1/x/story.ink' line 2: ran out of \
                      content. Do you need a '-> DONE' or '-> END'?";
        let inkjs = "RUNTIME ERROR: 'story.ink' line 2: ran out of content. Do you need a \
                     '-> DONE' or '-> END'?";
        assert_eq!(normalize_error(csharp), normalize_error(inkjs));
        assert_eq!(normalize_error(inkjs), inkjs);

        assert_eq!(
            normalize_error("Error: Missing function binding for external: 'f'"),
            "ERROR: Missing function binding for external: 'f'"
        );
        // An included file's name (no directory) is untouched on both sides.
        assert_eq!(
            normalize_error("RUNTIME ERROR: 'trailing_weave.ink' line 4: x"),
            "RUNTIME ERROR: 'trailing_weave.ink' line 4: x"
        );
    }

    #[test]
    fn floats_reprint_as_f32_and_integers_survive() {
        assert_eq!(normalize_floats("0.6666666666666666\n"), "0.6666667\n");
        assert_eq!(normalize_floats("2.3333333333333335"), "2.3333333");
        assert_eq!(normalize_floats("2.3333333"), "2.3333333");
        assert_eq!(normalize_floats("x = 42, y = -1.5."), "x = 42, y = -1.5.");
        assert_eq!(normalize_floats("v1.2.3 and 10"), "v1.2.3 and 10");
        assert_eq!(normalize_floats("no numbers"), "no numbers");
        // Anything an f32 could have printed is left alone — including the
        // shapes a generated story's TEXT can contain by accident, which a
        // blanket reprint would rewrite on one side only (`a0.0,` → `a0,`).
        assert_eq!(normalize_floats("a0.0,"), "a0.0,");
        assert_eq!(normalize_floats("2.50 and 0.000"), "2.50 and 0.000");
        assert_eq!(normalize_floats("12345678.5"), "12345678.5");
        assert_eq!(normalize_floats("1234567890.5"), "1234568000");
        // Idempotent.
        let once = normalize_floats("0.30000000000000004");
        assert_eq!(once, "0.3");
        assert_eq!(normalize_floats(&once), once);
    }

    #[test]
    fn episode_value_normalisation_touches_only_text_and_error() {
        let mut v: JsonValue = serde_json::json!({
            "steps": [{
                "text": "0.6666666666666666\n",
                "tags": ["0.6666666666666666"],
                "outcome": {"Choices": {"presented": [{"text": "1.10000000001", "index": 0, "tags": []}], "selected": 0}},
                "variable_changes": {"x": 0.666_666_666_666_666_6},
                "visit_changes": {},
                "turn_index": 1
            }],
            "outcome": {"Error": "Error: boom"},
            "choice_path": [],
            "initial_state": {"variables": {}, "turn_index": 0}
        });
        normalize_episode_value(&mut v);
        assert_eq!(v["steps"][0]["text"], "0.6666667\n");
        assert_eq!(v["steps"][0]["tags"][0], "0.6666666666666666");
        assert_eq!(
            v["steps"][0]["outcome"]["Choices"]["presented"][0]["text"],
            "1.1"
        );
        assert_eq!(
            v["steps"][0]["variable_changes"]["x"],
            0.666_666_666_666_666_6
        );
        assert_eq!(v["outcome"]["Error"], "ERROR: boom");
    }

    #[test]
    fn first_difference_names_the_path() {
        let a = serde_json::json!({"steps": [{"text": "a"}, {"text": "b"}]});
        let b = serde_json::json!({"steps": [{"text": "a"}, {"text": "c"}]});
        let d = first_difference(&a, &b).expect("differs");
        assert!(d.starts_with("at /steps/1/text:"), "{d}");
        assert!(first_difference(&a, &a).is_none());
        let short = serde_json::json!({"steps": [{"text": "a"}]});
        let d = first_difference(&a, &short).expect("differs");
        assert!(d.contains("length 2 (C#) vs 1 (inkjs)"), "{d}");
    }
}
