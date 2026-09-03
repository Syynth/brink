//! `Safe` fixer for `E110` (the deprecated `#@effects(…)` tag-channel
//! spelling): rewrite the whole tag line to the `@[effects(…)]` annotation
//! spelling, translating the argument list from the legacy **colon**
//! mini-grammar (`reads: gold, hp`) to the annotation's **paren-clause**
//! mini-grammar (`reads(gold, hp)`) — issue #3426, milestone 8 of the
//! auto-fix epic (#3374).
//!
//! # Why the argument list needs translating, not copying
//!
//! The two spellings do not share an argument grammar. `#@effects(…)`
//! freezes the legacy colon shape forever (`brink_ir::hir::lower::directive`'s
//! `parse_effects_clauses` — "the E110-warned surface does not evolve"),
//! while `@[effects(…)]` uses the amended Rust-meta-item paren-clause shape
//! (`parse_effects_paren_clauses`, stdlib-spec §9.2, 2026-07-19). A byte-for-byte
//! copy of the parenthesised argument text into the new spelling would
//! therefore hand the annotation-side parser a string in the *wrong*
//! grammar — `@[effects(reads: gold, hp)]` is malformed there (`E101`: a
//! bare top-level identifier is always a flag under the paren-clause
//! grammar, so `hp` after `reads:` is nonsense, and `reads:` itself is not
//! a clause opener at all in that grammar).
//!
//! So this fixer parses the legacy colon text into the same
//! `{pure, silent, total, reads, writes, calls}` shape
//! [`brink_ir::EffectsAssertion`] carries, then re-renders it as paren
//! clauses. [`parse_legacy_effects`] below is an independently-tested port
//! of `parse_effects_clauses`'s algorithm — it cannot call the original
//! directly (that function is `pub(super)` to `brink-ir`'s lowering
//! internals, not part of this crate's dependency surface) — but it is
//! never trusted to *certify* the fix on its own: it only decides whether a
//! translation is safe to offer and what text to write. The actual proof
//! that the rewritten annotation preserves the definition's effects is the
//! fixture test at the bottom of this module, which re-parses the produced
//! `@[effects(…)]` line through the real production pipeline and compares
//! the analyzer's own inferred effect row before and after
//! (`brink_ide::effects::EffectRowView`) — a bug in this module's parser
//! would show up there as a failing test, not a silently-wrong "safe" fix.
//!
//! # Scope: ink only, knot/stitch leading run only
//!
//! `#@effects(…)` is a `brink-syntax` (ink) tag-channel directive; the
//! native `.brink` surface has no such alias — `brink_ir::hir::lower_native::
//! annotation` recognizes only the `@[effects(…)]` spelling — so `E110`
//! never fires there, and this fixer bails immediately on a native file.
//!
//! `effects_assertion_from_directives` (the only place `E110` is raised) is
//! called from exactly two sites — `hir::lower::structure::knot` and
//! `::stitch` — both feeding it `leading_body_directives(&body)`, which
//! collects tags/annotations from a `KNOT_BODY`/`STITCH_BODY`'s **leading
//! run** (interleaved trivia, empty lines, tag lines, and annotation lines
//! only) that are not themselves attached to a following `VAR`/`CONST`/
//! `LIST`/`EXTERNAL` declaration. So every reachable `E110` site is a
//! `TAG_LINE` already sitting in a position an `@[…]` annotation line is
//! equally legal in (`in_leading_annotation_run` accepts the same three
//! sibling kinds `in_leading_body_run` does) — replacing the tag line's own
//! text in place, on the same line, keeps it exactly where the definition's
//! annotations already go. No repositioning logic is needed.
//!
//! # Narrowing
//!
//! No fix is offered when the tag:
//!
//! - is on the **native** surface (no such alias exists there to begin
//!   with);
//! - is **dynamic** (carries an `InlineLogic` `{…}` child) — directives must
//!   be static, and text-slicing around an inline-logic node the way
//!   [`brink_syntax::ast::Tag::text`] does would silently drop the dynamic
//!   part rather than translate it;
//! - has **no argument list** at all (bare `#@effects`, or an unbalanced
//!   paren) — nothing to translate, and the diagnostic here is `E100`, not
//!   this fixer's business;
//! - fails to **parse** under the legacy colon grammar (an unknown clause
//!   key, a non-identifier value, or a `pure`/clause contradiction) — the
//!   diagnostic here is `E101`, and guessing at a malformed author intent
//!   is exactly what a `Safe` fixer must refuse to do.
//!
//! Any of these narrows to "no fix" rather than emitting a best-effort
//! guess — `docs/autofix-spec.md` §3's "narrow, never downgrade".

use brink_db::ProjectDb;
use brink_ir::{Diagnostic, DiagnosticCode};

use crate::fix::{Applicability, Fix, FixCx, Fixer};
use crate::rename::FileEdit;

/// The `E110` fixer: rewrite `#@effects(…)` to `@[effects(…)]`.
pub struct EffectsTagFixer;

impl Fixer for EffectsTagFixer {
    fn code(&self) -> DiagnosticCode {
        DiagnosticCode::E110
    }

    fn max_applicability(&self) -> Applicability {
        Applicability::Safe
    }

    fn fixes(&self, cx: &FixCx<'_>, d: &Diagnostic) -> Vec<Fix> {
        effects_tag_fix(cx.db, d)
    }
}

/// The one edit this fixer ever offers: the whole `TAG_LINE`'s text,
/// replaced with the `@[effects(…)]` spelling of the same assertion.
fn effects_tag_fix(db: &ProjectDb, d: &Diagnostic) -> Vec<Fix> {
    use brink_syntax::ast::{AstNode as _, Tag};

    // `#@effects` has no native counterpart (module doc, "Scope").
    if db.is_native(d.file) {
        return Vec::new();
    }
    let Some(source) = db.source(d.file) else {
        return Vec::new();
    };
    let parse = brink_syntax::parse(source);
    let root = parse.tree().syntax().clone();

    // `d.range` is exactly `tag.syntax().text_range()` — the same range
    // `parse_directive_tag` stamped the `ParsedDirective` with (mirrors
    // `arity_trim_fix`'s own range-equality lookup for `d.range`).
    let Some(tag) = root
        .descendants()
        .filter_map(Tag::cast)
        .find(|t| t.syntax().text_range() == d.range)
    else {
        return Vec::new();
    };

    // A dynamic tag carries an `InlineLogic` node child; `Tag::text()` only
    // walks tokens, so it would silently drop that part rather than
    // translate it. Refuse rather than guess.
    let is_dynamic = tag
        .syntax()
        .children_with_tokens()
        .any(|el| el.as_node().is_some());
    if is_dynamic {
        return Vec::new();
    }

    // `Tag::text()` strips the leading `#` and trims — exactly the
    // `trimmed` value `parse_directive_tag` computes.
    let trimmed = tag.text();
    let Some(rest) = trimmed.strip_prefix('@') else {
        return Vec::new();
    };
    let name: String = rest
        .chars()
        .take_while(|c| *c != '(' && !c.is_whitespace())
        .collect();
    if name != "effects" {
        // Dispatch is by diagnostic code, so this should not happen — but
        // never rewrite a tag whose name this fixer did not verify.
        return Vec::new();
    }
    let after_name = rest.trim_start_matches(|c: char| c != '(');
    let Some(arg) = after_name
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .map(|s| s.trim().to_string())
    else {
        // Bare `#@effects` or an unbalanced paren — `E100`'s territory.
        return Vec::new();
    };

    let Some(parsed) = parse_legacy_effects(&arg) else {
        // Malformed under the legacy grammar — `E101`'s territory. Do not
        // guess at what the author meant.
        return Vec::new();
    };

    let new_args = render_annotation_args(&parsed);
    // Replace exactly the `Tag` node's own range (`#@effects(…)`), not the
    // enclosing `TAG_LINE`'s — the parser bumps the line's trailing NEWLINE
    // token *inside* `TAG_LINE` before finishing the node
    // (`parser::tag::tag_line`), so replacing that wider range would eat
    // the newline and glue the next line onto this one.
    let range = tag.syntax().text_range();
    vec![Fix {
        code: DiagnosticCode::E110,
        title: "Rewrite to the `@[effects(…)]` annotation spelling".to_owned(),
        applicability: Applicability::Safe,
        edits: vec![FileEdit {
            file: d.file,
            range,
            new_text: format!("@[effects({new_args})]"),
        }],
        caret: None,
    }]
}

// ── Legacy colon-grammar parse (mirrors `parse_effects_clauses`) ────────────

/// The parsed shape of a `#@effects(…)` argument list — the same fields
/// [`brink_ir::EffectsAssertion`] carries, minus the source range this
/// fixer has no use for.
struct LegacyEffects {
    pure: bool,
    silent: bool,
    total: bool,
    reads: Vec<String>,
    writes: Vec<String>,
    calls: Vec<String>,
}

/// Which clause a bare (no `key:` prefix) value continues — mirrors
/// `brink_ir`'s private `EffectsClauseKind`.
#[derive(Clone, Copy)]
enum ClauseKind {
    Reads,
    Writes,
    Calls,
}

/// A plain identifier (letter/underscore start, alphanumeric/underscore
/// rest) — mirrors `brink_ir::hir::lower::directive::is_effects_ident`
/// exactly (the only legal shape for a clause value on either spelling).
fn is_effects_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn push_clause(
    kind: ClauseKind,
    value: &str,
    reads: &mut Vec<String>,
    writes: &mut Vec<String>,
    calls: &mut Vec<String>,
) -> bool {
    if !is_effects_ident(value) {
        return false;
    }
    match kind {
        ClauseKind::Reads => reads.push(value.to_owned()),
        ClauseKind::Writes => writes.push(value.to_owned()),
        ClauseKind::Calls => calls.push(value.to_owned()),
    }
    true
}

/// Parse a `#@effects(…)` argument list under the legacy colon grammar —
/// an independently-tested port of `brink_ir::hir::lower::directive::
/// parse_effects_clauses` (that function is private to `brink-ir`'s
/// lowering internals, so it cannot be called from here; see this module's
/// doc for why a port, not a diagnostics-emitting reuse, is what is needed,
/// and why that is safe). `None` on any malformed piece, a `pure`/clause
/// contradiction, or a wholly vacuous argument list — the same success
/// conditions the original enforces before it would hand back an
/// `EffectsAssertion`.
fn parse_legacy_effects(text: &str) -> Option<LegacyEffects> {
    let mut pure = false;
    let mut silent = false;
    let mut total = false;
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    let mut calls = Vec::new();
    let mut current: Option<ClauseKind> = None;
    let mut ok = true;

    for piece in text.split(',') {
        let piece = piece.trim();
        if piece.is_empty() {
            // Tolerate a stray/trailing comma — matches the original.
            continue;
        }
        if let Some((key, rest)) = piece.split_once(':') {
            let key = key.trim();
            let kind = match key {
                "reads" => ClauseKind::Reads,
                "writes" => ClauseKind::Writes,
                "calls" => ClauseKind::Calls,
                _ => {
                    ok = false;
                    continue;
                }
            };
            current = Some(kind);
            let rest = rest.trim();
            if !rest.is_empty() && !push_clause(kind, rest, &mut reads, &mut writes, &mut calls) {
                ok = false;
            }
        } else {
            // A bare piece is a flag only while no clause is open — after a
            // `reads:`/`writes:`/`calls:` opener, a bare piece continues
            // that clause instead (the documented legacy footgun: `reads:
            // gold, silent` reads `silent` as a *value*, not the flag).
            if current.is_none() {
                match piece {
                    "pure" => {
                        pure = true;
                        continue;
                    }
                    "silent" => {
                        silent = true;
                        continue;
                    }
                    "total" => {
                        total = true;
                        continue;
                    }
                    _ => {}
                }
            }
            let Some(kind) = current else {
                ok = false;
                continue;
            };
            if !push_clause(kind, piece, &mut reads, &mut writes, &mut calls) {
                ok = false;
            }
        }
    }

    // `pure` asserts the EMPTY state row — combining it with a clause that
    // grants state atoms is contradictory, not a union.
    if pure && !(reads.is_empty() && writes.is_empty() && calls.is_empty()) {
        ok = false;
    }
    if !ok {
        return None;
    }
    if !pure && !silent && !total && reads.is_empty() && writes.is_empty() && calls.is_empty() {
        return None;
    }
    Some(LegacyEffects {
        pure,
        silent,
        total,
        reads,
        writes,
        calls,
    })
}

/// Render a parsed assertion as the `@[effects(…)]` paren-clause argument
/// text. Flags first (in a fixed `pure, silent, total` order), then
/// `reads(…)`/`writes(…)`/`calls(…)` clauses in that order, each preserving
/// its collected values' order — order carries no semantic weight either
/// way (`EffectsAssertion`'s lists join a set-like state), but a fixed
/// order keeps the output deterministic.
fn render_annotation_args(e: &LegacyEffects) -> String {
    let mut parts = Vec::new();
    if e.pure {
        parts.push("pure".to_owned());
    }
    if e.silent {
        parts.push("silent".to_owned());
    }
    if e.total {
        parts.push("total".to_owned());
    }
    if !e.reads.is_empty() {
        parts.push(format!("reads({})", e.reads.join(", ")));
    }
    if !e.writes.is_empty() {
        parts.push(format!("writes({})", e.writes.join(", ")));
    }
    if !e.calls.is_empty() {
        parts.push(format!("calls({})", e.calls.join(", ")));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fix::fixes_at;
    use crate::session::IdeSession;

    fn session_with(path: &str, src: &str) -> IdeSession {
        let mut session = IdeSession::new();
        session.set_language_dialect(brink_analyzer::Dialect::Brink);
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

    // ── legacy-grammar port: unit coverage independent of the fixer ──────

    #[test]
    fn legacy_parse_reads_clause_with_continuation() {
        let parsed = parse_legacy_effects("reads: gold, hp").expect("parses");
        assert_eq!(parsed.reads, vec!["gold", "hp"]);
        assert!(parsed.writes.is_empty());
        assert!(!parsed.pure);
    }

    #[test]
    fn legacy_parse_documented_footgun_silent_continues_open_clause() {
        // The documented legacy footgun: after `reads:` opens the clause,
        // a bare `silent` continues it as a *value*, not the global flag.
        let parsed = parse_legacy_effects("reads: gold, silent").expect("parses");
        assert_eq!(parsed.reads, vec!["gold", "silent"]);
        assert!(!parsed.silent);
    }

    #[test]
    fn legacy_parse_pure_flag() {
        let parsed = parse_legacy_effects("pure").expect("parses");
        assert!(parsed.pure);
        assert!(parsed.reads.is_empty());
    }

    #[test]
    fn legacy_parse_rejects_pure_with_a_clause() {
        assert!(parse_legacy_effects("pure, reads: gold").is_none());
    }

    #[test]
    fn legacy_parse_rejects_unknown_clause_key() {
        assert!(parse_legacy_effects("bogus: mood").is_none());
    }

    #[test]
    fn legacy_parse_rejects_empty_argument_list() {
        assert!(parse_legacy_effects("").is_none());
        assert!(parse_legacy_effects("  ,  ").is_none());
    }

    #[test]
    fn render_orders_flags_then_clauses() {
        let e = LegacyEffects {
            pure: false,
            silent: true,
            total: true,
            reads: vec!["gold".to_owned()],
            writes: vec!["mood".to_owned()],
            calls: vec!["Alarm".to_owned()],
        };
        assert_eq!(
            render_annotation_args(&e),
            "silent, total, reads(gold), writes(mood), calls(Alarm)"
        );
    }

    // ── the fixer, on a live session ──────────────────────────────────────

    #[test]
    fn e110_ink_rewrites_reads_clause_to_paren_annotation() {
        let src = "VAR mood = 5\n\n-> greet\n\n=== greet ===\n#@effects(reads: mood)\nMood is {mood}.\n-> END\n";
        let session = session_with("test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("#@effects").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(
            fixes.len(),
            1,
            "{:?}",
            fixes.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert_eq!(fixes[0].code, DiagnosticCode::E110);
        assert_eq!(fixes[0].applicability, Applicability::Safe);
        let patched = applied(src, &fixes[0]);
        assert_eq!(
            patched,
            "VAR mood = 5\n\n-> greet\n\n=== greet ===\n@[effects(reads(mood))]\nMood is {mood}.\n-> END\n"
        );
    }

    #[test]
    fn e110_reanalysis_clears_the_diagnostic() {
        let src = "VAR mood = 5\n\n-> greet\n\n=== greet ===\n#@effects(reads: mood)\nMood is {mood}.\n-> END\n";
        let session = session_with("test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("#@effects").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(fixes.len(), 1);
        let patched = applied(src, &fixes[0]);

        let after = session_with("test.ink", &patched);
        let after_file = after.file_id("test.ink").expect("file id");
        let diags = after.db().diagnostics(after_file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E110),
            "{diags:?}"
        );
    }

    /// The analyzer's own inferred effect row for `greet` is byte-identical
    /// before and after — the actual proof this fix preserves the
    /// definition's effects, computed by re-running the real production
    /// parsers (both grammars) rather than trusting [`parse_legacy_effects`]
    /// in isolation. `EffectRowView` is used (not the raw `EffectRow`)
    /// because it name-resolves atoms and is independent of
    /// `DefinitionId`-allocation order across the two separate compilations
    /// (its own doc: "exactly what `effects-diff` needs to compare two
    /// builds without spurious churn").
    #[test]
    fn e110_fix_preserves_the_analyzers_inferred_effect_row() {
        let src = "VAR mood = 5\n\n-> greet\n\n=== greet ===\n#@effects(reads: mood)\nMood is {mood}.\n-> END\n";
        let before_session = session_with("test.ink", src);
        let before_file = before_session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("#@effects").expect("cursor site")).expect("fits");
        let cx = FixCx::new(before_session.db());
        let fixes = fixes_at(&cx, before_file, off);
        assert_eq!(fixes.len(), 1);
        let patched = applied(src, &fixes[0]);
        assert_ne!(patched, src, "the fix must actually change the source");

        let after_session = session_with("test.ink", &patched);
        let after_file = after_session.file_id("test.ink").expect("file id");

        let before_index = before_session.db().symbol_index();
        let before_def = before_index
            .symbols
            .values()
            .find(|s| s.name == "greet" && s.kind == brink_ir::SymbolKind::Knot)
            .expect("greet is a known knot")
            .id;
        let before_row = before_session
            .db()
            .effects(before_def)
            .expect("greet has an inferred effect row before the fix");
        let before_view = crate::effects::EffectRowView::from_row(&before_row, &before_index);

        let after_index = after_session.db().symbol_index();
        let after_def = after_index
            .symbols
            .values()
            .find(|s| s.name == "greet" && s.kind == brink_ir::SymbolKind::Knot)
            .expect("greet is still a known knot")
            .id;
        let after_row = after_session
            .db()
            .effects(after_def)
            .expect("greet has an inferred effect row after the fix");
        let after_view = crate::effects::EffectRowView::from_row(&after_row, &after_index);

        assert_eq!(
            before_view, after_view,
            "the fix changed greet's inferred effect row"
        );
        assert!(
            after_index
                .symbols
                .get(&after_def)
                .is_some_and(|s| s.file == after_file),
            "sanity: the resolved def is in the file just analyzed"
        );
    }

    #[test]
    fn e110_ink_stitch_leading_run_rewrites_too() {
        // `E176`'s own sibling coverage rule: check the structurally
        // parallel shape (stitch, not just knot) rather than assuming it
        // behaves the same — `effects_assertion_from_directives` is called
        // separately from `hir::lower::structure::stitch`.
        let src = "-> greet.hello\n\n=== greet ===\n= hello\n#@effects(pure)\nHi!\n-> END\n";
        let session = session_with("test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("#@effects").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(
            fixes.len(),
            1,
            "{:?}",
            fixes.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        let patched = applied(src, &fixes[0]);
        assert_eq!(
            patched,
            "-> greet.hello\n\n=== greet ===\n= hello\n@[effects(pure)]\nHi!\n-> END\n"
        );

        let after = session_with("test.ink", &patched);
        let after_file = after.file_id("test.ink").expect("file id");
        let diags = after.db().diagnostics(after_file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E110),
            "{diags:?}"
        );
    }

    // ── narrowing: shapes this fixer must refuse ─────────────────────────

    #[test]
    fn e110_no_fix_for_a_malformed_clause() {
        // `bogus` is not a recognized clause key — `E101` fires alongside
        // `E110`, and this fixer must not guess at a rewrite for text it
        // cannot parse.
        let src = "-> greet\n\n=== greet ===\n#@effects(bogus: mood)\nHi!\n-> END\n".to_owned();
        let session = session_with("test.ink", &src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("#@effects").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
    }

    #[test]
    fn e110_no_fix_for_a_dynamic_tag() {
        // Directives must be static; a `{…}` inline-logic child makes this
        // tag dynamic (`E046` also fires), and a text-based rewrite would
        // silently drop the interpolated part rather than translate it.
        let src =
            "VAR mood = 5\n-> greet\n\n=== greet ===\n#@effects(reads: {mood})\nHi!\n-> END\n"
                .to_owned();
        let session = session_with("test.ink", &src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("#@effects").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
    }

    #[test]
    fn e110_no_fix_for_a_bare_tag_with_no_arguments() {
        // Bare `#@effects` has no argument list — `E100` fires, and there
        // is nothing here to translate.
        let src = "-> greet\n\n=== greet ===\n#@effects\nHi!\n-> END\n".to_owned();
        let session = session_with("test.ink", &src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("#@effects").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
    }
}
