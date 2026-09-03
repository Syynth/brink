//! `Safe` fixer for `E092` (a `#@public`/`#@private` directive that restates
//! the module's own default): delete the redundant directive tag — issue
//! #3424, milestone 8 of the auto-fix epic (#3374,
//! `docs/autofix-spec.md` §9's first-wave `Safe` list).
//!
//! # Why this is `Safe`
//!
//! `E092` fires from `brink-analyzer::manifest::insert_symbol`'s
//! `effective_visibility` check: a declared module defaults `Private`, an
//! undeclared stem-module defaults `Public` (`docs/modules-spec.md` §4), and
//! an explicit `#@private`/`#@public` override that names exactly that
//! default is redundant. Deleting the override changes nothing about the
//! *effective* visibility — the default takes over and computes the same
//! [`brink_ir::Visibility`] the override already named — and a `#@…`
//! tag-directive is static, compile-time-only text: `hir::lower::directive`'s
//! own module doc says every directive "is consumed at compile time and
//! erased from runtime tag output", so removing one can never change a
//! line's `tags`/`element` payload either. Both halves of the `Safe`
//! obligation (`docs/observable-semantics-spec.md` §2/§2.2) hold: the trace
//! is untouched (nothing about program *behaviour* reads a directive tag)
//! and translation identity is untouched (the directive was never a
//! translatable content line — `content::tag_line`'s chokepoint never lowers
//! a directive line to content in the first place).
//!
//! # Why the tag is always alone on its line, here
//!
//! The issue's own fix description ("remove the tag, and the line if the tag
//! was alone on it; keep other tags on the line") reads as if a directive
//! could share a `TAG_LINE` with something else. It cannot, for exactly the
//! declarations this diagnostic reaches: `hir::lower::directive::scan_tag_line`
//! classifies a `TAG_LINE` as `Directives` (the only classification
//! `directives_before`/`leading_body_directives` ever collect from) **only**
//! when the line carries exactly one tag and it is directive-shaped — a
//! second tag of any kind, directive or plain, reclassifies the whole line
//! `Mixed`, which is invalid (`E047`) and is never attached to a declaration
//! at all (so a mark from a `Mixed` line can never be `chosen` and can never
//! be redundant). So whenever `E092` actually fires, the responsible
//! directive is the line's sole tag — this module deletes the entire
//! `TAG_LINE` node (its trailing newline included, since the parser folds it
//! into the same node) rather than special-casing a shared line that cannot
//! occur here.
//!
//! # Scope — ink only
//!
//! `#@private`/`#@public` is an ink-dialect-only tag-channel directive
//! (`hir::lower::directive`'s own module doc: "riding the tag channel
//! (`#@…`)"). The native `.brink` surface has no such channel — its
//! visibility marker is the `pub` keyword (issue #1582) instead, and there
//! is no "tag" and no "line" there for the issue's fix shape ("remove the
//! tag … keep other tags on the line") to remove.
//!
//! In practice `E092` cannot even fire for native today:
//! `brink_db::queries::module_map_query`'s own doc says a native file's
//! module is always marked `declared` ("so it always qualifies
//! `DefinitionId`"), and a declared module defaults `Private` — so native's
//! own visibility mark (`Some(VisibilityMark::Public)`, the only non-`None`
//! value native lowering ever produces, per `hir::emit_native::pub_prefix`'s
//! doc) is always a real override, never a redundant restatement. Still,
//! this fixer checks `ProjectDb::is_native` first and returns nothing for a
//! native file unconditionally, as defense-in-depth rather than a
//! narrowing of any reachable shape: everything below only ever parses with
//! the *ink* grammar (`brink_syntax::parse`), so running it over native
//! source on the strength of "well, `E092` shouldn't reach here" would risk
//! a bogus range match instead of visibly doing nothing.
//!
//! # Locating the directive
//!
//! `E092`'s diagnostic range is `DeclaredSymbol::range`, which is always the
//! *name* token's own range — never the whole declaration and never the
//! directive (`crates/brink-lsp/src/convert.rs`'s `is_unnecessary` doc
//! documents this gap in detail; `brink_ir::VisibilityDirective::range`
//! exists but is never plumbed through to `DeclaredSymbol`). So this fixer
//! re-derives the directive's location from the CST itself rather than the
//! diagnostic's own range: it finds the `ast::Identifier` whose range
//! matches `d.range`, then walks to that identifier's owning declaration and
//! replays the same two attachment rules `hir::lower::directive` uses
//! (`directives_before` for `VAR`/`CONST`/`LIST`/`EXTERNAL`/`STRUCT`;
//! `leading_body_directives` for a knot/stitch's own body) — reimplemented
//! locally against `brink_syntax`'s public AST rather than reusing those
//! functions, which are `pub(super)` inside `brink-ir::hir::lower` and
//! unreachable from this crate.
//!
//! To stay unambiguous, the fixer requires **exactly one**
//! visibility-shaped directive (`#@private` or `#@public`, bare, static) in
//! the attached run. Two visibility directives on the same target is a
//! separate conflict (`E093`) that `visibility_from_directives` resolves by
//! keeping the *first* one — a shape this fixer declines rather than guess
//! which occurrence the diagnostic actually meant.

use brink_db::ProjectDb;
use brink_ir::{Diagnostic, DiagnosticCode, FileId};
use brink_syntax::ast::{self, AstNode};
use brink_syntax::{SyntaxKind, SyntaxNode};
use rowan::TextRange;

use crate::fix::{Applicability, Fix, FixCx, Fixer};
use crate::rename::FileEdit;

/// The `E092` fixer: delete a redundant `#@public`/`#@private` directive tag.
pub struct RedundantVisibilityFixer;

impl Fixer for RedundantVisibilityFixer {
    fn code(&self) -> DiagnosticCode {
        DiagnosticCode::E092
    }

    fn max_applicability(&self) -> Applicability {
        Applicability::Safe
    }

    fn fixes(&self, cx: &FixCx<'_>, d: &Diagnostic) -> Vec<Fix> {
        redundant_visibility_fix(cx.db, d)
    }
}

fn redundant_visibility_fix(db: &ProjectDb, d: &Diagnostic) -> Vec<Fix> {
    // Narrow (module doc "Scope" above): native's `pub`-keyword redundancy
    // has no tag-directive shape to remove.
    if db.is_native(d.file) {
        return Vec::new();
    }
    let Some(source) = db.source(d.file) else {
        return Vec::new();
    };
    let Some(range) = find_directive_to_remove(source, d.range) else {
        return Vec::new();
    };
    build_fix(d.file, range)
}

fn build_fix(file: FileId, range: TextRange) -> Vec<Fix> {
    vec![Fix {
        code: DiagnosticCode::E092,
        title: "Remove the redundant visibility directive".to_owned(),
        applicability: Applicability::Safe,
        edits: vec![FileEdit {
            file,
            range,
            new_text: String::new(),
        }],
        caret: None,
    }]
}

/// Parse `source` as ink and locate the sole `TAG_LINE` a redundant
/// `#@public`/`#@private` at `target_range` (the diagnosed name's own range)
/// must have come from. `None` when the shape is anything other than
/// exactly-one-visibility-directive attached to the identified declaration —
/// including "no directive found at all", which should not happen for a real
/// `E092` instance but is treated as "narrow, don't guess" all the same.
fn find_directive_to_remove(source: &str, target_range: TextRange) -> Option<TextRange> {
    let parse = brink_syntax::parse(source);
    let tree = parse.tree();
    let root = tree.syntax().clone();

    let ident = root
        .descendants()
        .filter_map(ast::Identifier::cast)
        .find(|i| i.syntax().text_range() == target_range)?;
    let parent = ident.syntax().parent()?;

    match parent.kind() {
        SyntaxKind::KNOT_HEADER => {
            let knot_def = parent.parent()?;
            let knot_def = ast::KnotDef::cast(knot_def)?;
            let body = knot_def.body()?;
            find_visibility_in_leading_run(body.syntax())
        }
        SyntaxKind::STITCH_HEADER => {
            let stitch_def = parent.parent()?;
            let stitch_def = ast::StitchDef::cast(stitch_def)?;
            let body = stitch_def.body()?;
            find_visibility_in_leading_run(body.syntax())
        }
        SyntaxKind::VAR_DECL
        | SyntaxKind::CONST_DECL
        | SyntaxKind::LIST_DECL
        | SyntaxKind::EXTERNAL_DECL
        | SyntaxKind::STRUCT_DECL => find_visibility_before(&parent),
        _ => None,
    }
}

/// Whether `kind` is whitespace/comment trivia or a bare newline — the two
/// token kinds `hir::lower::directive`'s own lookback treats as invisible
/// when walking between declaration and directive.
fn is_trivia_token(kind: SyntaxKind) -> bool {
    kind.is_trivia() || kind == SyntaxKind::NEWLINE
}

/// One `TAG_LINE`'s classification for this fixer's purposes: does it carry
/// the single redundant visibility directive, some other single directive
/// (`#@was`, `#@local`, …, left alone), or is it not a clean single-directive
/// line at all (a plain runtime tag, or a `Mixed` line mixing several)?
enum LineKind {
    Visibility(TextRange),
    OtherDirective,
    NotDirective,
}

fn classify_tag_line(tl: &ast::TagLine) -> LineKind {
    let Some(tags) = tl.tags() else {
        return LineKind::NotDirective;
    };
    let all: Vec<ast::Tag> = tags.tags().collect();
    if all.len() != 1 {
        // Two-or-more tags on one line is never a valid single directive
        // (`hir::lower::directive::scan_tag_line`'s `Mixed` arm) — never
        // attached, never the source of a redundant mark.
        return LineKind::NotDirective;
    }
    let tag = &all[0];
    let dynamic = tag
        .syntax()
        .children_with_tokens()
        .any(|c| c.as_node().is_some());
    let text = tag.text(); // "#" already stripped and trimmed
    let Some(rest) = text.strip_prefix('@') else {
        return LineKind::NotDirective;
    };
    let name: String = rest
        .chars()
        .take_while(|c| *c != '(' && !c.is_whitespace())
        .collect();
    let bare = !dynamic && rest.len() == name.len();
    if bare && !dynamic && (name == "private" || name == "public") {
        LineKind::Visibility(tl.syntax().text_range())
    } else {
        LineKind::OtherDirective
    }
}

/// [`hir::lower::directive::directives_before`]'s backward lookback,
/// reimplemented against public `brink_syntax` AST: walk `decl`'s preceding
/// siblings, skipping trivia and `EMPTY_LINE`s, collecting consecutive
/// single-directive `TAG_LINE`s until something else is hit. Returns the
/// range of the sole visibility-directive line found, or `None` when there
/// isn't exactly one.
fn find_visibility_before(decl: &SyntaxNode) -> Option<TextRange> {
    let mut found: Option<TextRange> = None;
    let mut count = 0usize;
    let mut cursor = decl.prev_sibling_or_token();
    while let Some(el) = cursor {
        match el {
            rowan::NodeOrToken::Token(tok) => {
                if !is_trivia_token(tok.kind()) {
                    break;
                }
                cursor = tok.prev_sibling_or_token();
            }
            rowan::NodeOrToken::Node(node) => {
                if node.kind() == SyntaxKind::EMPTY_LINE {
                    cursor = node.prev_sibling_or_token();
                    continue;
                }
                let Some(tl) = ast::TagLine::cast(node.clone()) else {
                    break;
                };
                match classify_tag_line(&tl) {
                    LineKind::Visibility(range) => {
                        count += 1;
                        found = Some(range);
                    }
                    LineKind::OtherDirective => {}
                    LineKind::NotDirective => break,
                }
                cursor = node.prev_sibling_or_token();
            }
        }
    }
    (count == 1).then_some(found).flatten()
}

/// [`hir::lower::directive::leading_body_directives`]'s forward scan,
/// reimplemented the same way: walk `body`'s children from the start,
/// skipping trivia, `EMPTY_LINE`s and `ANNOTATION_LINE`s, and collecting
/// consecutive single-directive `TAG_LINE`s (a plain or `Mixed` tag line does
/// *not* end the leading run — it simply is not itself a directive) until a
/// real content node is hit. Returns the range of the sole visibility line
/// found, or `None` when there isn't exactly one.
fn find_visibility_in_leading_run(body: &SyntaxNode) -> Option<TextRange> {
    let mut found: Option<TextRange> = None;
    let mut count = 0usize;
    for el in body.children_with_tokens() {
        match el {
            rowan::NodeOrToken::Token(tok) => {
                if !is_trivia_token(tok.kind()) {
                    break;
                }
            }
            rowan::NodeOrToken::Node(node) => match node.kind() {
                SyntaxKind::EMPTY_LINE | SyntaxKind::ANNOTATION_LINE => {}
                SyntaxKind::TAG_LINE => {
                    let Some(tl) = ast::TagLine::cast(node) else {
                        break;
                    };
                    match classify_tag_line(&tl) {
                        LineKind::Visibility(range) => {
                            count += 1;
                            found = Some(range);
                        }
                        LineKind::OtherDirective | LineKind::NotDirective => {}
                    }
                }
                _ => break,
            },
        }
    }
    (count == 1).then_some(found).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fix::fixes_at;
    use crate::session::IdeSession;
    use brink_ir::Visibility;

    fn session_with(dialect: brink_analyzer::Dialect, path: &str, src: &str) -> IdeSession {
        let mut session = IdeSession::new();
        session.set_language_dialect(dialect);
        session.update_source(path, src.to_string());
        session.update_and_analyze(path, src.to_string());
        session
    }

    fn applied(src: &str, fix: &Fix) -> String {
        let mut out = src.to_owned();
        let mut edits: Vec<&FileEdit> = fix.edits.iter().collect();
        edits.sort_by_key(|e| std::cmp::Reverse(e.range.start()));
        for e in edits {
            out.replace_range(
                usize::from(e.range.start())..usize::from(e.range.end()),
                &e.new_text,
            );
        }
        out
    }

    /// The `Visibility` the analyzer resolves for the symbol named `name` —
    /// used to prove visibility resolution is unchanged by the fix, not just
    /// that the diagnostic clears.
    fn resolved_visibility(session: &IdeSession, name: &str) -> Visibility {
        let index = session.db().symbol_index();
        let ids = index.by_name.get(name);
        assert!(ids.is_some(), "no symbol named `{name}` in the index");
        let ids = ids.expect("just asserted above");
        assert_eq!(
            ids.len(),
            1,
            "expected exactly one symbol named `{name}`, got {ids:?}"
        );
        let info = index.symbols.get(&ids[0]);
        assert!(
            info.is_some(),
            "id in by_name but not in symbols: {:?}",
            ids[0]
        );
        info.expect("just asserted above").visibility
    }

    fn only_fix(fixes: &[Fix]) -> &Fix {
        assert_eq!(
            fixes.len(),
            1,
            "{:?}",
            fixes.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert_eq!(fixes[0].code, DiagnosticCode::E092);
        assert_eq!(fixes[0].applicability, Applicability::Safe);
        &fixes[0]
    }

    // ── VAR: `directives_before` attachment, undeclared module ──────────

    #[test]
    fn e092_ink_var_redundant_public_removes_the_whole_line() {
        let src = "#@public\nVAR score = 0\nHello, world.\n~ score = score + 1\nScore is {score}.\n-> DONE\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("score").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        let fix = only_fix(&fixes);
        assert_eq!(
            applied(src, fix),
            "VAR score = 0\nHello, world.\n~ score = score + 1\nScore is {score}.\n-> DONE\n"
        );
    }

    #[test]
    fn e092_visibility_resolution_is_identical_before_and_after() {
        // Proves the fix does not merely clear the diagnostic — the
        // analyzer's *resolved* visibility for the definition (not just the
        // raw `#@public` mark) is the exact same value on both sides.
        let src = "#@public\nVAR score = 0\nHello, world.\n-> DONE\n";
        let before_session = session_with(brink_analyzer::Dialect::Brink, "test.ink", src);
        let before_vis = resolved_visibility(&before_session, "score");
        assert_eq!(before_vis, Visibility::Public);

        let file = before_session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("score").expect("cursor site")).expect("fits");
        let cx = FixCx::new(before_session.db());
        let fixes = fixes_at(&cx, file, off);
        let fix = only_fix(&fixes);
        let patched = applied(src, fix);

        let after_session = session_with(brink_analyzer::Dialect::Brink, "test.ink", &patched);
        let after_vis = resolved_visibility(&after_session, "score");
        assert_eq!(
            after_vis, before_vis,
            "resolved visibility must be identical before/after the fix"
        );
    }

    #[test]
    fn e092_reanalysis_clears_the_diagnostic() {
        let src = "#@public\nVAR score = 0\nHello, world.\n-> DONE\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("score").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        let fix = only_fix(&fixes);
        let patched = applied(src, fix);

        let after = session_with(brink_analyzer::Dialect::Brink, "test.ink", &patched);
        let after_file = after.file_id("test.ink").expect("file id");
        let diags = after.db().diagnostics(after_file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E092),
            "{diags:?}"
        );
    }

    // ── CONST/EXTERNAL/STRUCT: same `directives_before` attachment ──────

    #[test]
    fn e092_ink_const_redundant_public_removes_the_whole_line() {
        let src = "#@public\nCONST SPEED = 3\nGo.\n-> DONE\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("SPEED").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        let fix = only_fix(&fixes);
        assert_eq!(applied(src, fix), "CONST SPEED = 3\nGo.\n-> DONE\n");
    }

    #[test]
    fn e092_ink_external_redundant_public_removes_the_whole_line() {
        let src = "#@public\nEXTERNAL ping()\nHi.\n-> DONE\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("ping").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        let fix = only_fix(&fixes);
        assert_eq!(applied(src, fix), "EXTERNAL ping()\nHi.\n-> DONE\n");
    }

    // ── KNOT: `leading_body_directives` attachment, declared module ─────

    #[test]
    fn e092_ink_knot_redundant_private_in_declared_module_removes_the_whole_line() {
        // A declared module defaults Private (`docs/modules-spec.md` §4), so
        // an explicit `#@private` on a knot in this file is redundant —
        // mirrors `brink_db::db`'s own
        // `redundant_private_in_declared_module_is_e092` end-to-end test.
        let src = "#@module(quest)\n=== ambush ===\n#@private\nHi\n-> DONE\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("ambush").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        let fix = only_fix(&fixes);
        assert_eq!(
            applied(src, fix),
            "#@module(quest)\n=== ambush ===\nHi\n-> DONE\n"
        );
    }

    #[test]
    fn e092_ink_knot_reanalysis_clears_the_diagnostic() {
        let src = "#@module(quest)\n=== ambush ===\n#@private\nHi\n-> DONE\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("ambush").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        let fix = only_fix(&fixes);
        let patched = applied(src, fix);

        let after = session_with(brink_analyzer::Dialect::Brink, "test.ink", &patched);
        let after_file = after.file_id("test.ink").expect("file id");
        let diags = after.db().diagnostics(after_file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E092),
            "{diags:?}"
        );
        let parse = brink_syntax::parse(&patched);
        assert!(parse.errors().is_empty(), "{:?}", parse.errors());
    }

    // ── STITCH: `leading_body_directives` on a nested body ───────────────

    #[test]
    fn e092_ink_stitch_redundant_public_removes_the_whole_line() {
        let src = "=== town ===\nHi\n= market\n#@public\nStalls.\n-> DONE\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("market").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        let fix = only_fix(&fixes);
        assert_eq!(
            applied(src, fix),
            "=== town ===\nHi\n= market\nStalls.\n-> DONE\n"
        );
    }

    // ── narrowing: native has no tag to remove ──────────────────────────

    #[test]
    fn e092_native_no_fix_regardless_of_dispatch() {
        // A native `.brink` file's module is always `declared`
        // (`brink_db::queries::module_map_query`'s own doc: "marked
        // `declared` so it always qualifies `DefinitionId`"), so the
        // declared-module default is always `Private` and native's own
        // `pub` mark (`Some(VisibilityMark::Public)`, the only non-`None`
        // mark native lowering ever produces) is never redundant there in
        // practice — `E092` cannot currently fire on a native file at all.
        // This test is defense-in-depth against `crate::fix::fixes_for`'s
        // dispatch, which is keyed only on `d.code` and does not itself
        // guarantee a diagnostic's file matches the dialect this fixer
        // assumes: this module only ever parses with the ink grammar, so
        // handing it a native file must return nothing rather than run that
        // parser over native syntax and risk a bogus match.
        let src = "pub var score = 0\nflow main() {\n  Hello\n  -> END\n}\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.brink", src);
        let file = session.file_id("test.brink").expect("file id");
        assert!(
            session
                .db()
                .diagnostics(file)
                .expect("diagnostics")
                .iter()
                .all(|d| d.code != DiagnosticCode::E092),
            "native `pub` is not redundant here — declared defaults Private"
        );
        assert!(session.db().is_native(file));

        let off = u32::try_from(src.find("score").expect("cursor site")).expect("fits");
        let name_range = {
            let index = session.db().symbol_index();
            let id = index.by_name.get("score").expect("declared")[0];
            index.symbols.get(&id).expect("info").range
        };
        assert!(name_range.contains_inclusive(off.into()));
        let d = Diagnostic {
            file,
            range: name_range,
            message: DiagnosticCode::E092.title().to_owned(),
            code: DiagnosticCode::E092,
        };
        let cx = FixCx::new(session.db());
        assert!(crate::fix::fixes_for(&cx, &d).is_empty());
    }

    // ── narrowing: two visibility directives is a conflict, not a target ──

    #[test]
    fn e092_no_fix_when_two_visibility_directives_stack() {
        // `#@public` then `#@private` immediately above the same VAR is
        // itself a conflict (`E093` on the second one);
        // `visibility_from_directives` keeps the *first* occurrence
        // (`Public`) as `chosen`, which — undeclared module, default
        // Public — is also redundant, so `E092` fires too. This fixer
        // declines rather than guess which line the diagnostic means.
        let src = "#@public\n#@private\nVAR score = 0\nHello.\n-> DONE\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session.db().diagnostics(file).expect("diagnostics");
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E092),
            "fixture must actually raise E092: {diags:?}"
        );
        let off = u32::try_from(src.find("score").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
    }

    // ── a non-redundant visibility directive offers nothing ──────────────

    #[test]
    fn e092_no_offer_when_the_directive_is_not_redundant() {
        let src = "#@private\nVAR secret = 0\nHello.\n-> DONE\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session.db().diagnostics(file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E092),
            "{diags:?}"
        );
        let off = u32::try_from(src.find("secret").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
    }
}
