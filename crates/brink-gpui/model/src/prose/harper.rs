//! Harper (`brink-prose`): the checker on every platform today, and the
//! grammar while typing on macOS once the OS does the spelling there
//! (`docs/gpui-prose-checker-spec.md` §1).
//!
//! `brink-prose` is the web's own wasm artifact, linked here directly
//! (`Cargo.toml`, `brink-prose`). It speaks UTF-16 at its boundary, which
//! is the unit every engine in this module answers in.

use super::{Lint16, MAX_FIXES, ProseFix};

/// Check `text`'s prose `spans` (UTF-16, half-open). The project's words
/// and the dialect are as `[prose]` and the analysis give them.
pub(super) fn check(
    text: &str,
    spans: &[(u32, u32)],
    dictionary: &[String],
    dialect: Option<&str>,
) -> Vec<Lint16> {
    let request = brink_prose::CheckRequest {
        text: text.to_owned(),
        spans: spans
            .iter()
            .map(|&(start, end)| brink_prose::SpanJs {
                start: start as usize,
                end: end as usize,
            })
            .collect(),
        dictionary: dictionary.to_vec(),
        dialect: dialect.map(str::to_owned),
    };
    brink_prose::check(&request)
        .lints
        .into_iter()
        .map(|lint| Lint16 {
            start: lint.start as u32,
            end: lint.end as u32,
            kind: lint.kind,
            message: lint.message,
            fixes: lint
                .suggestions
                .into_iter()
                .filter_map(|s| match s.kind {
                    "replace" => Some(ProseFix::Replace(s.text)),
                    "remove" => Some(ProseFix::Remove),
                    _ => None,
                })
                .take(MAX_FIXES)
                .collect(),
        })
        .collect()
}
