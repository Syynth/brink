//! `Safe` fixer for `E014` ("logic line has no effect"): delete a bare `~`
//! line that resolves to nothing at all — issue #3423, milestone 8 of the
//! auto-fix epic (#3374, `docs/autofix-spec.md` §9's first-wave `Safe` list:
//! "E014 bare `~` → delete the line"). `tests/fix/E014/` (before/expected)
//! predates this fixer by design — see that fixture's own `README.md` — and
//! is what this module is measured against; `expected_verdicts()` in
//! `crates/internal/brink-test-harness/tests/fix_safe_obligations.rs`
//! already pins its verdict to `ObservablyEquivalent`.
//!
//! # Which `E014` sites this covers
//!
//! `E014` is not one CST shape and is raised from **fifteen** call sites
//! across three files, not one shape or a single small cluster:
//! `hir::lower::content::logic_line`'s `impl LowerBody for ast::LogicLine`
//! (five: a bare `~` line's own five accessor checks), `hir::lower::content::
//! logic_block`'s `~ { … }` block-statement lowerers (six: `TempDecl`'s
//! missing identifier/name and `ForStmt`'s missing identifier/name, two each,
//! plus `Assignment`'s missing target/value inside a block), and
//! `hir::lower_native::control_flow` (four, not three, on the native
//! surface). Only **one** of the fifteen is "a logic line with no effect" in
//! the sense this fixer discharges — the trailing catch-all
//! (`logic_line.rs`'s last statement, `Err(sink.diagnose(range,
//! DiagnosticCode::E014))`), reached only when **none** of
//! `stmt_block()`/`await_stmt()`/`return_stmt()`/`temp_decl()`/
//! `assignment()` matched and no direct child casts to `ast::Expr` either —
//! i.e. `~` followed by nothing the grammar recognizes as a statement or
//! expression at all (a bare `~` immediately followed by end-of-line, or by
//! a token `atom()` doesn't start an expression with).
//!
//! The other **four** sites in `logic_line.rs` itself raise the identical
//! `E014` code from a **malformed** partial parse: `self.temp_decl()` is
//! `Some` but its `identifier()` is empty (`~ temp = 5` — a name failed to
//! parse), or `self.assignment()` is `Some` but its `target()`/`value()` is
//! missing (`~ x =` with nothing after the `=`). Those are error-recovery
//! diagnostics over a real (partial) construct, not an effect-free line —
//! deleting `~ x =` when `x`'s old value might still be read downstream, or
//! deleting `~ temp =` when the right-hand side might carry a call, is not
//! "no effect", so a `Safe` fixer must never touch them.
//! [`empty_logic_line_deletion`]'s structural check — every one of those
//! five accessors must be `None` — is exactly what excludes them: any of the
//! five being non-`None` is *itself* one of those four sites, never this
//! one.
//!
//! `logic_block.rs`'s six block-statement sites (`~ { temp = 5 }` missing a
//! name, `~ { x = }` missing a target/value, `~ { for = in y {} }` missing a
//! loop variable's name) are excluded by a *different* mechanism than the
//! accessor check above, because they never reach it: each one diagnoses at
//! the **inner** `TempDecl`/`Assignment`/`ForStmt` node's own
//! `.syntax().text_range()`, not the enclosing `LogicLine`'s range, so
//! [`empty_logic_line_deletion`]'s exact-range lookup (`find(|ll|
//! ll.syntax().text_range() == target_range)`) never finds a `LogicLine` at
//! that range at all and the leading `?` returns `None` before the five
//! accessors are even consulted. `control_flow.rs`'s four native sites are
//! narrowed the same way this module's "Why native never reaches this
//! fixer" section below describes: the dialect gate in [`fix`] refuses
//! before any structural check runs, for these as for the ordinary
//! bare-`~` shape.
//!
//! # Why native never reaches this fixer
//!
//! Native's own "nothing after `~`" shape parses differently: `~` with
//! nothing recognized falls through `parser::stmt::logic_line`'s dispatch to
//! `expr_stmt_line`, which unconditionally opens an `EXPR_STMT` node; HIR
//! lowering (`lower_native::body::lower_logic_line_expr_stmt`) then finds
//! `ll.expr_stmt()` is `Some` but its `.expr()` is empty and raises `E015`
//! ("expression is missing an operand"), never `E014` — so there is no
//! native shape this fixer's own diagnostic code is even raised for. Guarded
//! by [`fix`]'s dialect check and
//! `no_fix_on_native_even_for_its_own_malformed_e014_shape` below (proving
//! the *other* native `E014` sites — the malformed-partial ones — are
//! refused too, same as ink's).
//!
//! # Re-deriving effect-freedom, not trusting the diagnostic
//!
//! The diagnostic alone only says "lowering couldn't find a statement here";
//! this module never takes that on faith. [`empty_logic_line_deletion`]
//! re-parses the source and checks, from the CST itself, that the located
//! `LogicLine` carries none of the five recognized statement kinds and no
//! `Expr` child — mirroring `logic_line.rs`'s own predicate exactly, so a
//! disagreement between the two would be a bug in the mirror, not a trusted
//! shortcut. Given that predicate holds, the node's own children are
//! nothing but the `~` token and trivia (`atom()`'s `_ => false` arm
//! consumes no tokens and builds no node — see the doc on
//! [`empty_logic_line_deletion`] for the token-level argument for why no
//! call, assignment, increment, external reference, or list op can be
//! hiding in there).

use brink_db::ProjectDb;
use brink_ir::{Diagnostic, DiagnosticCode};
use rowan::TextRange;

use crate::fix::{Applicability, Fix, FixCx, Fixer};
use crate::rename::FileEdit;

/// The `E014` fixer: delete a bare `~` line (and its line break) that
/// resolves to no statement and no expression at all.
pub struct EmptyLogicLineFixer;

impl Fixer for EmptyLogicLineFixer {
    fn code(&self) -> DiagnosticCode {
        DiagnosticCode::E014
    }

    fn max_applicability(&self) -> Applicability {
        Applicability::Safe
    }

    fn fixes(&self, cx: &FixCx<'_>, d: &Diagnostic) -> Vec<Fix> {
        fix(cx.db, d)
    }
}

/// Ink-only (see this module's doc, "Why native never reaches this fixer").
fn fix(db: &ProjectDb, d: &Diagnostic) -> Vec<Fix> {
    if db.is_native(d.file) {
        return Vec::new();
    }
    let Some(source) = db.source(d.file) else {
        return Vec::new();
    };
    let Some(range) = empty_logic_line_deletion(source, d.range) else {
        return Vec::new();
    };
    vec![Fix {
        code: DiagnosticCode::E014,
        title: "Remove effect-free `~` line".to_owned(),
        applicability: Applicability::Safe,
        edits: vec![FileEdit {
            file: d.file,
            range,
            new_text: String::new(),
        }],
        caret: None,
    }]
}

/// Whether every byte of `source[range]` is an ASCII space or tab —
/// deliberately narrower than `char::is_whitespace` (no newlines, no
/// Unicode whitespace) since the only thing either side of the line is
/// allowed to hold is plain indentation.
fn is_blank(bytes: &[u8]) -> bool {
    bytes.iter().all(|b| *b == b' ' || *b == b'\t')
}

/// The deletion range for a genuinely effect-free `~` line at `target_range`,
/// or `None` when the shape at that range is not provably this fixer's own
/// (either it is one of `E014`'s other fourteen raise sites — a malformed
/// partial construct, on either surface, or a `~ { … }` block statement whose
/// range never matches a `LogicLine` at all — or something else entirely
/// sits there, or the line is not clean enough on either side of the node to
/// delete safely).
///
/// `target_range` is `d.range` verbatim — `logic_line.rs`'s five call sites
/// all diagnose at `self.syntax().text_range()`, the whole `LogicLine` node's
/// own span, so a plain equality lookup on the re-parsed tree finds the exact
/// node the diagnostic was raised against (same range contract
/// `arity_trim_fix`'s own `expected_param_count` doc cites for `d.range`,
/// issue #1561).
///
/// # Why the structural check alone proves effect-freedom
///
/// A `LogicLine` for which every one of `stmt_block()`/`await_stmt()`/
/// `return_stmt()`/`temp_decl()`/`assignment()` is `None` **and** no direct
/// child casts to `ast::Expr` can only arise one way in this grammar
/// (`brink_syntax::parser::logic::logic_line`): the dispatch's catch-all arm
/// (`_ => { expression(p); }`) called `expression_bp` → `atom(p)`, and
/// `atom` returned `false` from its own catch-all (`_ => false`) — the only
/// arm that builds no node and advances no token. Every other `atom` arm
/// either builds a real `Expr`-castable node (a call, a literal, a path, a
/// `ref`/prefix expression, a list/struct/array/map literal) or recurses
/// into one — any of which would show up as the `Expr` child this check
/// already rules out. So the located node's only children are the `~` token
/// itself plus whitespace/newline trivia: no call, no assignment (the
/// `Assignment`/`TempDecl` node kinds are ruled out directly), no
/// `++`/`--` (a `POSTFIX_EXPR` only ever wraps an already-parsed `Expr`,
/// which this check also rules out), no external reference, no list op —
/// there is no sub-node left for any of those to be attached to. This is a
/// mirror of `logic_line.rs`'s own predicate (re-derived from the CST, not
/// read off the diagnostic), not a copy of it: an actual disagreement
/// between the two would surface as `every_fixture_matches_its_fixer`
/// (`brink_ide::fix::tests`) or `assert_safe_fix`
/// (`brink_test_harness::fix`) failing, not as this fixer silently trusting
/// something it didn't check.
fn empty_logic_line_deletion(source: &str, target_range: TextRange) -> Option<TextRange> {
    use brink_syntax::SyntaxKind;
    use brink_syntax::ast::{AstNode as _, Expr, LogicLine};
    use rowan::NodeOrToken;

    let parse = brink_syntax::parse(source);
    let tree = parse.tree();
    let root = tree.syntax().clone();

    let line = root
        .descendants()
        .filter_map(LogicLine::cast)
        .find(|ll| ll.syntax().text_range() == target_range)?;

    if line.stmt_block().is_some()
        || line.await_stmt().is_some()
        || line.return_stmt().is_some()
        || line.temp_decl().is_some()
        || line.assignment().is_some()
    {
        return None;
    }
    if line.syntax().children().any(|c| Expr::cast(c).is_some()) {
        return None;
    }

    // A comment is trivia to the parser (`Parser::skip_ws`), so it is
    // swallowed into this node's own token run rather than surfacing as a
    // child the checks above would see — deleting the line would silently
    // drop the author's authored text, which is never `Safe` (review
    // finding on #3423: `~ // TODO: ...` and `~ /* keep this note */` both
    // lower to a childless `LogicLine` here, same as a genuinely bare `~`).
    let has_comment_trivia = line.syntax().children_with_tokens().any(|c| {
        matches!(
            c,
            NodeOrToken::Token(t)
                if t.kind() == SyntaxKind::LINE_COMMENT || t.kind() == SyntaxKind::BLOCK_COMMENT
        )
    });
    if has_comment_trivia {
        return None;
    }

    // A block comment spans multiple physical lines as one trivia token, so
    // the check above is not a complete backstop against multi-line content
    // hiding in this node — refuse outright whenever the node's own range
    // covers more than one physical line. `docs/decision-log.md`'s "the
    // deletion range extends to the whole physical line" ruling is about
    // extending a single-line node's range to that line's own boundaries,
    // never about deleting several lines that happened to parse into one
    // node.
    let node_text = &source[usize::from(target_range.start())..usize::from(target_range.end())];
    if node_text.trim_end_matches('\n').contains('\n') {
        return None;
    }

    let bytes = source.as_bytes();
    let start = usize::from(target_range.start());
    let end = usize::from(target_range.end());

    // Extend the start back to the beginning of the physical line, refusing
    // when anything but plain indentation sits between the two — real
    // content there (never observed reachable from this grammar shape, but
    // never assumed) would be deleted right along with the `~` line, which
    // is not "no effect" for it.
    let line_start = bytes[..start]
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(0, |i| i + 1);
    if !is_blank(&bytes[line_start..start]) {
        return None;
    }

    // The node's own range already includes its trailing `NEWLINE` in the
    // ordinary case (`logic_line`'s own `if p.at(NEWLINE) { p.bump(); }`
    // before `finish_node()`) or reaches end-of-file on a last line with
    // none. Either way there is nothing left on this physical line to check
    // or extend into.
    let already_terminated = end == bytes.len() || bytes.get(end.wrapping_sub(1)) == Some(&b'\n');
    let line_end = if already_terminated {
        end
    } else {
        // The node stopped short of the line's real end — the parser hit a
        // token `atom()` doesn't start an expression with and left it (and
        // everything after, up to the real newline) unconsumed. Only plain
        // trailing whitespace there is safe to fold into the deletion;
        // anything else means real content trails on the same line, which
        // this fixer must not silently drop.
        match bytes[end..].iter().position(|b| *b == b'\n') {
            Some(rel) if is_blank(&bytes[end..end + rel]) => end + rel + 1,
            None if is_blank(&bytes[end..]) => bytes.len(),
            _ => return None,
        }
    };

    let start = u32::try_from(line_start).ok()?;
    let end = u32::try_from(line_end).ok()?;
    if end <= start {
        return None;
    }
    Some(TextRange::new(start.into(), end.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fix::fixes_at;
    use crate::session::IdeSession;

    fn ink_session(src: &str) -> IdeSession {
        let mut session = IdeSession::new();
        session.set_language_dialect(brink_analyzer::Dialect::Brink);
        session.update_source("test.ink", src.to_string());
        session.update_and_analyze("test.ink", src.to_string());
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

    #[test]
    fn e014_drops_the_bare_tilde_line() {
        // Mirrors `tests/fix/E014/{before,expected}.ink` exactly.
        let src = "VAR score = 0\nHello.\n~\n~ score = score + 1\nScore is {score}.\n* [Go on]\n  Onward.\n  -> DONE\n";
        let session = ink_session(src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("~\n").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(
            fixes.len(),
            1,
            "{:?}",
            fixes.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert_eq!(fixes[0].code, DiagnosticCode::E014);
        assert_eq!(fixes[0].applicability, Applicability::Safe);
        assert_eq!(
            applied(src, &fixes[0]),
            "VAR score = 0\nHello.\n~ score = score + 1\nScore is {score}.\n* [Go on]\n  Onward.\n  -> DONE\n"
        );
    }

    #[test]
    fn e014_reanalysis_clears_the_diagnostic() {
        let src = "VAR score = 0\nHello.\n~\n~ score = score + 1\n-> DONE\n";
        let session = ink_session(src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("~\n").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(fixes.len(), 1);
        let patched = applied(src, &fixes[0]);

        let after = ink_session(&patched);
        let after_file = after.file_id("test.ink").expect("file id");
        let diags = after.db().diagnostics(after_file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E014),
            "{diags:?}"
        );
    }

    #[test]
    fn e014_drops_an_indented_bare_tilde_inside_a_choice() {
        // The generalization beyond the flush-left fixture: leading
        // indentation before the `~` must be deleted too, or the two spaces
        // would land in front of `Onward.` instead.
        let src = "=== main ===\n* [Go]\n  ~\n  Onward.\n  -> DONE\n";
        let session = ink_session(src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("~\n").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(
            fixes.len(),
            1,
            "{:?}",
            fixes.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert_eq!(
            applied(src, &fixes[0]),
            "=== main ===\n* [Go]\n  Onward.\n  -> DONE\n"
        );
    }

    #[test]
    fn e014_drops_a_bare_tilde_at_eof_with_no_trailing_newline() {
        // A bare `~` as the very last byte of the file: the node's own range
        // reaches EOF directly (no newline to delete), and there is nothing
        // to extend into either side. Still a valid, if degenerate, `Safe`
        // deletion.
        let src = "Hello.\n~";
        let session = ink_session(src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find('~').expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(fixes.len(), 1);
        assert_eq!(applied(src, &fixes[0]), "Hello.\n");
    }

    // ── narrowing: the other fourteen `E014` raise sites are refused ─────

    #[test]
    fn e014_no_fix_when_temp_decl_is_missing_its_identifier() {
        // `~ temp = 5` — `self.temp_decl()` is `Some`, but its identifier
        // failed to parse. A different `E014` raise site
        // (`logic_line.rs`'s first `ok_or_else`) from the same code; the
        // line is not effect-free (`5` may still be worth evaluating, and
        // more importantly this is a malformed construct, not an empty
        // one), so no fix may be offered.
        let src = "=== main ===\n~ temp = 5\n-> DONE\n";
        let session = ink_session(src);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session
            .db()
            .diagnostics(file)
            .expect("diagnostics")
            .to_vec();
        let found = diags.iter().find(|d| d.code == DiagnosticCode::E014);
        assert!(found.is_some(), "expected an E014 diagnostic: {diags:?}");
        let target = found.expect("just asserted above");
        let cx = FixCx::new(session.db());
        let offered = crate::fix::fixes_for(&cx, target);
        assert!(
            offered.is_empty(),
            "{:?}",
            offered.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    #[test]
    fn e014_no_fix_when_assignment_is_missing_its_value() {
        // `~ score =` — `self.assignment()` is `Some`, `target()` is
        // present, but `value()` is missing. Another of the seven malformed
        // raise sites, not the empty-line shape.
        let src = "VAR score = 0\n=== main ===\n~ score =\n-> DONE\n";
        let session = ink_session(src);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session
            .db()
            .diagnostics(file)
            .expect("diagnostics")
            .to_vec();
        let found = diags.iter().find(|d| d.code == DiagnosticCode::E014);
        assert!(found.is_some(), "expected an E014 diagnostic: {diags:?}");
        let target = found.expect("just asserted above");
        let cx = FixCx::new(session.db());
        let offered = crate::fix::fixes_for(&cx, target);
        assert!(
            offered.is_empty(),
            "{:?}",
            offered.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_fix_on_native_even_for_its_own_malformed_e014_shape() {
        // Native's own `E014` sites are all malformed-partial (missing
        // name/place/value on `let`/`assign`/`for`) — this module's doc,
        // "Why native never reaches this fixer". A content-ground `~ let`
        // with a missing name is one of them; regardless of shape, the
        // dialect gate in `fix` refuses before any structural check runs.
        let mut session = IdeSession::new();
        session.set_language_dialect(brink_analyzer::Dialect::Brink);
        let src = "flow main() {\n  ~ let = 5\n}\n";
        session.update_source("test.brink", src.to_string());
        session.update_and_analyze("test.brink", src.to_string());
        let file = session.file_id("test.brink").expect("file id");
        let diags = session
            .db()
            .diagnostics(file)
            .expect("diagnostics")
            .to_vec();
        let found = diags.iter().find(|d| d.code == DiagnosticCode::E014);
        assert!(found.is_some(), "expected an E014 diagnostic: {diags:?}");
        let target = found.expect("just asserted above");
        let cx = FixCx::new(session.db());
        let offered = crate::fix::fixes_for(&cx, target);
        assert!(
            offered.is_empty(),
            "{:?}",
            offered.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    #[test]
    fn e014_no_fix_when_the_bare_tilde_carries_a_trailing_line_comment() {
        // A comment is trivia to the parser and is swallowed into the
        // `LogicLine`'s own token run, so the five accessors and the
        // `Expr`-child scan alone cannot see it. Deleting the line would
        // silently drop the author's `TODO`, which is not `Safe` — review
        // finding on #3423.
        let src =
            "VAR score = 0\nHello.\n~ // TODO: bump the score here\nScore is {score}.\n-> DONE\n";
        let session = ink_session(src);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session
            .db()
            .diagnostics(file)
            .expect("diagnostics")
            .to_vec();
        let found = diags.iter().find(|d| d.code == DiagnosticCode::E014);
        assert!(found.is_some(), "expected an E014 diagnostic: {diags:?}");
        let target = found.expect("just asserted above");
        let cx = FixCx::new(session.db());
        let offered = crate::fix::fixes_for(&cx, target);
        assert!(
            offered.is_empty(),
            "{:?}",
            offered.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    #[test]
    fn e014_no_fix_when_the_bare_tilde_carries_a_multiline_block_comment() {
        // A multi-line `/* … */` is one `BLOCK_COMMENT` trivia token, so the
        // `LogicLine` node's own range spans several physical lines. Even
        // setting the comment-token check aside, the multi-physical-line
        // guard must refuse this on its own — deleting the node's whole
        // range here would drop three authored lines, not one. Review
        // finding on #3423.
        let src = "VAR score = 0\nHello.\n~ /* keep\n   this note\n   please */\nScore is {score}.\n-> DONE\n";
        let session = ink_session(src);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session
            .db()
            .diagnostics(file)
            .expect("diagnostics")
            .to_vec();
        let found = diags.iter().find(|d| d.code == DiagnosticCode::E014);
        assert!(found.is_some(), "expected an E014 diagnostic: {diags:?}");
        let target = found.expect("just asserted above");
        let cx = FixCx::new(session.db());
        let offered = crate::fix::fixes_for(&cx, target);
        assert!(
            offered.is_empty(),
            "{:?}",
            offered.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    #[test]
    fn e014_no_fix_for_a_malformed_block_statement_temp_decl() {
        // `~ { temp = 5 }` — a `~ { … }` block-statement `TempDecl` missing
        // its name. `logic_block.rs`'s `lower_block_temp_decl` raises `E014`
        // at the inner `TempDecl` node's own range, not any `LogicLine`'s,
        // so `empty_logic_line_deletion`'s exact-range `LogicLine` lookup
        // never finds a match here — pinning that this is what excludes the
        // shape, not the five-accessor check (which never runs).
        let src = "VAR score = 0\n=== main ===\n~ {\n  temp = 5\n}\nScore is {score}.\n-> DONE\n";
        let session = ink_session(src);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session
            .db()
            .diagnostics(file)
            .expect("diagnostics")
            .to_vec();
        let found = diags.iter().find(|d| d.code == DiagnosticCode::E014);
        assert!(found.is_some(), "expected an E014 diagnostic: {diags:?}");
        let target = found.expect("just asserted above");
        let cx = FixCx::new(session.db());
        let offered = crate::fix::fixes_for(&cx, target);
        assert!(
            offered.is_empty(),
            "{:?}",
            offered.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }
}
