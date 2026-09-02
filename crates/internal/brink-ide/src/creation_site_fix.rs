//! Quick-fixes for the T1c `#fn(target, args…)` creation-site diagnostics
//! (issue #744; the checks themselves live in
//! `brink_analyzer::fn_values`, docs/t1c-spec.md §2):
//!
//! - **E081** (over-binding): the bound args are longer than the target's
//!   declared param row — [`TrimFnLiteralArgsFixer`] offers "remove extra
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
//! Both fixers key off the analyzer's own diagnostic (they are dispatched
//! from [`crate::fix::fixes_for`] on `d.code`, and anchor their structural
//! search at `d.range.start()`) — same posture as [`crate::import_fix`]'s
//! E025 fixer: the diagnostic pass, not this module, owns the rule.
//!
//! `#fn(...)` is ink-frontend-only syntax (there is no native-dialect
//! spelling — `brink_ir::hir::lower_native` never lowers a `FnLiteral`), so
//! this module parses with `brink_syntax` unconditionally and skips native
//! files via [`brink_db::ProjectDb::is_native`], mirroring the dialect
//! branch [`crate::import_fix::import_edit`] takes for the same reason.
//!
//! Both fixes are expressed as minimal [`FileEdit`]s over the located
//! `#fn(...)` site — the one fix currency (`docs/autofix-spec.md` §2).

use brink_db::ProjectDb;
use brink_ir::{Diagnostic, DiagnosticCode, FileId, SymbolIndex, SymbolInfo, SymbolKind};
use brink_syntax::ast::{AstNode as _, FnLiteral};
use rowan::{TextRange, TextSize};

use crate::fix::{Applicability, Fix, FixCx, Fixer};
use crate::rename::FileEdit;

/// The `E081` over-binding fixer: trim the creation site's bound-argument
/// list back to the target's declared param row.
pub struct TrimFnLiteralArgsFixer;

impl Fixer for TrimFnLiteralArgsFixer {
    fn code(&self) -> DiagnosticCode {
        DiagnosticCode::E081
    }

    /// The discarded arguments were being bound, not ignored — dropping them
    /// loses author-written text, which §3 puts below `Safe`.
    fn max_applicability(&self) -> Applicability {
        Applicability::Suggested
    }

    fn fixes(&self, cx: &FixCx<'_>, d: &Diagnostic) -> Vec<Fix> {
        let Some(site) = CreationSite::locate(cx.db, d) else {
            return Vec::new();
        };
        let keep = site.info.params.len();
        if site.args_len <= keep {
            return Vec::new();
        }
        let Some(range) = site.trailing_args_range(keep) else {
            return Vec::new();
        };
        vec![Fix {
            code: DiagnosticCode::E081,
            title: format!(
                "Remove extra argument(s) — `{}` declares {keep} parameter(s)",
                site.target_name
            ),
            applicability: Applicability::Suggested,
            edits: vec![FileEdit {
                file: d.file,
                range,
                new_text: String::new(),
            }],
            caret: None,
        }]
    }
}

/// The `E080` unbound-`ref`-param fixer: append the durable global `VAR`s
/// that fill the unbound trailing `ref` params.
pub struct BindRefArgsFixer;

impl Fixer for BindRefArgsFixer {
    fn code(&self) -> DiagnosticCode {
        DiagnosticCode::E080
    }

    /// Binding a specific `VAR` is a guess about which cell the author meant
    /// (an unambiguous same-named one, but still a choice), so `Suggested`.
    fn max_applicability(&self) -> Applicability {
        Applicability::Suggested
    }

    fn fixes(&self, cx: &FixCx<'_>, d: &Diagnostic) -> Vec<Fix> {
        let Some(site) = CreationSite::locate(cx.db, d) else {
            return Vec::new();
        };
        // Only offered when no *already-bound* argument itself carries an
        // E080 (see `has_e080_on_bound_arg`'s doc) — otherwise the fix would
        // add the missing args and still leave the call not compiling.
        if has_e080_on_bound_arg(cx.db, d.file, &site.literal, site.args_len) {
            return Vec::new();
        }
        let index = cx.db.symbol_index();
        let Some(vars) = bind_ref_vars(&index, &site.info.params, site.args_len) else {
            return Vec::new();
        };

        let title = if vars.len() == 1 {
            format!(
                "Bind `{}` as the ref argument for `{}`",
                vars[0], site.target_name
            )
        } else {
            let joined = vars
                .iter()
                .map(|v| format!("`{v}`"))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "Bind {joined} as the ref arguments for `{}`",
                site.target_name
            )
        };
        vec![Fix {
            code: DiagnosticCode::E080,
            title,
            applicability: Applicability::Suggested,
            edits: vec![FileEdit {
                file: d.file,
                range: TextRange::empty(site.args_end()),
                new_text: format!(", {}", vars.join(", ")),
            }],
            caret: None,
        }]
    }
}

/// The `#fn(target, args…)` literal a creation-site diagnostic points at,
/// plus the target's resolved declaration.
struct CreationSite {
    literal: FnLiteral,
    target_name: String,
    /// End of the target path — where the argument list starts.
    target_end: TextSize,
    info: SymbolInfo,
    args_len: usize,
}

impl CreationSite {
    /// Locate the `#fn(...)` literal `d` anchors in, and resolve its target
    /// to a function definition. `None` for a native file (no `#fn(...)`
    /// spelling), an unresolvable target, or a target that is not a function.
    fn locate(db: &ProjectDb, d: &Diagnostic) -> Option<Self> {
        if db.is_native(d.file) {
            // `#fn(...)` has no native-dialect spelling — see module doc.
            return None;
        }
        let source = db.source(d.file)?;
        let at = d.range.start();

        let parse = brink_syntax::parse(source);
        let root = parse.tree().syntax().clone();

        // Both E081 and E080 anchor inside the `#fn(...)` literal — E080 at
        // the literal itself for an unbound param, at the offending argument
        // for a bound-but-not-durable one. The tightest literal covering the
        // anchor is the site in either case.
        let literal = root
            .descendants()
            .filter_map(FnLiteral::cast)
            .filter(|fl| fl.syntax().text_range().contains_inclusive(at))
            .min_by_key(|fl| fl.syntax().text_range().len())?;

        let target_path = literal.target()?;
        let target_name = target_path.full_name();

        let (resolutions, _) = db.resolve(d.file)?;
        let target_range = target_path.syntax().text_range();
        let res = resolutions.iter().find(|r| r.range == target_range)?;
        let index = db.symbol_index();
        let info = index.symbols.get(&res.target)?;
        let is_function_def = matches!(info.kind, SymbolKind::Knot | SymbolKind::Stitch)
            && info.detail.as_deref() == Some("function");
        if !is_function_def {
            return None;
        }

        let args_len = literal.args().count();
        Some(Self {
            literal,
            target_name,
            target_end: target_range.end(),
            info: info.clone(),
            args_len,
        })
    }

    /// The byte range covering every argument after the first `keep` — the
    /// span an over-binding trim deletes. Runs from the end of the last kept
    /// item (the target path when `keep == 0`) to the literal's real closing
    /// `)`.
    ///
    /// `None` when the parser never consumed a `)` (an unterminated
    /// `#fn(...)` under error recovery): assuming `text_range().end() - 1` is
    /// a `)` byte would silently fuse the last kept argument with whatever
    /// follows the node. See [`crate::text::closing_paren_offset`].
    fn trailing_args_range(&self, keep: usize) -> Option<TextRange> {
        let start = if keep == 0 {
            self.target_end
        } else {
            self.literal
                .args()
                .nth(keep - 1)?
                .syntax()
                .text_range()
                .end()
        };
        let close_paren = crate::text::closing_paren_offset(self.literal.syntax())?;
        Some(TextRange::new(
            start,
            TextSize::from(u32::try_from(close_paren).unwrap_or(u32::MAX)),
        ))
    }

    /// Where an appended argument goes: after the last bound argument, or
    /// straight after the target path when nothing is bound yet.
    fn args_end(&self) -> TextSize {
        self.literal
            .args()
            .last()
            .map_or(self.target_end, |a| a.syntax().text_range().end())
    }
}

/// Whether any *already-bound* argument (`fl`'s args at index `< args_len`)
/// itself carries an E080 diagnostic — e.g. a `ref` param bound to a
/// temp/param/CONST/field-projection instead of a durable `VAR`
/// (`fn_values::FnValueVisitor::check_ref_arg`, which anchors that
/// diagnostic at the argument's own range, not the whole `#fn(...)`
/// literal's).
///
/// [`BindRefArgsFixer`]'s fix only ever *appends* args for the
/// currently-*unbound* trailing `ref` params — it can never clear a
/// diagnostic on an argument that is already there. Offering it anyway when
/// one of those exists would leave the call still not compiling after the
/// "fix", contradicting this module's guarantee that the fix always leaves
/// the call fully bound (see module doc). Skipping under-fixes rather than
/// guessing here — same posture as the `val`-param-inside-the-span case in
/// [`bind_ref_vars`]'s own doc.
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

/// The durable global `VAR` names that fill the E080 fix's missing bound
/// arguments, in parameter order.
///
/// The span that needs filling is `[args_len, last_ref_idx]` — up to and
/// including the *last* declared `ref` param, since any `ref` param at or
/// after `args_len` is unbound and every position up to it must get a real
/// value to keep the bound args a contiguous prefix. A trailing `val` param
/// *after* the last `ref` param needs no value (val params never require
/// binding); a `val` param *inside* the span does, and there is no safe
/// value to synthesize for it, so no fix is offered in that case — see
/// module doc.
fn bind_ref_vars(
    index: &SymbolIndex,
    params: &[brink_ir::ParamInfo],
    args_len: usize,
) -> Option<Vec<String>> {
    let last_ref_idx = params.iter().rposition(|p| p.is_ref)?;
    if last_ref_idx < args_len {
        return None;
    }
    let span = params.get(args_len..=last_ref_idx)?;
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
    Some(vars)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fix::fixes_at;
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

    /// Apply a fix's edits to `src`. The logic under test is the fixer's;
    /// this only splices (single-file fixtures, spliced back to front).
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
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        let titles: Vec<&String> = fixes.iter().map(|f| &f.title).collect();
        assert_eq!(fixes.len(), 1, "{titles:?}");
        assert_eq!(fixes[0].code, DiagnosticCode::E081);
        assert_eq!(fixes[0].applicability, Applicability::Suggested);
        assert_eq!(
            fixes[0].title,
            "Remove extra argument(s) — `double` declares 1 parameter(s)"
        );
        assert_eq!(
            applied(&src, &fixes[0]),
            format!("{PURE}=== main ===\n~ temp f = #fn(double, 1)\n-> DONE\n")
        );
    }

    #[test]
    fn trim_edit_reanalysis_clears_e081() {
        let src = format!("{PURE}=== main ===\n~ temp f = #fn(double, 1, 2)\n-> DONE\n");
        let session = session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("2)").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(fixes.len(), 1);
        let patched = applied(&src, &fixes[0]);

        // Prove the resulting source actually passes analysis (the E079-E081
        // house rule): re-run the same per-file diagnostics pass the offer
        // gated on and confirm E081 is gone — and that the fix landed in a
        // project that actually parses `#fn(...)` at all (Brink dialect),
        // not vacuously passing because E051 (extension syntax) ate the
        // diagnostic surface first.
        let after = session_with(&patched);
        let file = after.file_id("test.ink").expect("file id");
        let diags = after.db().diagnostics(file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E081),
            "{diags:?}"
        );
    }

    #[test]
    fn no_trim_edit_when_closing_paren_is_missing() {
        // Unterminated `#fn(...)` — parser error-recovery (`p.expect
        // (R_PAREN)` without a `)` to consume) leaves the FN_LITERAL node
        // without ever bumping an `R_PAREN` token, so its `text_range().
        // end()` does not land on a `)` byte. Splicing at `end() - 1` would
        // silently fuse the "2" argument with the newline that actually
        // follows the node instead of failing safe.
        let src = format!("{PURE}=== main ===\n~ temp f = #fn(double, 1, 2\n-> DONE\n");
        let session = session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session.db().diagnostics(file).expect("diagnostics");
        let e081 = diags.iter().find(|d| d.code == DiagnosticCode::E081);
        assert!(
            e081.is_some(),
            "fixture must still carry an E081 to fix: {diags:?}"
        );
        let cx = FixCx::new(session.db());
        assert!(
            TrimFnLiteralArgsFixer
                .fixes(&cx, e081.expect("just asserted above"))
                .is_empty()
        );
    }

    #[test]
    fn no_trim_offer_when_binding_is_exact() {
        let src = format!("{PURE}=== main ===\n~ temp f = #fn(double, 1)\n-> DONE\n");
        let session = session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("1)").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
    }

    // ── E080: bind ref argument(s) ───────────────────────────────────

    #[test]
    fn offers_bind_for_unbound_ref_param_with_matching_var() {
        let src = format!("{HEAL}VAR hp = 10\n=== main ===\n~ temp f = #fn(heal)\n-> DONE\n");
        let session = session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("#fn(heal)").expect("site") + 5).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        let titles: Vec<&String> = fixes.iter().map(|f| &f.title).collect();
        assert_eq!(fixes.len(), 1, "{titles:?}");
        assert_eq!(fixes[0].code, DiagnosticCode::E080);
        assert_eq!(fixes[0].title, "Bind `hp` as the ref argument for `heal`");
        assert_eq!(
            applied(&src, &fixes[0]),
            format!("{HEAL}VAR hp = 10\n=== main ===\n~ temp f = #fn(heal, hp)\n-> DONE\n")
        );
    }

    #[test]
    fn bind_edit_reanalysis_clears_e080() {
        let src = format!("{HEAL}VAR hp = 10\n=== main ===\n~ temp f = #fn(heal)\n-> DONE\n");
        let session = session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("#fn(heal)").expect("site") + 5).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(fixes.len(), 1);
        let patched = applied(&src, &fixes[0]);

        let after = session_with(&patched);
        let file = after.file_id("test.ink").expect("file id");
        let diags = after.db().diagnostics(file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E080),
            "{diags:?}"
        );
    }

    /// Two unbound `ref` params means two `E080` diagnostics on the same
    /// literal, and the fixer's one edit binds both — so the menu must show
    /// the entry once, not twice (`fixes_at`'s identical-fix collapse).
    #[test]
    fn binds_multiple_trailing_ref_params_in_one_shot() {
        let src = format!(
            "{HEAL2}VAR hp = 10\nVAR mp = 5\n=== main ===\n~ temp f = #fn(heal2)\n-> DONE\n"
        );
        let session = session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("#fn(heal2)").expect("site") + 5).expect("fits");
        let diags = session.db().diagnostics(file).expect("diagnostics");
        assert_eq!(
            diags
                .iter()
                .filter(|d| d.code == DiagnosticCode::E080)
                .count(),
            2,
            "fixture must carry one E080 per unbound ref param: {diags:?}"
        );
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(fixes.len(), 1, "identical fixes collapse into one entry");
        assert_eq!(
            fixes[0].title,
            "Bind `hp`, `mp` as the ref arguments for `heal2`"
        );
        let patched = applied(&src, &fixes[0]);
        assert_eq!(
            patched,
            format!(
                "{HEAL2}VAR hp = 10\nVAR mp = 5\n=== main ===\n~ temp f = #fn(heal2, hp, mp)\n-> DONE\n"
            )
        );

        let after = session_with(&patched);
        let file = after.file_id("test.ink").expect("file id");
        let diags = after.db().diagnostics(file).expect("diagnostics");
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
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
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
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
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
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
    }

    #[test]
    fn no_offer_where_there_is_no_diagnostic() {
        let src = format!("{PURE}=== main ===\n~ temp f = #fn(double, 1)\n-> DONE\n");
        let session = session_with(&src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("double").expect("site")).expect("fits");
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
    }

    /// `#fn(...)` in a `.brink` file has no native spelling, so the fixers
    /// decline rather than parsing it with the ink frontend (module doc).
    #[test]
    fn native_file_gets_no_creation_site_fix() {
        let mut session = IdeSession::new();
        session.set_language_dialect(brink_analyzer::Dialect::Brink);
        let src = "flow start() {\n  Hi\n}\n".to_owned();
        session.update_and_analyze("story.brink", src);
        let file = session.file_id("story.brink").expect("file id");
        let cx = FixCx::new(session.db());
        let d = Diagnostic {
            file,
            range: rowan::TextRange::empty(TextSize::from(0)),
            message: String::new(),
            code: DiagnosticCode::E081,
        };
        assert!(TrimFnLiteralArgsFixer.fixes(&cx, &d).is_empty());
        let d = Diagnostic {
            code: DiagnosticCode::E080,
            ..d
        };
        assert!(BindRefArgsFixer.fixes(&cx, &d).is_empty());
    }
}
