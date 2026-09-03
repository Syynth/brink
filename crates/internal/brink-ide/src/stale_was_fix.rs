//! `Safe` fixer for `E095` (`#@was(name)` naming the definition's own
//! current name — nothing to migrate): delete the stale `#@was(…)` tag —
//! issue #3425, milestone 8 of the auto-fix epic (#3374,
//! `docs/autofix-spec.md` §9's first-wave `Safe` list).
//!
//! # Why this is mechanically `Safe`
//!
//! `#@was(old_name)` is `brink_ir::hir::lower::directive::was_from_directives`'s
//! rename-migration record (M-3, `docs/modules-spec.md` §5/§7): every call
//! site that reads it (`structure::knot::lower_knot`,
//! `structure::stitch::{lower_top_level_stitch,lower_stitch}`,
//! `decl::{var,constant,list,external}`, and the file-level module record in
//! `structure::mod::lower_file`) compares `old_name` against the target's
//! *current* name **before** storing anything — when they're equal, `E095`
//! is diagnosed and the directive's payload is dropped on the spot; the
//! `was`/`ModuleDecl.was` field the caller builds is left `None` for that
//! occurrence. So a self-aliasing `#@was` never reaches the alias-table
//! codegen (`brink_analyzer::manifest::insert_symbol`) or anything
//! downstream of it *at all* — deleting the tag removes a value nothing
//! reads, which is `Applicability::Safe` by construction: no compiled
//! artifact (bytecode, alias table, exported line table) can differ,
//! because none of them was ever built from this occurrence.
//!
//! A `#@was` naming a genuinely *different* old name is untouched — only an
//! exact `old_name == current_name` match ever raises `E095`, and this
//! module only reacts to that diagnostic.
//!
//! # Reachability
//!
//! Reachable on the **ink-compat surface only**: every `E095` call site
//! above lives under `brink_ir::hir::lower` (the `.ink` lowering road, see
//! that module's own `pub mod lower;` / `pub mod lower_native;` split in
//! `hir/mod.rs`). The native surface's own rename record is a different
//! directive spelled `@[was("old::path")]` (`hir::lower_native::module`),
//! diagnosing a different code (`E132`) for a different malformed shape —
//! it has no self-alias check at all, so `E095` cannot fire there. This
//! fixer still checks [`brink_db::ProjectDb::is_native`] and refuses rather
//! than assume, mirroring [`crate::arity_trim_fix`]'s and
//! [`crate::import_fix`]'s own dialect branches.
//!
//! # Deletion shape
//!
//! Every `#@was` that can reach `E095` is, by construction, the **sole**
//! tag on its own `TAG_LINE` — `was_from_directives` only ever sees
//! directives `apply_scope_directives`'s siblings
//! (`directives_before`/`leading_body_directives`) collect via
//! `scan_tag_line`, which classifies a line `TagLineClass::Directives` only
//! when it carries **exactly one** tag and no plain ones (`brink-syntax`'s
//! `tag_line` grammar: `tag_line = { tags ~ NEWLINE }`, so the `TAG_LINE`
//! node's own range already includes the trailing `NEWLINE` token when the
//! parser found one). So the diagnostic's own range (`d.range`, exactly the
//! `Tag` node's `text_range()` — see `parse_directive_tag`) always
//! identifies its enclosing `TAG_LINE` uniquely, and deleting that whole
//! `TAG_LINE` range removes the entire physical line — tag, any leading
//! indentation (`tag()` calls `p.skip_ws()` *inside* its own node, before
//! bumping the `#`) and the trailing newline — cleanly, with no blank line
//! left behind.

use brink_ir::{Diagnostic, DiagnosticCode};
use rowan::TextRange;

use crate::fix::{Applicability, Fix, FixCx, Fixer};
use crate::rename::FileEdit;

/// The `E095` fixer: delete a `#@was(name)` tag that names the definition's
/// own current name.
pub struct StaleWasFixer;

impl Fixer for StaleWasFixer {
    fn code(&self) -> DiagnosticCode {
        DiagnosticCode::E095
    }

    fn max_applicability(&self) -> Applicability {
        Applicability::Safe
    }

    fn fixes(&self, cx: &FixCx<'_>, d: &Diagnostic) -> Vec<Fix> {
        // `E095` is a compat-surface (`.ink`) diagnostic only (this module's
        // doc, "Reachability") — never assume, refuse for a native file.
        if cx.db.is_native(d.file) {
            return Vec::new();
        }
        let Some(source) = cx.db.source(d.file) else {
            return Vec::new();
        };
        let Some(line_range) = was_tag_line_range(source, d.range) else {
            return Vec::new();
        };
        vec![Fix {
            code: DiagnosticCode::E095,
            title: "Remove stale `#@was` tag — it already names the current name".to_owned(),
            applicability: Applicability::Safe,
            edits: vec![FileEdit {
                file: d.file,
                range: line_range,
                new_text: String::new(),
            }],
            caret: None,
        }]
    }
}

/// Locate the `TAG_LINE` whose sole `Tag` matches `target_range` exactly
/// (the diagnostic's own anchor — see this module's doc, "Deletion shape"),
/// and return that line's whole range, trailing newline included.
///
/// `None` when no such line exists (a stale/mismatched range) — narrowing
/// rather than guessing at a fallback span, per `docs/autofix-spec.md` §3's
/// "narrow, never downgrade".
fn was_tag_line_range(source: &str, target_range: TextRange) -> Option<TextRange> {
    use brink_syntax::ast::{self, AstNode as _};

    let parse = brink_syntax::parse(source);
    let root = parse.tree().syntax().clone();

    root.descendants().find_map(|node| {
        let tag_line = ast::TagLine::cast(node)?;
        let tags = tag_line.tags()?;
        let mut it = tags.tags();
        let sole = it.next()?;
        if it.next().is_some() {
            // Not a pure single-directive line — every `E095`-raising `#@was`
            // is (this module's doc), so this line isn't the one.
            return None;
        }
        (sole.syntax().text_range() == target_range).then(|| tag_line.syntax().text_range())
    })
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

    fn one_fix_at(session: &IdeSession, path: &str, needle: &str, src: &str) -> Fix {
        let file = session.file_id(path).expect("file id");
        let off = u32::try_from(src.find(needle).expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let mut fixes = fixes_at(&cx, file, off);
        assert_eq!(
            fixes.len(),
            1,
            "{:?}",
            fixes.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        let fix = fixes.remove(0);
        assert_eq!(fix.code, DiagnosticCode::E095);
        assert_eq!(fix.applicability, Applicability::Safe);
        fix
    }

    fn assert_clears_e095(path: &str, patched: &str) {
        let after = session_with(path, patched);
        let file = after.file_id(path).expect("file id");
        let diags = after.db().diagnostics(file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E095),
            "{diags:?}"
        );
    }

    // ── knot ─────────────────────────────────────────────────────────

    #[test]
    fn knot_self_alias_is_removed() {
        let src = "=== greet ===\n#@was(greet)\nHello!\n-> DONE\n";
        let session = session_with("test.ink", src);
        let fix = one_fix_at(&session, "test.ink", "#@was(greet)", src);
        let patched = applied(src, &fix);
        assert_eq!(patched, "=== greet ===\nHello!\n-> DONE\n");
        assert_clears_e095("test.ink", &patched);
    }

    // ── top-level stitch ─────────────────────────────────────────────

    #[test]
    fn top_level_stitch_self_alias_is_removed() {
        let src = "= greet\n#@was(greet)\nHello!\n-> DONE\n";
        let session = session_with("test.ink", src);
        let fix = one_fix_at(&session, "test.ink", "#@was(greet)", src);
        let patched = applied(src, &fix);
        assert_eq!(patched, "= greet\nHello!\n-> DONE\n");
        assert_clears_e095("test.ink", &patched);
    }

    // ── nested stitch (qualified against the enclosing knot) ─────────

    #[test]
    fn nested_stitch_self_alias_is_removed() {
        // `was_from_directives` on a nested stitch qualifies the bare old
        // name against the enclosing knot before comparing
        // (`stitch.rs`'s own doc: "the enclosing knot isn't being
        // renamed") — `#@was(inner)` under `knot.inner` self-aliases even
        // though the tag names only the bare stitch name.
        let src = "=== knot ===\n-> inner\n= inner\n#@was(inner)\nHello!\n-> DONE\n";
        let session = session_with("test.ink", src);
        let fix = one_fix_at(&session, "test.ink", "#@was(inner)", src);
        let patched = applied(src, &fix);
        assert_eq!(
            patched,
            "=== knot ===\n-> inner\n= inner\nHello!\n-> DONE\n"
        );
        assert_clears_e095("test.ink", &patched);
    }

    // ── VAR ──────────────────────────────────────────────────────────

    #[test]
    fn var_self_alias_is_removed() {
        let src = "#@was(score)\nVAR score = 0\nHello.\n-> DONE\n";
        let session = session_with("test.ink", src);
        let fix = one_fix_at(&session, "test.ink", "#@was(score)", src);
        let patched = applied(src, &fix);
        assert_eq!(patched, "VAR score = 0\nHello.\n-> DONE\n");
        assert_clears_e095("test.ink", &patched);
    }

    // ── CONST ────────────────────────────────────────────────────────

    #[test]
    fn const_self_alias_is_removed() {
        let src = "#@was(limit)\nCONST limit = 5\nHello.\n-> DONE\n";
        let session = session_with("test.ink", src);
        let fix = one_fix_at(&session, "test.ink", "#@was(limit)", src);
        let patched = applied(src, &fix);
        assert_eq!(patched, "CONST limit = 5\nHello.\n-> DONE\n");
        assert_clears_e095("test.ink", &patched);
    }

    // ── LIST ─────────────────────────────────────────────────────────

    #[test]
    fn list_self_alias_is_removed() {
        let src = "#@was(colors)\nLIST colors = red, green, blue\nHello.\n-> DONE\n";
        let session = session_with("test.ink", src);
        let fix = one_fix_at(&session, "test.ink", "#@was(colors)", src);
        let patched = applied(src, &fix);
        assert_eq!(patched, "LIST colors = red, green, blue\nHello.\n-> DONE\n");
        assert_clears_e095("test.ink", &patched);
    }

    // ── EXTERNAL ─────────────────────────────────────────────────────

    #[test]
    fn external_self_alias_is_removed() {
        let src = "#@was(shout)\nEXTERNAL shout(msg)\nHello.\n-> DONE\n";
        let session = session_with("test.ink", src);
        let fix = one_fix_at(&session, "test.ink", "#@was(shout)", src);
        let patched = applied(src, &fix);
        assert_eq!(patched, "EXTERNAL shout(msg)\nHello.\n-> DONE\n");
        assert_clears_e095("test.ink", &patched);
    }

    // ── file-level module rename record ──────────────────────────────

    #[test]
    fn file_module_self_alias_is_removed() {
        let src = "#@module(town)\n#@was(town)\nHello.\n-> DONE\n";
        let session = session_with("test.ink", src);
        let fix = one_fix_at(&session, "test.ink", "#@was(town)", src);
        let patched = applied(src, &fix);
        assert_eq!(patched, "#@module(town)\nHello.\n-> DONE\n");
        assert_clears_e095("test.ink", &patched);
    }

    // ── a `#@was` naming a genuinely different old name is untouched ──

    #[test]
    fn a_was_naming_a_different_old_name_raises_no_e095_and_offers_no_fix() {
        let src = "=== greet ===\n#@was(oldGreet)\nHello!\n-> DONE\n";
        let session = session_with("test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session.db().diagnostics(file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E095),
            "a different old name must not raise E095: {diags:?}"
        );
    }

    // ── review-narrowing refusals ─────────────────────────────────────

    #[test]
    fn no_fix_on_the_native_surface() {
        // `E095` never fires on `.brink` (this module's doc,
        // "Reachability") — proven here by calling the fixer directly
        // against a native file id rather than relying on the diagnostic
        // never existing, so a future change that somehow raised E095 on a
        // native file could not silently earn an unsafe fix.
        let mut session = IdeSession::new();
        session.set_language_dialect(brink_analyzer::Dialect::Brink);
        let src = "flow greet() {\n  Hello!\n  -> END\n}\n";
        session.update_source("test.brink", src.to_string());
        session.update_and_analyze("test.brink", src.to_string());
        let file = session.file_id("test.brink").expect("file id");
        let cx = FixCx::new(session.db());
        let fake = Diagnostic {
            file,
            range: TextRange::new(0.into(), 4.into()),
            message: DiagnosticCode::E095.title().to_owned(),
            code: DiagnosticCode::E095,
        };
        assert!(StaleWasFixer.fixes(&cx, &fake).is_empty());
    }

    #[test]
    fn no_fix_when_the_diagnostic_range_matches_no_tag_line() {
        // A stale/mismatched range (no `TAG_LINE` at all carries it) must
        // not fall back to guessing a span — `was_tag_line_range` returns
        // `None` and the fixer offers nothing.
        let src = "=== greet ===\n#@was(greet)\nHello!\n-> DONE\n";
        let session = session_with("test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let cx = FixCx::new(session.db());
        let fake = Diagnostic {
            file,
            range: TextRange::new(0.into(), 1.into()),
            message: DiagnosticCode::E095.title().to_owned(),
            code: DiagnosticCode::E095,
        };
        assert!(StaleWasFixer.fixes(&cx, &fake).is_empty());
    }

    #[test]
    fn e095_reanalysis_clears_after_applying_the_offered_fix() {
        // End-to-end through `fixes_at` exactly like a real Problems-panel
        // click: offer, apply, reanalyze, diagnostic gone.
        let src = "#@was(hp)\nVAR hp = 10\nHello.\n-> DONE\n";
        let session = session_with("test.ink", src);
        let fix = one_fix_at(&session, "test.ink", "#@was(hp)", src);
        let patched = applied(src, &fix);
        assert_eq!(patched, "VAR hp = 10\nHello.\n-> DONE\n");
        assert_clears_e095("test.ink", &patched);
    }
}
