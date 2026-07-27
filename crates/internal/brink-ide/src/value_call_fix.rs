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
//! known-good count — always safe, since the excess args are simply
//! discarded and the kept prefix was already accepted by the same
//! per-position check the diagnostic itself ran.
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
use brink_ir::{FileId, SymbolKind};
use brink_syntax::ast::{AstNode as _, FunctionCall, SourceFile};
use rowan::{TextRange, TextSize};

use crate::code_actions::{CodeAction, CodeActionData, CodeActionKind};

/// Collect "remove extra argument(s)" quick-fixes for a `call(...)`/
/// `bind(...)` over-arity site at `offset` in `file_id`. Empty unless the
/// cursor sits inside such a call and its recorded [`ValueCallFact`] is an
/// over-supply shape (`ArityMismatch`/`OverBind` with `got` exceeding what
/// the callee's known type accepts).
///
/// [`ValueCallFact`]: brink_analyzer::ValueCallFact
#[must_use]
pub fn value_call_actions(db: &ProjectDb, file_id: FileId, offset: u32) -> Vec<CodeAction> {
    if db.is_native(file_id) {
        // See module doc: native `.brink` call()/bind() sites are a tracked
        // follow-up, not covered by this CST-level fix.
        return Vec::new();
    }
    let Some(source) = db.source(file_id) else {
        return Vec::new();
    };
    let at = TextSize::from(offset);

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

    // Only offer the fix when the analyzer actually reported it as a
    // diagnostic (strict mode) — under gradual types `value_calls` is
    // recorded but never surfaced, and there is nothing to "fix" if the
    // author sees no error.
    let node_range = fc.syntax().text_range();
    let has_e063 = db.diagnostics(file_id).is_some_and(|diags| {
        diags
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E063 && node_range.contains_range(d.range))
    });
    if !has_e063 {
        return Vec::new();
    }

    let Some(occurrence) = function_call_occurrence(&root, &verb, node_range) else {
        return Vec::new();
    };

    vec![CodeAction {
        title: format!(
            "Remove extra argument(s) — `{verb}` accepts {keep} argument(s) after the callee here"
        ),
        kind: CodeActionKind::QuickFix,
        data: CodeActionData::TrimValueCallArgs {
            verb,
            occurrence,
            keep,
        },
    }]
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

/// The 0-based index of the `verb(...)` call at `node_range` among every
/// `FunctionCall` in the file named `verb`, in source order — the
/// disambiguating key [`CodeActionData::TrimValueCallArgs`] carries instead
/// of a byte range (same convention as
/// `crate::creation_site_fix::fn_literal_occurrence`).
fn function_call_occurrence(
    root: &brink_syntax::SyntaxNode,
    verb: &str,
    node_range: TextRange,
) -> Option<usize> {
    root.descendants()
        .filter_map(FunctionCall::cast)
        .filter(|fc| fc.name().as_deref() == Some(verb))
        .position(|fc| fc.syntax().text_range() == node_range)
}

/// Resolve a [`CodeActionData::TrimValueCallArgs`] action: a pure source
/// rewrite, re-locating the `occurrence`-th `verb(...)` call fresh from
/// `source`.
#[must_use]
pub fn resolve_value_call_action(source: &str, data: &CodeActionData) -> Option<String> {
    let CodeActionData::TrimValueCallArgs {
        verb,
        occurrence,
        keep,
    } = data
    else {
        return None;
    };

    let parse = brink_syntax::parse(source);
    let root = parse.tree().syntax().clone();
    let fc = root
        .descendants()
        .filter_map(FunctionCall::cast)
        .filter(|fc| fc.name().as_deref() == Some(verb.as_str()))
        .nth(*occurrence)?;
    let arg_list = fc.arg_list()?;
    let args: Vec<_> = arg_list.args().collect();
    // `keep` is the count of args *after* the callee (matching
    // `ValueCallFact`'s own `args[1..]` convention); the callee itself
    // (`args[0]`) always stays.
    let total_keep = keep.checked_add(1)?;
    if args.len() <= total_keep {
        // Already at or under the kept count — nothing to trim (stale
        // offer).
        return None;
    }

    let last_kept_end: usize = args[total_keep - 1].syntax().text_range().end().into();
    // `ARG_LIST` never includes the surrounding parens (`divert::arg_list`
    // starts after `(` and stops before `)`), and the `FUNCTION_CALL`
    // node's own last byte is only `)` when the parser actually found and
    // consumed one — re-locate the real closing `)` token instead of
    // assuming it, since parse-error recovery (an unterminated call) can
    // leave the node closed without one. See
    // `crate::text::closing_paren_offset`.
    let close_paren = crate::text::closing_paren_offset(fc.syntax())?;

    let mut out = String::with_capacity(source.len());
    out.push_str(source.get(..last_kept_end)?);
    out.push_str(source.get(close_paren..)?);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::IdeSession;

    fn strict_session_with(src: &str) -> IdeSession {
        let mut session = IdeSession::new();
        session.set_language_dialect(brink_analyzer::Dialect::Brink);
        session.set_type_policy(brink_analyzer::TypePolicy::Strict);
        session.update_source("test.ink", src.to_string());
        session.update_and_analyze("test.ink", src.to_string());
        session
    }

    const HEAL: &str = "=== function heal(hp, amount) ===\n~ return hp + amount\n\n";

    #[test]
    fn offers_trim_for_call_over_arity() {
        let src = format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal)\n~ temp r = call(f, 1, 2, 3)\n-> DONE\n"
        );
        let session = strict_session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("3)").expect("cursor site")).expect("fits");
        let actions = value_call_actions(session.db(), file, off);
        let titles: Vec<&String> = actions.iter().map(|a| &a.title).collect();
        assert_eq!(actions.len(), 1, "{titles:?}");
        assert!(actions[0].title.contains("Remove extra argument"));
        assert!(
            matches!(
                &actions[0].data,
                CodeActionData::TrimValueCallArgs { verb, occurrence: 0, keep: 2 }
                    if verb == "call"
            ),
            "{:?}",
            actions[0].data
        );
    }

    #[test]
    fn trim_resolves_and_reanalysis_clears_e063() {
        let src = format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal)\n~ temp r = call(f, 1, 2, 3)\n-> DONE\n"
        );
        let fixed = resolve_value_call_action(
            &src,
            &CodeActionData::TrimValueCallArgs {
                verb: "call".to_owned(),
                occurrence: 0,
                keep: 2,
            },
        )
        .expect("resolves");
        assert_eq!(
            fixed,
            format!(
                "{HEAL}=== main ===\n~ temp f = #fn(heal)\n~ temp r = call(f, 1, 2)\n-> DONE\n"
            )
        );

        let mut session = IdeSession::new();
        session.set_language_dialect(brink_analyzer::Dialect::Brink);
        session.set_type_policy(brink_analyzer::TypePolicy::Strict);
        session.update_and_analyze("test.ink", fixed);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session.db().diagnostics(file).expect("diagnostics");
        assert!(
            diags
                .iter()
                .all(|d| d.code != brink_ir::DiagnosticCode::E063),
            "{diags:?}"
        );
    }

    #[test]
    fn resolve_returns_none_when_closing_paren_is_missing() {
        // Unterminated `call(...)` — same parse-error-recovery hazard as
        // `creation_site_fix::trim_fn_literal_args`: the FUNCTION_CALL node
        // never gets an `R_PAREN` token, so assuming `text_range().end() -
        // 1` is `)` would silently fuse "3" with whatever follows.
        let src = format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal)\n~ temp r = call(f, 1, 2, 3\n-> DONE\n"
        );
        let fixed = resolve_value_call_action(
            &src,
            &CodeActionData::TrimValueCallArgs {
                verb: "call".to_owned(),
                occurrence: 0,
                keep: 2,
            },
        );
        assert_eq!(fixed, None);
    }

    #[test]
    fn offers_trim_for_bind_over_binding() {
        let src = format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal)\n~ temp g = bind(f, 1, 2, 3)\n-> DONE\n"
        );
        let session = strict_session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("3)").expect("cursor site")).expect("fits");
        let actions = value_call_actions(session.db(), file, off);
        let titles: Vec<&String> = actions.iter().map(|a| &a.title).collect();
        assert_eq!(actions.len(), 1, "{titles:?}");
        assert!(
            matches!(
                &actions[0].data,
                CodeActionData::TrimValueCallArgs { verb, occurrence: 0, keep: 2 }
                    if verb == "bind"
            ),
            "{:?}",
            actions[0].data
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
        let off = u32::try_from(src.find("3)").expect("cursor site")).expect("fits");
        assert!(value_call_actions(session.db(), file, off).is_empty());
    }

    #[test]
    fn no_offer_when_arity_matches() {
        let src = format!(
            "{HEAL}=== main ===\n~ temp f = #fn(heal)\n~ temp r = call(f, 1, 2)\n-> DONE\n"
        );
        let session = strict_session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("2)").expect("cursor site")).expect("fits");
        assert!(value_call_actions(session.db(), file, off).is_empty());
    }
}
