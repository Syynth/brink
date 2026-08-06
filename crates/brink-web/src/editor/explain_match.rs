//! The explain-match query's wasm binding (issue #2113, NS-T seam 3/6) —
//! `EditorSession::explain_match`/`explain_match_doc` wrap
//! [`brink_ir::ExplainMatchCache`] the same way `hover.rs` wraps
//! `brink_ide::hover`: a thin per-session cache plus offset/JSON
//! translation, no new logic of its own.
//!
//! # Why this DTO's ranges are raw bytes, not UTF-16
//!
//! Every other JSON DTO this crate emits converts byte ranges to UTF-16
//! against the *one* file the range belongs to (`byte_to_utf16`,
//! `to_relative`) — sound because that file is always the caller's active
//! document. A [`brink_ir::ClassifiedMatch::handler`] range is different:
//! it is a location in the project's **configured conventions module**,
//! which may not be a document this session has ever opened at all. This
//! session has no general "convert an arbitrary byte offset in an
//! arbitrary project file to UTF-16" facility (nothing before this needed
//! one), and building one is out of scope for this seam — the conventions
//! module's own path is not even a new fact for a host that already read
//! `[project] conventions` out of its own copy of `brink.toml` to discover
//! it. So `ExplainMatchJs` reports **raw byte offsets** throughout,
//! including the classified line's own captures (which this session
//! *could* convert, being always in the active document) — one convention
//! for the whole payload beats mixing UTF-16 captures with byte-offset
//! handler locations in a single response.
//!
//! # Every range is also file-absolute, not view-relative (#2113 review, w143)
//!
//! This is a second, orthogonal axis from bytes-vs-UTF-16 above, and it
//! matters to exactly the same fragment/view callers `hover_impl`/
//! `to_relative` serve: `explain_match_impl` converts its **input** offset
//! from view-relative to absolute via `to_absolute` (same as `hover_impl`),
//! but every range in the returned `ExplainMatchJs` — the classified line's
//! own capture ranges *and* the matched/attempted handler's declaration
//! range — comes back file-absolute, with no `to_relative` step on the way
//! out. A caller under `openFragment`/`setViewContext` therefore cannot map
//! any of these ranges back into its own document without first knowing
//! the view's own offset — unlike `hover`'s `start`/`end`, which are already
//! view-relative. Converting the capture ranges (always in the active
//! document, unlike the handler range) is left as a follow-up rather than
//! done here: it would make this DTO's payload straddle two conventions at
//! once (UTF-16, view-relative captures alongside byte, file-absolute
//! handler locations) for a caller this query has no evidence anyone is
//! driving through a fragment view yet. Until that follow-up lands, treat
//! every `ExplainMatchJs` range as file-absolute bytes, full stop — the
//! same caveat applies to the `explainMatch`/`explainMatchDoc` TS wrappers
//! in `@brink-lang/web` (`packages/wasm/src/index.ts`).

use rowan::TextSize;
use wasm_bindgen::prelude::*;

use super::{EditorSession, ViewContext};
use crate::editor_dto::explain_match_to_js;

#[wasm_bindgen]
impl EditorSession {
    /// Explain what would match the line containing `offset` in a document
    /// handle. Returns JSON (`ExplainMatchJs`, raw byte ranges — see this
    /// module's own doc) or `"null"` if the handle doesn't resolve to an
    /// open file.
    pub fn explain_match_doc(&mut self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        let path = d.path.clone();
        let view = d.view;
        self.explain_match_impl(&path, view, offset)
    }

    /// Explain what would match the line containing `offset` in the active
    /// file. Returns JSON (`ExplainMatchJs`) or `"null"`.
    pub fn explain_match(&mut self, offset: u32) -> String {
        let path = self.active_path.clone();
        let view = self.view;
        self.explain_match_impl(&path, view, offset)
    }
}

impl EditorSession {
    fn explain_match_impl(&mut self, path: &str, view: Option<ViewContext>, offset: u32) -> String {
        let abs_offset = self.to_absolute(path, view.as_ref(), offset);
        let Some((line_start, line_text)) = self.source_of(path).map(|source| {
            let (start, text) = line_containing(source, abs_offset);
            (start, text.to_owned())
        }) else {
            return "null".to_owned();
        };

        let projection = self.session.db().conventions_projection();
        let explanation =
            self.explain_cache
                .explain(&projection, TextSize::new(line_start), &line_text);
        serde_json::to_string(&explain_match_to_js(explanation)).unwrap_or_default()
    }
}

/// The byte range `[start, end)` of the line containing `offset` in
/// `source` — `start` is the byte after the preceding `\n` (or `0`), `end`
/// is the byte before the following `\n` (or `source.len()`). Mirrors the
/// `source.split('\n')`-with-running-offset idiom `brink-ide`'s own
/// `line_context.rs` uses, without pulling in that module's UTF-16 column
/// machinery this call site has no use for.
#[expect(
    clippy::cast_possible_truncation,
    reason = "ink/native files are always < 4GB, matching this crate's other byte-offset casts"
)]
fn line_containing(source: &str, offset: u32) -> (u32, &str) {
    let offset = (offset as usize).min(source.len());
    let start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = source[start..]
        .find('\n')
        .map_or(source.len(), |i| start + i);
    (start as u32, &source[start..end])
}

#[cfg(test)]
mod tests {
    use super::EditorSession;

    /// Fixture for the conventions-reaches-the-editor tests below. Until
    /// issue #1880 landed, `brink_ide::session::IdeSession::analysis_options`
    /// hardcoded `conventions: None` on every call, so
    /// `self.session.db().conventions_projection()` — what
    /// `explain_match_impl` reads — was **always empty** for every project
    /// configured the only way an embedder can: `apply_project_config`/
    /// `discover_project_config`. That blocked the *editor* path for
    /// anything built on `ConventionsProjection` (the `E169` diagnostic
    /// included) even though the query itself was already proven correct
    /// against real project data at the `brink-db`/`brink-ir` layer
    /// (`crates/internal/brink-db/tests/issue_2111_conventions_projection.rs`,
    /// this crate's own `hir::explain` unit tests) — #1880 is what threads
    /// the pointer the rest of the way through `IdeSession`.
    const CONVENTIONS: &str = "\
@[convention(claims = \"^(?<name>[A-Z][A-Z ]*)$\", order = 10)]
fn cue(name: string) {
  return name;
}

flow main() {
  VENDOR
}
";

    fn session_with_conventions() -> EditorSession {
        let mut s = EditorSession::new();
        s.update_file("conventions.brink", CONVENTIONS);
        s.apply_project_config("[project]\nconventions = \"conventions.brink\"\n")
            .expect("valid conventions pointer");
        assert!(s.set_active_file("conventions.brink"));
        s
    }

    /// The wasm entry point is wired all the way to
    /// `brink_ir::ExplainMatchCache` AND, since issue #1880, all the way to
    /// the live `ProjectDb`'s real `ConventionsProjection` through
    /// `EditorSession` — a real hit reports a real winner, not just
    /// well-formed JSON reporting "nothing configured". Before #1880's fix
    /// this asserted `matched: false` with `attempted: []` (see this
    /// module's own doc): a false `true`/populated `attempted` was the
    /// signal the gap had closed. It has.
    #[test]
    fn explain_match_reaches_the_live_conventions_projection_through_editor_session() {
        let mut s = session_with_conventions();
        let offset =
            u32::try_from(CONVENTIONS.find("VENDOR").expect("VENDOR line")).expect("offset");

        let json = s.explain_match(offset);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(
            v["matched"],
            serde_json::json!(true),
            "the conventions pointer must reach the live db through \
             EditorSession, so `cue`'s pattern really matches the VENDOR \
             line — got {v}"
        );
        assert_eq!(
            v["winner"]["handler"]["name"],
            serde_json::json!("cue"),
            "the winning handler must be `cue`, the only one declared — \
             got {v}"
        );
    }

    /// The miss sibling of the hit above, at the same live-db layer: a line
    /// that matches no declared handler must report a real (non-empty)
    /// `attempted` list, not the pre-#1880 "unconfigured" empty list.
    #[test]
    fn explain_match_reports_a_real_miss_through_editor_session() {
        let mut s = session_with_conventions();
        let offset =
            u32::try_from(CONVENTIONS.find("flow main").expect("flow line")).expect("offset");

        let json = s.explain_match(offset);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(v["matched"], serde_json::json!(false), "got {v}");
        let attempted = v["attempted"].as_array().cloned().unwrap_or_default();
        assert!(
            !attempted.is_empty(),
            "a real miss must report the handlers it tried against, now \
             that the conventions module is reachable — got {v}"
        );
    }

    /// The core composition logic itself — a real hit reporting its winner,
    /// captures, and shadowed handlers, and a real miss reporting attempted
    /// patterns in registration order — is exercised directly against
    /// `brink_ir::ExplainMatchCache`, the layer that owns it: see
    /// `crates/internal/brink-ir/src/hir/explain.rs`'s own test module. This
    /// crate's job is the wasm/JSON boundary, which the test above proves is
    /// wired; duplicating the hit/shadow/attempted-order assertions here
    /// would test `brink-ir` a second time through a slower harness for no
    /// added coverage.
    #[test]
    fn explain_match_doc_returns_null_for_an_unknown_handle() {
        let mut s = EditorSession::new();
        assert_eq!(s.explain_match_doc(999, 0), "null");
    }
}
