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

use brink_syntax_native::SyntaxNode;
use rowan::{TextRange, TextSize};
use wasm_bindgen::prelude::*;

use super::{EditorSession, ViewContext};
use crate::editor_dto::explain_match_to_js;

// ── `kind` composition (issue #2310) ────────────────────────────────
//
// `brink_ir::explain_match`'s own module doc traces why it cannot derive
// `brink_ir::ElementKind` ("matched kind") for its own bare-`text` inputs:
// one variant (`Parenthetical`) is chain-gated on the *previous* line,
// which a standalone line of text cannot answer. This module is the traced
// follow-up — "a caller that does hold a parsed CST node... can call
// `candidate` itself and pair its result alongside this module's
// `LineExplanation`" — except it does even better than re-calling
// `candidate`: `brink_ir::HirFile::element_matches` already carries the
// compile-time-correct `kind` for every line a handler claimed in the last
// successfully compiled snapshot of this file, so `matched_element_kind`
// below only *reads* that, never re-derives anything.
//
// This is a live read, not a stored snapshot: `self.session.db().hir(..)`
// (`brink-db`'s `ProjectDb::hir`) is a salsa query recomputed off the
// current revision — the same revision `self.source_of` reads text from —
// and `EditorSession::update_file` re-analyzes synchronously on every edit
// (`update_and_analyze`), so there is no window in which the compiled
// record can lag behind the live text `explanation` was just computed
// from (issue #2310 review). `matched_element_kind` still declines (reports
// `None`) on a real hit, for reasons that have nothing to do with
// staleness:
//
//   - `path` names an ink-dialect file — `HirFile::element_matches` is
//     always empty for the ink frontend, which has no `@[element]`
//     channel at all (see that field's own doc, `hir/types.rs`).
//   - `path` has no compiled `FileId`/`HirFile` at all, or its
//     `element_matches` simply has no entry for this line.
//   - the live winner claimed a line the compiler structurally declined
//     to record its own claim for — e.g. a scene heading carrying a
//     `[slug]` or trailing tags (`candidate` only recognizes a bare
//     `SCENE_HEADING` with nothing but its `SCENE_TITLE` child), or a
//     line folded into a block handler's own captured run rather than
//     claimed on its own.
//
// `matched_element_kind` guards against reporting a kind for the wrong
// handler by requiring the compiled `ElementMatch`'s own `handler` name
// to agree with the live winner, in addition to the line ranges
// overlapping; on any disagreement, or no compiled record at all, it
// reports `None` rather than guess — the same "never imply more than we
// know" discipline `brink_ir::explain_match`'s own doc holds captures to.

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
        // Issue #2351: when this exact line CST-parses to one of the five
        // claim-candidate shapes `hir::lower_native::element::candidate`
        // recognizes (`CUE`, `COMPACT_CUE`, `PARENTHETICAL`,
        // `SCENE_HEADING`, a wholly-literal `CONTENT_LINE`), classify that
        // node instead of the raw line text — the same sub-node text the
        // compiler's own `try_claim` matches against, so a real `@NAME`
        // cue or `(delivery)` parenthetical reports the same `matched`
        // answer the compiler's own claiming pass would. `node` stays
        // `None` (falling back to the pre-#2351 raw-text walk, unchanged)
        // for an ink-dialect file (no native parse tree at all), a file
        // with no compiled `FileId`, or a line that structurally is not
        // one of those five shapes (a knot/stitch header, a logic line,
        // blank text).
        let node = self.claim_candidate_node_at(path, abs_offset);
        let base = node
            .as_ref()
            .map_or_else(|| TextSize::new(line_start), |n| n.text_range().start());
        let explanation = self
            .explain_cache
            .explain(&projection, base, &line_text, node.as_ref());
        let kind = self.matched_element_kind(path, line_start, &line_text, &explanation);
        serde_json::to_string(&explain_match_to_js(explanation, kind)).unwrap_or_default()
    }

    /// The claim-candidate CST node covering `abs_offset` in `path`'s
    /// current parse, if any — issue #2351. `None` whenever there is
    /// nothing to hand `ExplainMatchCache::explain`'s node-aware path: an
    /// ink-dialect file (`db.parse_native` is native-only), an unknown
    /// path, or an offset that lands on a line outside the five shapes
    /// [`brink_ir::nearest_element_candidate`] recognizes.
    fn claim_candidate_node_at(&self, path: &str, abs_offset: u32) -> Option<SyntaxNode> {
        let db = self.session.db();
        let file_id = db.file_id(path)?;
        let parse = db.parse_native(file_id)?;
        let root = parse.syntax();
        let token = root
            .token_at_offset(TextSize::from(abs_offset))
            .right_biased()?;
        let start_node = token.parent()?;
        brink_ir::nearest_element_candidate(&start_node)
    }

    /// The compile-time [`brink_ir::ElementKind`] ("matched kind") for the
    /// line `explanation` classified, read from the last-compiled
    /// [`brink_ir::HirFile::element_matches`] for `path` — see this
    /// module's own doc for why this is a read, not a re-derivation, and
    /// why it can decline rather than guess.
    fn matched_element_kind(
        &self,
        path: &str,
        line_start: u32,
        line_text: &str,
        explanation: &brink_ir::LineExplanation,
    ) -> Option<brink_ir::ElementKind> {
        let brink_ir::LineExplanation::Matched { winner, .. } = explanation else {
            // Only a claimed line ever gets an `ElementMatch` at all —
            // `hir::lower_native::element::try_claim` never pushes one for
            // a line no handler claims — so a miss has no kind to report.
            return None;
        };
        let db = self.session.db();
        let file_id = db.file_id(path)?;
        let hir = db.hir(file_id)?;
        let line_range = TextRange::new(
            TextSize::from(line_start),
            TextSize::from(line_start) + TextSize::of(line_text),
        );
        hir.element_matches
            .iter()
            .find(|m| m.handler.text == winner.handler.text && ranges_overlap(m.line, line_range))
            .map(|m| m.kind)
    }
}

/// Whether `a` and `b` share any byte at all — tolerant of the two ranges
/// not being byte-identical (the compiled `ElementMatch::line` is a CST
/// node's own range; this call site's `line_range` is derived from a naive
/// `\n`-split of the live source), since both describe "the same source
/// line" for any well-formed file.
fn ranges_overlap(a: TextRange, b: TextRange) -> bool {
    a.start() < b.end() && b.start() < a.end()
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

@[convention(claims = \"^(?<kind>INT|EXT)\\. (?<title>.+)$\", order = 20)]
fn heading(kind: string, title: string) {
  return title;
}

flow main() {
  VENDOR
  @VENDOR
  INT. MARKET SQUARE
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

    /// Issue #2310: the compile-time `ElementKind` ("matched kind") is
    /// composed onto `winner.kind`, read from the last-compiled
    /// `HirFile::element_matches` for the active file — not re-derived.
    /// The `VENDOR` line carries no structural sigil (no `@`, no
    /// `INT./EXT.` prefix), so its compile-time shape is
    /// `ElementKind::ContentLine`.
    #[test]
    fn explain_match_composes_the_compiled_element_kind_onto_the_winner() {
        let mut s = session_with_conventions();
        let offset =
            u32::try_from(CONVENTIONS.find("VENDOR").expect("VENDOR line")).expect("offset");

        let json = s.explain_match(offset);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(
            v["winner"]["kind"],
            serde_json::json!("content_line"),
            "a bare, sigil-free line's compile-time shape is ContentLine — got {v}"
        );
    }

    /// Issue #2310 review (finding 3): a test that only ever sees
    /// `ContentLine` cannot tell a real read of the compiled record from a
    /// hardcoded/misindexed one, since `ContentLine` is also what a
    /// `matched_element_kind` bug (e.g. always returning the fixture's
    /// first `element_matches` entry) would produce. `INT. MARKET SQUARE`
    /// structurally parses as a `SCENE_HEADING` (the `INT.`/`EXT.` prefix
    /// rule, native syntax) and the fixture's `heading` handler claims it,
    /// so its compile-time shape is `ElementKind::SceneHeading` — a kind
    /// the compiled record genuinely differs on from the `VENDOR` case
    /// above.
    #[test]
    fn explain_match_reports_a_kind_that_differs_from_content_line() {
        let mut s = session_with_conventions();
        let offset = u32::try_from(
            CONVENTIONS
                .find("INT. MARKET SQUARE")
                .expect("heading line"),
        )
        .expect("offset");

        let json = s.explain_match(offset);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(
            v["winner"]["handler"]["name"],
            serde_json::json!("heading"),
            "got {v}"
        );
        assert_eq!(
            v["winner"]["kind"],
            serde_json::json!("scene_heading"),
            "a structural INT./EXT. heading's compile-time shape is \
             SceneHeading, not ContentLine — got {v}"
        );
    }

    /// Issue #2351 (fixing the gap issue #2310's own review finding 2
    /// named): `explain_match_impl` now classifies the CUE's `CUE_NAME`
    /// sub-node — the exact text `candidate`/`try_claim` match against —
    /// instead of the whole raw `"@VENDOR"` line, so the live walk agrees
    /// with the compiler: a real `@NAME` cue line reports a genuine hit,
    /// `winner.handler == "cue"`, `captures[0].text == "VENDOR"`, and (via
    /// #2310's existing `matched_element_kind` composition, now finally
    /// reachable for this shape) `kind == "cue"`. Before this fix, the
    /// live walk saw the whole `"@VENDOR"` text, which the `cue` handler's
    /// own `^[A-Z][A-Z ]*$` pattern (no `@` in its character class) could
    /// never match — a false, whole-answer `matched: false` even though
    /// the compiler's own compiled record claimed the exact same line.
    #[test]
    fn explain_match_reports_a_real_cue_kind_for_a_real_at_cue_line() {
        let mut s = session_with_conventions();
        let offset =
            u32::try_from(CONVENTIONS.find("@VENDOR").expect("@VENDOR cue line")).expect("offset");

        let json = s.explain_match(offset);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(
            v["matched"],
            serde_json::json!(true),
            "a real @VENDOR cue line must agree with the compiler's own claim — got {v}"
        );
        assert_eq!(
            v["winner"]["handler"]["name"],
            serde_json::json!("cue"),
            "got {v}"
        );
        assert_eq!(
            v["winner"]["captures"][0]["text"],
            serde_json::json!("VENDOR"),
            "the cue's own pattern must see only the name segment, without the leading \
             '@' — got {v}"
        );
        assert_eq!(
            v["winner"]["kind"],
            serde_json::json!("cue"),
            "a real @NAME cue's compile-time shape is Cue, and #2310's kind composition \
             can finally reach it now that `matched` agrees — got {v}"
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
