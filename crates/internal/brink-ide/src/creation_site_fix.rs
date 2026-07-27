//! Quick-fixes for the T1c `#fn(target, args…)` creation-site diagnostics
//! (issue #744; the checks themselves live in
//! `brink_analyzer::fn_values`, docs/t1c-spec.md §2):
//!
//! - **E081** (over-binding): the bound args are longer than the target's
//!   declared param row — [`fn_value_actions`] offers "remove extra
//!   argument(s)", trimming the creation site back to the declared prefix.
//! - **E080** (unbound `ref` param): a `ref` param has no bound argument at
//!   all (`fl.args.len() <= param index`) — offered only when *every*
//!   declared param from `args.len()` through the target's *last* `ref`
//!   param (inclusive) is itself `ref`, each with an unambiguous same-named
//!   durable global `VAR` in scope. That span is exactly what must gain a
//!   real value to keep the bound args a contiguous prefix and clear every
//!   unbound `ref` param in one shot — a trailing `val` param *after* the
//!   last `ref` param needs no value (val params never require binding), so
//!   it is correctly left out of the span, but a `val` param *inside* the
//!   span has no safe value to synthesize, so the whole fix is skipped in
//!   that case — under-fixing (staying silent) rather than guessing a value
//!   the author never wrote. The fix is also withheld outright when an
//!   *already-bound* `ref` argument itself carries an E080 (bound to a
//!   temp/param/CONST/field-projection instead of a durable `VAR`) — see
//!   [`has_e080_on_bound_arg`] — since appending the missing args would
//!   still leave that pre-existing E080 behind, breaking the "always leaves
//!   the call fully bound" guarantee.
//!
//! **E079** (target is not a function definition) has no offered fix here:
//! there is no single mechanical rewrite that recovers the author's intent
//! (the fix ranges from "declare the target as a function" to "the caller
//! meant a different name entirely"), so it is left as a diagnostic only.
//!
//! Both fixes gate on the analyzer's own diagnostic being present at the
//! cursor (`brink_db::ProjectDb::diagnostics`) before doing any of their own
//! structural work — same posture as [`crate::import_fix`]'s E025 gate: the
//! diagnostic pass, not this module, owns the rule.
//!
//! `#fn(...)` is ink-frontend-only syntax (there is no native-dialect
//! spelling — `brink_ir::hir::lower_native` never lowers a `FnLiteral`), so
//! this module parses with `brink_syntax` unconditionally and skips native
//! files via [`brink_db::ProjectDb::is_native`], mirroring the dialect
//! branch [`crate::import_fix::insert_import`] takes for the same reason.
//!
//! Session-aware structural facts (the target's declared param row, from
//! [`brink_db::ProjectDb::symbol_index`]/`resolve`) are captured into the
//! [`CodeActionData`] payload at offer time; resolution
//! ([`resolve_fn_value_action`]) is then a pure source rewrite keyed by the
//! target name + its occurrence index among same-named `#fn(...)` sites in
//! the file (never a stored byte range, which could go stale against an
//! intervening edit — the same convention
//! [`crate::code_actions::CodeActionData::SortStitches`]/`FormatStitch`
//! already follow, keyed by name rather than position).

use brink_db::ProjectDb;
use brink_ir::{DiagnosticCode, FileId, SymbolIndex, SymbolKind};
use brink_syntax::ast::{AstNode as _, FnLiteral};
use rowan::TextSize;

use crate::code_actions::{CodeAction, CodeActionData, CodeActionKind};

/// Collect `#fn(...)` creation-site quick-fixes applicable at `offset` in
/// `file_id`: "remove extra argument(s)" for E081, "bind ref argument(s)"
/// for E080. Empty when the cursor is not inside a `#fn(...)` literal
/// carrying one of these diagnostics.
#[must_use]
pub fn fn_value_actions(db: &ProjectDb, file_id: FileId, offset: u32) -> Vec<CodeAction> {
    if db.is_native(file_id) {
        // `#fn(...)` has no native-dialect spelling — see module doc.
        return Vec::new();
    }
    let Some(source) = db.source(file_id) else {
        return Vec::new();
    };
    let at = TextSize::from(offset);

    let parse = brink_syntax::parse(source);
    let root = parse.tree().syntax().clone();

    let Some(fl) = root
        .descendants()
        .filter_map(FnLiteral::cast)
        .filter(|fl| fl.syntax().text_range().contains_inclusive(at))
        .min_by_key(|fl| fl.syntax().text_range().len())
    else {
        return Vec::new();
    };

    // Gate on the analyzer's own diagnostic — it, not this function, owns
    // the creation-site rules (same posture as `import_fix::import_actions`
    // gating on E025). Both E081 and E080 anchor at the whole `#fn(...)`
    // literal's own range (`fn_values::FnValueVisitor::push` sites use
    // `fl.ptr.text_range()`), which is exactly what we just matched by
    // cursor containment.
    let has_creation_diag = db.diagnostics(file_id).is_some_and(|diags| {
        diags.iter().any(|d| {
            matches!(d.code, DiagnosticCode::E081 | DiagnosticCode::E080)
                && d.range.contains_inclusive(at)
        })
    });
    if !has_creation_diag {
        return Vec::new();
    }

    let Some(target_path) = fl.target() else {
        return Vec::new();
    };
    let target_name = target_path.full_name();

    let Some((resolutions, _)) = db.resolve(file_id) else {
        return Vec::new();
    };
    let target_range = target_path.syntax().text_range();
    let Some(res) = resolutions.iter().find(|r| r.range == target_range) else {
        return Vec::new();
    };
    let index = db.symbol_index();
    let Some(info) = index.symbols.get(&res.target) else {
        return Vec::new();
    };
    let is_function_def = matches!(info.kind, SymbolKind::Knot | SymbolKind::Stitch)
        && info.detail.as_deref() == Some("function");
    if !is_function_def {
        return Vec::new();
    }

    let args_len = fl.args().count();
    let node_range = fl.syntax().text_range();
    let Some(occurrence) = fn_literal_occurrence(&root, &target_name, node_range) else {
        return Vec::new();
    };

    let mut actions = Vec::new();

    // E081 — over-binding: trim back to the declared row.
    if args_len > info.params.len() {
        actions.push(CodeAction {
            title: format!(
                "Remove extra argument(s) — `{target_name}` declares {} parameter(s)",
                info.params.len()
            ),
            kind: CodeActionKind::QuickFix,
            data: CodeActionData::TrimFnLiteralArgs {
                target: target_name.clone(),
                occurrence,
                keep: info.params.len(),
            },
        });
    }

    // E080 — a `ref` param with no bound arg at all. Only offered when no
    // *already-bound* argument itself carries an E080 (see
    // `has_e080_on_bound_arg`'s doc) — otherwise the fix would add the
    // missing args and still leave the call not compiling.
    if !has_e080_on_bound_arg(db, file_id, &fl, args_len)
        && let Some(action) =
            bind_ref_args_action(&index, &info.params, args_len, &target_name, occurrence)
    {
        actions.push(action);
    }

    actions
}

/// Whether any *already-bound* argument (`fl`'s args at index `< args_len`)
/// itself carries an E080 diagnostic — e.g. a `ref` param bound to a
/// temp/param/CONST/field-projection instead of a durable `VAR`
/// (`fn_values::FnValueVisitor::check_ref_arg`, which anchors that
/// diagnostic at the argument's own range, not the whole `#fn(...)`
/// literal's).
///
/// [`fn_value_actions`]'s "bind ref argument(s)" fix only ever *appends*
/// args for the currently-*unbound* trailing `ref` params — it can never
/// clear a diagnostic on an argument that is already there. Offering it
/// anyway when one of those exists would leave the call still not
/// compiling after the "fix", contradicting this module's guarantee that
/// the fix always leaves the call fully bound (see module doc). Skipping
/// under-fixes rather than guessing here — same posture as the `val`-param-
/// inside-the-span case in [`bind_ref_args_action`]'s own doc.
fn has_e080_on_bound_arg(db: &ProjectDb, file_id: FileId, fl: &FnLiteral, args_len: usize) -> bool {
    let Some(diags) = db.diagnostics(file_id) else {
        return false;
    };
    fl.args().take(args_len).any(|arg| {
        let arg_range = arg.syntax().text_range();
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::E080 && arg_range.contains_range(d.range))
    })
}

/// The E080 "bind ref argument(s)" fix, split out of
/// [`fn_value_actions`] to keep that function under the line-count lint.
///
/// The span that needs filling is `[args_len, last_ref_idx]` — up to and
/// including the *last* declared `ref` param, since any `ref` param at or
/// after `args_len` is unbound and every position up to it must get a real
/// value to keep the bound args a contiguous prefix. A trailing `val` param
/// *after* the last `ref` param needs no value (val params never require
/// binding); a `val` param *inside* the span does, and there is no safe
/// value to synthesize for it, so no fix is offered in that case — see
/// module doc.
fn bind_ref_args_action(
    index: &SymbolIndex,
    params: &[brink_ir::ParamInfo],
    args_len: usize,
    target_name: &str,
    occurrence: usize,
) -> Option<CodeAction> {
    let last_ref_idx = params.iter().rposition(|p| p.is_ref)?;
    if last_ref_idx < args_len {
        return None;
    }
    let span = &params[args_len..=last_ref_idx];
    if !span.iter().all(|p| p.is_ref) {
        return None;
    }

    let mut vars = Vec::with_capacity(span.len());
    for p in span {
        vars.push(matching_global_var(index, &p.name)?);
    }
    if vars.is_empty() {
        return None;
    }

    let title = if vars.len() == 1 {
        format!("Bind `{}` as the ref argument for `{target_name}`", vars[0])
    } else {
        let joined = vars
            .iter()
            .map(|v| format!("`{v}`"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("Bind {joined} as the ref arguments for `{target_name}`")
    };
    Some(CodeAction {
        title,
        kind: CodeActionKind::QuickFix,
        data: CodeActionData::BindFnLiteralRefArgs {
            target: target_name.to_owned(),
            occurrence,
            vars,
        },
    })
}

/// An unambiguous durable global `VAR` (or `#@local` flow-local, same
/// `SymbolKind::Variable` at this layer — `fn_values`'s own E080 rule treats
/// them identically) named exactly `name`. `None` when no such symbol
/// exists or more than one does (an ambiguous project-wide name — fail safe
/// by not offering a fix rather than guessing which one the author meant).
fn matching_global_var(index: &SymbolIndex, name: &str) -> Option<String> {
    let ids = index.by_name.get(name)?;
    let mut vars = ids.iter().filter(|id| {
        index
            .symbols
            .get(id)
            .is_some_and(|s| s.kind == SymbolKind::Variable)
    });
    let first = vars.next()?;
    if vars.next().is_some() {
        return None;
    }
    index.symbols.get(first).map(|s| s.name.clone())
}

/// The 0-based index of the `#fn(...)` literal at `node_range` among every
/// `#fn(target, …)` site in the file naming the same `target`, in source
/// (document) order — the disambiguating key [`CodeActionData::
/// TrimFnLiteralArgs`]/`BindFnLiteralRefArgs` carry instead of a byte range.
fn fn_literal_occurrence(
    root: &brink_syntax::SyntaxNode,
    target: &str,
    node_range: rowan::TextRange,
) -> Option<usize> {
    root.descendants()
        .filter_map(FnLiteral::cast)
        .filter(|fl| fl.target().is_some_and(|t| t.full_name() == target))
        .position(|fl| fl.syntax().text_range() == node_range)
}

/// Resolve a [`CodeActionData::TrimFnLiteralArgs`]/`BindFnLiteralRefArgs`
/// action: a pure source rewrite, re-locating the `occurrence`-th
/// `#fn(target, …)` site fresh from `source` (never trusting a
/// previously-computed byte range — see module doc).
#[must_use]
pub fn resolve_fn_value_action(source: &str, data: &CodeActionData) -> Option<String> {
    match data {
        CodeActionData::TrimFnLiteralArgs {
            target,
            occurrence,
            keep,
        } => trim_fn_literal_args(source, target, *occurrence, *keep),
        CodeActionData::BindFnLiteralRefArgs {
            target,
            occurrence,
            vars,
        } => bind_fn_literal_ref_args(source, target, *occurrence, vars),
        _ => None,
    }
}

fn nth_fn_literal(
    root: &brink_syntax::SyntaxNode,
    target: &str,
    occurrence: usize,
) -> Option<FnLiteral> {
    root.descendants()
        .filter_map(FnLiteral::cast)
        .filter(|fl| fl.target().is_some_and(|t| t.full_name() == target))
        .nth(occurrence)
}

fn trim_fn_literal_args(
    source: &str,
    target: &str,
    occurrence: usize,
    keep: usize,
) -> Option<String> {
    let parse = brink_syntax::parse(source);
    let root = parse.tree().syntax().clone();
    let fl = nth_fn_literal(&root, target, occurrence)?;
    let target_path = fl.target()?;
    let args: Vec<_> = fl.args().collect();
    if args.len() <= keep {
        // Already at or under the kept count — nothing to trim (stale
        // offer, e.g. a previous fix already applied).
        return None;
    }

    let last_kept_end: usize = if keep == 0 {
        target_path.syntax().text_range().end().into()
    } else {
        args[keep - 1].syntax().text_range().end().into()
    };
    // Re-locate the real closing `)` token rather than assuming the node's
    // `text_range().end() - 1` is a `)` byte — that assumption breaks under
    // parse-error recovery (an unterminated `#fn(...)`), see
    // `crate::text::closing_paren_offset`.
    let close_paren = crate::text::closing_paren_offset(fl.syntax())?;

    let mut out = String::with_capacity(source.len());
    out.push_str(source.get(..last_kept_end)?);
    out.push_str(source.get(close_paren..)?);
    Some(out)
}

fn bind_fn_literal_ref_args(
    source: &str,
    target: &str,
    occurrence: usize,
    vars: &[String],
) -> Option<String> {
    if vars.is_empty() {
        return None;
    }
    let parse = brink_syntax::parse(source);
    let root = parse.tree().syntax().clone();
    let fl = nth_fn_literal(&root, target, occurrence)?;
    let target_path = fl.target()?;
    let args: Vec<_> = fl.args().collect();

    let insert_at: usize = match args.last() {
        Some(a) => a.syntax().text_range().end().into(),
        None => target_path.syntax().text_range().end().into(),
    };
    let insertion = format!(", {}", vars.join(", "));

    let mut out = String::with_capacity(source.len() + insertion.len());
    out.push_str(source.get(..insert_at)?);
    out.push_str(&insertion);
    out.push_str(source.get(insert_at..)?);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::IdeSession;

    fn session_with(src: &str) -> IdeSession {
        let mut session = IdeSession::new();
        // `#fn(...)` is brink-dialect-only syntax (`fn_values::check` "runs
        // only under dialect = brink"); under the default `StrictInk` it is
        // extension syntax rejected as `E051`, never reaching E080/E081.
        session.set_language_dialect(brink_analyzer::Dialect::Brink);
        session.update_source("test.ink", src.to_string());
        session.update_and_analyze("test.ink", src.to_string());
        session
    }

    const HEAL: &str = "=== function heal(ref hp, amount) ===\n~ hp = hp + amount\n~ return hp\n\n";
    const HEAL2: &str =
        "=== function heal2(ref hp, ref mp) ===\n~ hp = hp + 1\n~ mp = mp + 1\n~ return hp\n\n";
    const PURE: &str = "=== function double(x) ===\n~ return x + x\n\n";

    // ── E081: remove extra argument(s) ──────────────────────────────

    #[test]
    fn offers_trim_for_over_binding() {
        let src = format!("{PURE}=== main ===\n~ temp f = #fn(double, 1, 2)\n-> DONE\n");
        let session = session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("2)").expect("cursor site")).expect("fits");
        let actions = fn_value_actions(session.db(), file, off);
        let titles: Vec<&String> = actions.iter().map(|a| &a.title).collect();
        assert_eq!(actions.len(), 1, "{titles:?}");
        assert!(actions[0].title.contains("Remove extra argument"));
        assert!(
            matches!(
                &actions[0].data,
                CodeActionData::TrimFnLiteralArgs { target, occurrence: 0, keep: 1 }
                    if target == "double"
            ),
            "{:?}",
            actions[0].data
        );
    }

    #[test]
    fn trim_resolves_and_reanalysis_clears_e081() {
        let src = format!("{PURE}=== main ===\n~ temp f = #fn(double, 1, 2)\n-> DONE\n");
        let fixed = trim_fn_literal_args(&src, "double", 0, 1).expect("resolves");
        assert_eq!(
            fixed,
            format!("{PURE}=== main ===\n~ temp f = #fn(double, 1)\n-> DONE\n")
        );

        // Prove the resulting source actually passes analysis (the E079-E081
        // house rule): re-run the same per-file diagnostics pass the offer
        // gated on and confirm E081 is gone — and that the fix landed in a
        // project that actually parses `#fn(...)` at all (Brink dialect),
        // not vacuously passing because E051 (extension syntax) ate the
        // diagnostic surface first.
        let mut session = IdeSession::new();
        session.set_language_dialect(brink_analyzer::Dialect::Brink);
        session.update_and_analyze("test.ink", fixed);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session.db().diagnostics(file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E081),
            "{diags:?}"
        );
    }

    #[test]
    fn trim_returns_none_when_closing_paren_is_missing() {
        // Unterminated `#fn(...)` — parser error-recovery (`p.expect
        // (R_PAREN)` without a `)` to consume) leaves the FN_LITERAL node
        // without ever bumping an `R_PAREN` token, so its `text_range().
        // end()` does not land on a `)` byte. Splicing at `end() - 1` would
        // silently fuse the "2" argument with the newline that actually
        // follows the node instead of failing safe.
        let src = format!("{PURE}=== main ===\n~ temp f = #fn(double, 1, 2\n-> DONE\n");
        assert_eq!(trim_fn_literal_args(&src, "double", 0, 1), None);
    }

    #[test]
    fn no_trim_offer_when_binding_is_exact() {
        let src = format!("{PURE}=== main ===\n~ temp f = #fn(double, 1)\n-> DONE\n");
        let session = session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("1)").expect("cursor site")).expect("fits");
        assert!(fn_value_actions(session.db(), file, off).is_empty());
    }

    // ── E080: bind ref argument(s) ───────────────────────────────────

    #[test]
    fn offers_bind_for_unbound_ref_param_with_matching_var() {
        let src = format!("{HEAL}VAR hp = 10\n=== main ===\n~ temp f = #fn(heal)\n-> DONE\n");
        let session = session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("#fn(heal)").expect("site") + 5).expect("fits");
        let actions = fn_value_actions(session.db(), file, off);
        let titles: Vec<&String> = actions.iter().map(|a| &a.title).collect();
        assert_eq!(actions.len(), 1, "{titles:?}");
        assert!(actions[0].title.contains("Bind `hp`"));
        assert!(
            matches!(
                &actions[0].data,
                CodeActionData::BindFnLiteralRefArgs { target, occurrence: 0, vars }
                    if target == "heal" && vars == &["hp".to_owned()]
            ),
            "{:?}",
            actions[0].data
        );
    }

    #[test]
    fn bind_resolves_and_reanalysis_clears_e080() {
        let src = format!("{HEAL}VAR hp = 10\n=== main ===\n~ temp f = #fn(heal)\n-> DONE\n");
        let fixed =
            bind_fn_literal_ref_args(&src, "heal", 0, &["hp".to_owned()]).expect("resolves");
        assert_eq!(
            fixed,
            format!("{HEAL}VAR hp = 10\n=== main ===\n~ temp f = #fn(heal, hp)\n-> DONE\n")
        );

        let mut session = IdeSession::new();
        session.set_language_dialect(brink_analyzer::Dialect::Brink);
        session.update_and_analyze("test.ink", fixed);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session.db().diagnostics(file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E080),
            "{diags:?}"
        );
    }

    #[test]
    fn binds_multiple_trailing_ref_params_in_one_shot() {
        let src = format!(
            "{HEAL2}VAR hp = 10\nVAR mp = 5\n=== main ===\n~ temp f = #fn(heal2)\n-> DONE\n"
        );
        let fixed = bind_fn_literal_ref_args(&src, "heal2", 0, &["hp".to_owned(), "mp".to_owned()])
            .expect("resolves");
        assert_eq!(
            fixed,
            format!(
                "{HEAL2}VAR hp = 10\nVAR mp = 5\n=== main ===\n~ temp f = #fn(heal2, hp, mp)\n-> DONE\n"
            )
        );

        let mut session = IdeSession::new();
        session.set_language_dialect(brink_analyzer::Dialect::Brink);
        session.update_and_analyze("test.ink", fixed);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session.db().diagnostics(file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E080),
            "{diags:?}"
        );
    }

    #[test]
    fn no_bind_offer_when_no_matching_var_in_scope() {
        // `heal`'s ref param is `hp`, but no `VAR hp` exists — nothing safe
        // to bind, so the fix must not be offered.
        let src = format!("{HEAL}=== main ===\n~ temp f = #fn(heal)\n-> DONE\n");
        let session = session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("#fn(heal)").expect("site") + 5).expect("fits");
        assert!(fn_value_actions(session.db(), file, off).is_empty());
    }

    #[test]
    fn no_bind_offer_when_an_already_bound_ref_arg_has_its_own_e080() {
        // `heal2`'s first ref param (`hp`) is bound to `t`, a temp — not a
        // durable cell, so it carries its own E080. The second ref param
        // (`mp`) is unbound and has a matching `VAR mp` in scope, so in
        // isolation the span-fill logic would happily offer "bind `mp`" —
        // but applying that fix would still leave `hp`'s E080 behind,
        // contradicting the "always leaves the call fully bound" guarantee
        // (module doc). No fix must be offered at all.
        let src = "=== function heal2(ref hp, ref mp) ===\n~ hp = hp + 1\n~ mp = mp + 1\n\
                   ~ return hp\n\nVAR mp = 5\n=== main ===\n~ temp t = 1\n\
                   ~ temp f = #fn(heal2, t)\n-> DONE\n"
            .to_owned();
        let session = session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session.db().diagnostics(file).expect("diagnostics");
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E080),
            "fixture must actually carry an E080 on the bound `t` arg: {diags:?}"
        );
        let off = u32::try_from(src.find("#fn(heal2").expect("site") + 5).expect("fits");
        assert!(fn_value_actions(session.db(), file, off).is_empty());
    }

    #[test]
    fn no_bind_offer_when_var_name_is_ambiguous() {
        // Two files each declare their own `#@module` with a global `VAR
        // hp`. Under M-2d cross-declared-module coexistence (issue #790,
        // `brink_analyzer::manifest::
        // cross_declared_module_duplicate_coexists_under_brink`) both
        // survive in the index as genuinely distinct `SymbolKind::Variable`
        // entries sharing the bare name `hp` — `matching_global_var` cannot
        // pick one, so no fix.
        //
        // Two files declaring `VAR hp` under the *same* (undeclared) module
        // do NOT reach this path: `symbol_index_with_modules` hashes an
        // undeclared module's names bare, so the two `hp`s collide as the
        // *same* `DefinitionId` (one duplicate-declaration diagnostic, one
        // surviving index entry) rather than genuinely two entries to
        // disambiguate between — that fixture does not exercise this guard
        // at all, it degenerates to `no_bind_offer_when_no_matching_var_in_
        // scope`'s single-entry case.
        //
        // `a.ink`'s `VAR hp` is also required in its own right: the `ref
        // hp` parameter inside `HEAL` is indexed as `SymbolKind::Param`,
        // which `matching_global_var`'s `SymbolKind::Variable` filter
        // discards, so `HEAL` alone never contributes to the ambiguity.
        let mut session = IdeSession::new();
        session.set_language_dialect(brink_analyzer::Dialect::Brink);
        let a = format!(
            "#@module(a)\n{HEAL}VAR hp = 10\n=== main ===\n~ temp f = #fn(heal)\n-> DONE\n"
        );
        let b = "#@module(b)\nVAR hp = 1\n-> END\n".to_owned();
        session.update_source("a.ink", a.clone());
        session.update_source("b.ink", b.clone());
        session.update_and_analyze("a.ink", a.clone());
        session.update_and_analyze("b.ink", b);
        let file = session.file_id("a.ink").expect("file id");
        let off = u32::try_from(a.find("#fn(heal)").expect("site") + 5).expect("fits");
        assert!(fn_value_actions(session.db(), file, off).is_empty());
    }

    #[test]
    fn no_offer_where_there_is_no_diagnostic() {
        let src = format!("{PURE}=== main ===\n~ temp f = #fn(double, 1)\n-> DONE\n");
        let session = session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("double").expect("site")).expect("fits");
        assert!(fn_value_actions(session.db(), file, off).is_empty());
    }
}
