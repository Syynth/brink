//! Quick-fix for the T1c-2/T1c-3 `call(f, args…)`/`bind(f, args…)` strict
//! over-arity diagnostics (issue #744, #733's `E063` over-supply cases;
//! `docs/t1c-spec.md` §3/§4).
//!
//! `brink_analyzer::strict::check_value_calls` reports `E063` for several
//! distinct [`ValueCallFact`] shapes (not-callable, arity mismatch, arg-type
//! mismatch, over-bind) sharing one code — never distinguishable by parsing
//! the diagnostic *message* (see [`crate::import_fix`]'s own doc: "never by
//! parsing the diagnostic message"). This module instead reads the same
//! structural facts the checker itself produces, off
//! [`brink_db::ProjectDb::infer_body`] (`BodyTypes::value_calls`) — the
//! ratified per-def seam (TM-5), not a re-run of the analyzer's own pass —
//! and offers a fix only for the two shapes with an unambiguous mechanical
//! rewrite: `got` argument(s) supplied where the callee's known type takes
//! fewer (`ArityMismatch`) or has fewer remaining (`OverBind`). "Remove
//! extra argument(s)" trims the call site's trailing args back to the
//! known-good count — the excess args were simply discarded at runtime and
//! the kept prefix was already accepted by the same per-position check the
//! diagnostic itself ran. That still deletes author-written text, so the
//! fixer declares `Suggested`, not `Safe` (`docs/autofix-spec.md` §3 —
//! `Safe` is what the observable-semantics oracle certifies, #3371).
//!
//! `NotCallable`/`UnknownCallee`/`ConflictedCallee`/`ArgMismatch` have no
//! offered fix: none of them has a mechanical rewrite (the callee's actual
//! type, or the right value to pass, cannot be inferred from the call site
//! alone).
//!
//! Ink-frontend only for the same reason as [`crate::creation_site_fix`]:
//! this module parses with `brink_syntax`'s CST directly to locate and trim
//! the call site's `ArgList`, so a native `.brink` file (a different
//! frontend/AST entirely) is skipped via
//! [`brink_db::ProjectDb::is_native`]. `call`/`bind` are reachable from a
//! native file too (`is_t1b_stdlib_name` is a HIR-level, dialect-generic
//! check), so this is a real, tracked scope cut — see the issue #744 PR's
//! scope note — not a claim that native sites never fire this diagnostic.

use brink_analyzer::ValueCallKind;
use brink_db::ProjectDb;
use brink_format::DefinitionId;
use brink_ir::{Diagnostic, DiagnosticCode, FileId, SymbolKind};
use brink_syntax::ast::{AstNode as _, FunctionCall, SourceFile};
use rowan::{TextRange, TextSize};

use crate::fix::{Applicability, Fix, FixCx, Fixer};
use crate::rename::FileEdit;

/// The `E063` over-supply fixer for `call(f, args…)`/`bind(f, args…)`: trim
/// the call's trailing arguments back to what the callee's known type
/// accepts.
///
/// Offers nothing unless the site's recorded [`ValueCallFact`] is an
/// over-supply shape (`ArityMismatch`/`OverBind` with `got` exceeding what
/// the callee accepts) — `E063`'s other shapes have no mechanical rewrite.
///
/// [`ValueCallFact`]: brink_analyzer::ValueCallFact
pub struct ValueCallArityFixer;

impl Fixer for ValueCallArityFixer {
    fn code(&self) -> DiagnosticCode {
        DiagnosticCode::E063
    }

    /// The discarded arguments were already being ignored at runtime, but
    /// deleting author-written text is a §3 `Suggested` change until the
    /// observable-semantics oracle (#3371) can certify it.
    fn max_applicability(&self) -> Applicability {
        Applicability::Suggested
    }

    fn fixes(&self, cx: &FixCx<'_>, d: &Diagnostic) -> Vec<Fix> {
        let db = cx.db;
        let file_id = d.file;
        if db.is_native(file_id) {
            // See module doc: native `.brink` call()/bind() sites are a
            // tracked follow-up, not covered by this CST-level fix.
            return Vec::new();
        }
        let Some(source) = db.source(file_id) else {
            return Vec::new();
        };
        let at = d.range.start();

        let parse = brink_syntax::parse(source);
        let tree = parse.tree();
        let root = tree.syntax().clone();

        let Some(fc) = root
            .descendants()
            .filter_map(FunctionCall::cast)
            .filter(|fc| matches!(fc.name().as_deref(), Some("call" | "bind")))
            .filter(|fc| fc.syntax().text_range().contains_inclusive(at))
            .min_by_key(|fc| fc.syntax().text_range().len())
        else {
            return Vec::new();
        };
        let Some(verb) = fc.name() else {
            return Vec::new();
        };
        let Some(ident_range) = fc.identifier().map(|id| id.syntax().text_range()) else {
            return Vec::new();
        };

        let Some(def) = enclosing_def_at(db, file_id, &tree, at) else {
            return Vec::new();
        };
        let Some(body) = db.infer_body(def) else {
            return Vec::new();
        };
        let Some(fact) = body.value_calls.iter().find(|f| f.range == ident_range) else {
            return Vec::new();
        };

        let keep = match &fact.kind {
            ValueCallKind::ArityMismatch { expected, got } if got > expected => *expected,
            ValueCallKind::OverBind { available, got } if got > available => *available,
            _ => return Vec::new(),
        };

        let Some(range) = trailing_args_range(&fc, keep) else {
            return Vec::new();
        };

        vec![Fix {
            code: DiagnosticCode::E063,
            title: format!(
                "Remove extra argument(s) — `{verb}` accepts {keep} argument(s) after the callee here"
            ),
            applicability: Applicability::Suggested,
            edits: vec![FileEdit {
                file: file_id,
                range,
                new_text: String::new(),
            }],
            caret: None,
        }]
    }
}

/// The byte range covering every argument after the callee plus the first
/// `keep` — the span the trim deletes.
///
/// `keep` is the count of args *after* the callee (matching `ValueCallFact`'s
/// own `args[1..]` convention); the callee itself (`args[0]`) always stays.
///
/// `None` when the site is already at or under the kept count, or when the
/// parser never consumed a `)`. `ARG_LIST` never includes the surrounding
/// parens (`divert::arg_list` starts after `(` and stops before `)`), and the
/// `FUNCTION_CALL` node's own last byte is only `)` when the parser actually
/// found and consumed one — re-locate the real closing `)` instead of
/// assuming it, since parse-error recovery (an unterminated call) can leave
/// the node closed without one. See [`crate::text::closing_paren_offset`].
fn trailing_args_range(fc: &FunctionCall, keep: usize) -> Option<TextRange> {
    let arg_list = fc.arg_list()?;
    let args: Vec<_> = arg_list.args().collect();
    let total_keep = keep.checked_add(1)?;
    if args.len() <= total_keep {
        return None;
    }
    let last_kept_end = args.get(total_keep - 1)?.syntax().text_range().end();
    let close_paren = crate::text::closing_paren_offset(fc.syntax())?;
    Some(TextRange::new(
        last_kept_end,
        TextSize::from(u32::try_from(close_paren).unwrap_or(u32::MAX)),
    ))
}

/// The enclosing knot/stitch [`DefinitionId`] for source position `at`,
/// found structurally from the CST (mirrors
/// `crate::inferred_types::enclosing_callable`, which instead starts from a
/// resolved [`brink_ir::SymbolInfo`]'s `Scope` — unavailable here, since a
/// `call`/`bind` callee is an arbitrary expression, not a resolvable
/// symbol). Deterministic even under a duplicate-declaration diagnostic:
/// picks the lowest id, never `HashMap` iteration order.
fn enclosing_def_at(
    db: &ProjectDb,
    file_id: FileId,
    tree: &SourceFile,
    at: TextSize,
) -> Option<DefinitionId> {
    let mut knot_name = None;
    let mut stitch_name = None;
    for knot in tree.knots() {
        if !knot.syntax().text_range().contains_inclusive(at) {
            continue;
        }
        knot_name = knot.header().and_then(|h| h.name());
        if let Some(body) = knot.body() {
            for stitch in body.stitches() {
                if stitch.syntax().text_range().contains_inclusive(at) {
                    stitch_name = stitch.header().and_then(|h| h.name());
                    break;
                }
            }
        }
        break;
    }
    let knot_name = knot_name?;
    let qualified = match stitch_name {
        Some(stitch) => format!("{knot_name}.{stitch}"),
        None => knot_name,
    };
    let index = db.symbol_index();
    index
        .by_name
        .get(&qualified)?
        .iter()
        .copied()
        .filter(|id| {
            index.symbols.get(id).is_some_and(|sym| {
                sym.file == file_id && matches!(sym.kind, SymbolKind::Knot | SymbolKind::Stitch)
            })
        })
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fix::fixes_at;
    use crate::session::IdeSession;

    fn strict_session_with(src: &str) -> IdeSession {
        let mut session = IdeSession::new();
        session.set_language_dialect(brink_analyzer::Dialect::Brink);
        session.set_type_policy(brink_analyzer::TypePolicy::Strict);
        session.update_source("test.ink", src.to_string());
        session.update_and_analyze("test.ink", src.to_string());
        session
    }

    /// Apply a fix's edits to `src`. The logic under test is the fixer's;
    /// this only splices.
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

    const HEAL: &str = "=== function heal(hp, amount) ===\n~ return hp + amount\n\n";

    #[test]
    fn offers_trim_for_call_over_arity() {
        let src = format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal)\n~ temp r = call(f, 1, 2, 3)\n-> DONE\n"
        );
        let session = strict_session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("(f, 1, 2, 3)").expect("cursor site") - 4).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        let titles: Vec<&String> = fixes.iter().map(|f| &f.title).collect();
        assert_eq!(fixes.len(), 1, "{titles:?}");
        assert_eq!(fixes[0].code, DiagnosticCode::E063);
        assert_eq!(fixes[0].applicability, Applicability::Suggested);
        assert_eq!(
            fixes[0].title,
            "Remove extra argument(s) — `call` accepts 2 argument(s) after the callee here"
        );
        assert_eq!(
            applied(&src, &fixes[0]),
            format!(
                "{HEAL}=== main ===\n~ temp f = #fn(heal)\n~ temp r = call(f, 1, 2)\n-> DONE\n"
            )
        );
    }

    #[test]
    fn trim_edit_reanalysis_clears_e063() {
        let src = format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal)\n~ temp r = call(f, 1, 2, 3)\n-> DONE\n"
        );
        let session = strict_session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("(f, 1, 2, 3)").expect("cursor site") - 4).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(fixes.len(), 1);
        let patched = applied(&src, &fixes[0]);

        let after = strict_session_with(&patched);
        let file = after.file_id("test.ink").expect("file id");
        let diags = after.db().diagnostics(file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E063),
            "{diags:?}"
        );
    }

    #[test]
    fn no_edit_when_closing_paren_is_missing() {
        // Unterminated `call(...)` — same parse-error-recovery hazard as
        // `creation_site_fix`: the FUNCTION_CALL node never gets an
        // `R_PAREN` token, so assuming `text_range().end() - 1` is `)` would
        // silently fuse "3" with whatever follows.
        let src = format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal)\n~ temp r = call(f, 1, 2, 3\n-> DONE\n"
        );
        let session = strict_session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session.db().diagnostics(file).expect("diagnostics");
        let e063 = diags.iter().find(|d| d.code == DiagnosticCode::E063);
        assert!(
            e063.is_some(),
            "fixture must still carry an E063 to fix: {diags:?}"
        );
        let cx = FixCx::new(session.db());
        assert!(
            ValueCallArityFixer
                .fixes(&cx, e063.expect("just asserted above"))
                .is_empty()
        );
    }

    #[test]
    fn offers_trim_for_bind_over_binding() {
        let src = format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal)\n~ temp g = bind(f, 1, 2, 3)\n-> DONE\n"
        );
        let session = strict_session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("(f, 1, 2, 3)").expect("cursor site") - 4).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        let titles: Vec<&String> = fixes.iter().map(|f| &f.title).collect();
        assert_eq!(fixes.len(), 1, "{titles:?}");
        assert!(fixes[0].title.contains("`bind`"), "{}", fixes[0].title);
        assert_eq!(
            applied(&src, &fixes[0]),
            format!(
                "{HEAL}=== main ===\n~ temp f = #fn(heal)\n~ temp g = bind(f, 1, 2)\n-> DONE\n"
            )
        );
    }

    #[test]
    fn no_offer_under_gradual_types() {
        // `value_calls` is recorded unconditionally but only *reported* by
        // strict mode — no diagnostic under gradual, so no fix.
        let src = format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal)\n~ temp r = call(f, 1, 2, 3)\n-> DONE\n"
        );
        let mut session = IdeSession::new();
        session.set_language_dialect(brink_analyzer::Dialect::Brink);
        // `types` defaults to `Strict` under `dialect = brink` (issue #1127
        // ruling) — force `Gradual` explicitly so this test actually
        // exercises the "no diagnostic under gradual" case it's named for.
        session.set_type_policy(brink_analyzer::TypePolicy::Gradual);
        session.update_source("test.ink", src.clone());
        session.update_and_analyze("test.ink", src.clone());
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("(f, 1, 2, 3)").expect("cursor site") - 4).expect("fits");
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
    }

    #[test]
    fn no_offer_when_arity_matches() {
        let src = format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal)\n~ temp r = call(f, 1, 2)\n-> DONE\n"
        );
        let session = strict_session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("(f, 1, 2)").expect("cursor site") - 4).expect("fits");
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
    }
}
