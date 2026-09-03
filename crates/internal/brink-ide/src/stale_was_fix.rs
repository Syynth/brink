//! `Safe` fixer for `E095` (`#@was(name)` naming the definition's own
//! current name — nothing to migrate): delete the stale `#@was(…)` tag —
//! issue #3425, milestone 8 of the auto-fix epic (#3374,
//! `docs/autofix-spec.md` §9's first-wave `Safe` list).
//!
//! # Why this is mechanically `Safe` — WITH one narrowing guard
//!
//! `#@was(old_name)` is `brink_ir::hir::lower::directive::was_from_directives`'s
//! rename-migration record (M-3, `docs/modules-spec.md` §5/§7): every call
//! site that reads it (`structure::knot::lower_knot`,
//! `structure::stitch::{lower_top_level_stitch,lower_stitch}`,
//! `decl::{var,constant,list,external}`, and the file-level module record in
//! `structure::mod::lower_file`) compares `old_name` against the target's
//! *current* name **before** storing anything — when they're equal, `E095`
//! is diagnosed and the directive's payload is dropped on the spot; the
//! `was`/`ModuleDecl.was` field *that owner* builds is left `None` for that
//! occurrence. In isolation that reasoning is sound: a self-aliasing
//! occurrence never reaches that owner's own alias-table codegen, and
//! deleting the tag removes a value that owner never read.
//!
//! But `#@module`'s file-level `#@was` and a top-level declaration's own
//! `directives_before` lookback can both claim the *same physical tag
//! line* — `apply_scope_directives`'s `module` arm documents this overlap
//! as "an entirely ordinary authoring style": a `#@was` sitting directly
//! above the file's first `VAR`/`CONST`/`LIST`/`EXTERNAL` (no blank line
//! between them) is read by *both* the module's own `file_module_was` scan
//! and that declaration's lookback. Each side only self-aliases (and only
//! `E095`s) against *its own* current name, so one side can self-alias
//! while the other reads the very same line as a live, non-self rename —
//! deleting the line to discharge one side's `E095` would silently drop the
//! other side's real `AliasEntry` (found in review, #3425: `#@module(town)`
//! / `#@was(town)` / `VAR gold = 0` self-aliases the module but the same
//! line also feeds `VAR gold`'s own `town → gold` alias; the mirror shape —
//! `#@was(gold)` self-aliasing the `VAR` while `#@module(town)` differs —
//! loses the *module's* alias instead). [`fixes`](StaleWasFixer::fixes)
//! withholds the fix outright in exactly that overlap, rather than
//! widening the deletion or guessing which side "really" owns the line —
//! see [`file_leading_was_context`] and the "review-narrowing refusals"
//! tests below.
//!
//! Outside that one overlap the reasoning above is unconditional: a
//! self-aliasing `#@was` that is *not* also a live rename for whichever
//! sibling owner might read the same physical line can be deleted with no
//! further per-shape guard, unlike `E031`/`E176`'s several (`docs/decision-log.md`,
//! "E095 Safe fix needs no narrowing").
//!
//! A `#@was` naming a genuinely *different* old name from the owner that
//! diagnosed it is untouched — only an exact `old_name == current_name`
//! match ever raises `E095`, and this module only reacts to that
//! diagnostic.
//!
//! **What "downstream" means here, precisely:** a self-aliasing `#@was`
//! does *not* vanish from every pass — `directive::collect_was_directives`
//! is a flat, name-blind syntactic sweep (it records every `#@was` tag's
//! range, self-alias or not) that feeds `hir.was_directives`, which
//! `brink_analyzer::dialect_gate` flags one `E051` per occurrence under
//! `StrictInk`, and which `brink_ir::hir::emit_native`'s
//! `refuse_unsupported_file_channels` treats as an unsupported file-level
//! channel when re-emitting to the native surface. Those two passes *do*
//! read a self-aliasing occurrence. What they never do is feed it into a
//! compiled `StoryData` artifact: the claim this module relies on is
//! narrower than "nothing downstream reads it at all" — it is that no
//! compiled artifact (bytecode, alias table, exported line table) can
//! differ, because the alias-table codegen path specifically (not the
//! dialect gate, not the respell refusal) already dropped the value before
//! this fixer ever runs. Deleting the tag can only ever *remove* an `E051`
//! flag or a respell refusal that named this exact occurrence — never add
//! one — which is the fix's intended effect, not a hidden side effect.
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
//! Every `#@was` reached via the **tag channel** (`#@was(...)`, the only
//! spelling `was_from_directives` recognizes) is the **sole** tag on its
//! own `TAG_LINE` when it self-aliases — `directives_before`/
//! `leading_body_directives` collect it via `scan_tag_line`, which
//! classifies a line `TagLineClass::Directives` only when it carries
//! **exactly one** tag and no plain ones (`brink-syntax`'s `tag_line`
//! grammar: `tag_line = { tags ~ NEWLINE }`, so the `TAG_LINE` node's own
//! range already includes the trailing `NEWLINE` token when the parser
//! found one). That is *not* the only way `E095` can fire, though: NS-A2's
//! `@[was(name)]` **annotation**-line spelling is also collected
//! (`leading_body_directives` folds `@[…]` lines into the same `dirs` list,
//! and `was_from_directives` never checks `from_annotation`), and an
//! `ANNOTATION_LINE` is a different node kind entirely — not a `TAG_LINE`,
//! so it carries no sole-tag structure to find. [`was_tag_line`] only ever
//! matches an `ast::TagLine`, so a diagnostic anchored on an annotation
//! line's range finds nothing and the fixer correctly offers no fix there
//! (see `no_fix_when_e095_is_raised_by_an_annotation_line`) — it is not
//! that annotation-spelled self-aliases are unreachable, only that this
//! fixer doesn't yet handle that spelling. So the diagnostic's own range
//! (`d.range`, exactly the
//! `Tag` node's `text_range()` — see `parse_directive_tag`) always
//! identifies its enclosing `TAG_LINE` uniquely, and deleting that whole
//! `TAG_LINE` range removes the entire physical line — tag, any leading
//! indentation (`tag()` calls `p.skip_ws()` *inside* its own node, before
//! bumping the `#`) and the trailing newline — cleanly, with no blank line
//! left behind.

use brink_ir::{Diagnostic, DiagnosticCode};
use brink_syntax::ast::AstNode as _;
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
        let Some(tag_line) = was_tag_line(source, d.range) else {
            return Vec::new();
        };
        // Review-narrowing (#3425): this exact physical line can also be
        // read by a *different* owner than the one that just self-aliased —
        // this module's doc, "Why this is mechanically Safe". Withhold
        // outright rather than deleting a line that still carries a live,
        // non-self rename for that other owner.
        if let Some((was_arg, module_name)) = file_leading_was_context(&tag_line) {
            if let Some(decl_name) = attached_decl_name(&tag_line)
                && Some(decl_name.as_str()) != was_arg.as_deref()
            {
                // Case (i): a file-level module self-alias, but the same
                // line also feeds a following VAR/CONST/LIST/EXTERNAL whose
                // name differs — that declaration's own alias is live.
                return Vec::new();
            }
            if let Some(module_name) = module_name
                && was_arg.as_deref() != Some(module_name.as_str())
            {
                // Case (ii): a declaration-level self-alias, but the file's
                // leading run also carries a `#@module` whose name differs —
                // the module's own alias is live.
                return Vec::new();
            }
        }
        let line_range = tag_line.syntax().text_range();
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
/// (the diagnostic's own anchor — see this module's doc, "Deletion shape").
///
/// `None` when no such line exists (a stale/mismatched range, or — per that
/// same doc section — a diagnostic anchored on an `@[was(…)]` annotation
/// line, which is a different node kind and carries no sole-tag structure
/// to find here) — narrowing rather than guessing at a fallback span, per
/// `docs/autofix-spec.md` §3's "narrow, never downgrade".
fn was_tag_line(source: &str, target_range: TextRange) -> Option<brink_syntax::ast::TagLine> {
    use brink_syntax::ast;

    let parse = brink_syntax::parse(source);
    let root = parse.tree().syntax().clone();

    root.descendants().find_map(|node| {
        let tag_line = ast::TagLine::cast(node)?;
        let tags = tag_line.tags()?;
        let mut it = tags.tags();
        let sole = it.next()?;
        if it.next().is_some() {
            // Not a pure single-directive line — every tag-channel
            // `E095`-raising `#@was` is (this module's doc), so this line
            // isn't the one.
            return None;
        }
        (sole.syntax().text_range() == target_range).then(|| tag_line.clone())
    })
}

/// A tag line's directive name and balanced-`(…)` argument text, trimmed —
/// the same text extraction
/// `brink_ir::hir::lower::directive::parse_directive_tag` does (that
/// function is `pub(super)` inside `brink-ir` and unreachable from this
/// crate), restricted to the two fields this module's narrowing needs. Only
/// meaningful for a pure single-tag line whose tag starts with `@`.
fn tag_name_and_arg(tag: &brink_syntax::ast::Tag) -> Option<(String, Option<String>)> {
    use brink_syntax::SyntaxKind;

    let mut text = String::new();
    let mut first = true;
    for child in tag.syntax().children_with_tokens() {
        if let rowan::NodeOrToken::Token(tok) = child {
            if first && tok.kind() == SyntaxKind::HASH {
                first = false;
                continue;
            }
            first = false;
            text.push_str(tok.text());
        } else {
            first = false;
        }
    }
    let trimmed = text.trim();
    let rest = trimmed.strip_prefix('@')?;
    let name: String = rest
        .chars()
        .take_while(|c| *c != '(' && !c.is_whitespace())
        .collect();
    let after_name = rest.trim_start_matches(|c: char| c != '(');
    let arg = after_name
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .map(|s| s.trim().to_string());
    Some((name, arg))
}

/// The sole directive's `(name, arg)` on a tag line carrying exactly one
/// tag, or `None` for a mixed/plain line (mirrors
/// `directive::scan_tag_line`'s single-directive classification).
fn sole_tag_name_and_arg(tl: &brink_syntax::ast::TagLine) -> Option<(String, Option<String>)> {
    let tags = tl.tags()?;
    let mut it = tags.tags();
    let sole = it.next()?;
    if it.next().is_some() {
        return None;
    }
    tag_name_and_arg(&sole)
}

/// If `tl` is a lone `#@was(...)` tag line sitting in the file's leading
/// run (only trivia / tag lines / empty lines under the `SOURCE_FILE` root
/// precede it — the one placement `directive::is_file_was_line` recognizes
/// as the module's own rename record), returns `(was_arg, module_name)`:
/// `module_name` is the name of a `#@module(...)` line found among the
/// preceding tag lines in that same run, if any. `None` when `tl` isn't in
/// that placement at all — the caller's narrowing only applies to the one
/// physical-line overlap this module's doc describes.
fn file_leading_was_context(
    tl: &brink_syntax::ast::TagLine,
) -> Option<(Option<String>, Option<String>)> {
    use brink_syntax::SyntaxKind;
    use brink_syntax::ast;

    let (name, arg) = sole_tag_name_and_arg(tl)?;
    if name != "was" {
        return None;
    }
    let parent = tl.syntax().parent()?;
    if parent.kind() != SyntaxKind::SOURCE_FILE {
        return None;
    }
    let mut module_name = None;
    let mut cursor = tl.syntax().prev_sibling_or_token();
    while let Some(el) = cursor {
        match el {
            rowan::NodeOrToken::Token(tok) => {
                if !(tok.kind().is_trivia() || tok.kind() == SyntaxKind::NEWLINE) {
                    return None;
                }
                cursor = tok.prev_sibling_or_token();
            }
            rowan::NodeOrToken::Node(n) => {
                if n.kind() == SyntaxKind::EMPTY_LINE {
                    cursor = n.prev_sibling_or_token();
                    continue;
                }
                let prev_tl = ast::TagLine::cast(n.clone())?;
                if let Some((pname, parg)) = sole_tag_name_and_arg(&prev_tl)
                    && pname == "module"
                {
                    module_name = parg.filter(|s| !s.is_empty());
                }
                cursor = n.prev_sibling_or_token();
            }
        }
    }
    Some((arg.filter(|s| !s.is_empty()), module_name))
}

/// The name of the `VAR`/`CONST`/`LIST`/`EXTERNAL` declaration `tl`
/// attaches to, if any — the next significant sibling (skipping trivia,
/// empty lines, and further directive lines), mirroring
/// `directive::attached_declaration`'s walk (also `pub(super)` and
/// unreachable from this crate) but returning the declaration's name
/// directly, since that's all this module's narrowing needs.
fn attached_decl_name(tl: &brink_syntax::ast::TagLine) -> Option<String> {
    use brink_syntax::SyntaxKind;
    use brink_syntax::ast;

    let mut cursor = tl.syntax().next_sibling_or_token();
    while let Some(el) = cursor {
        match el {
            rowan::NodeOrToken::Token(tok) => {
                if !(tok.kind().is_trivia() || tok.kind() == SyntaxKind::NEWLINE) {
                    return None;
                }
                cursor = tok.next_sibling_or_token();
            }
            rowan::NodeOrToken::Node(node) => {
                if node.kind() == SyntaxKind::EMPTY_LINE {
                    cursor = node.next_sibling_or_token();
                    continue;
                }
                if let Some(next_tl) = ast::TagLine::cast(node.clone()) {
                    if sole_tag_name_and_arg(&next_tl).is_some() {
                        cursor = next_tl.syntax().next_sibling_or_token();
                        continue;
                    }
                    return None;
                }
                return match node.kind() {
                    SyntaxKind::VAR_DECL => ast::VarDecl::cast(node).and_then(|d| d.name()),
                    SyntaxKind::CONST_DECL => ast::ConstDecl::cast(node).and_then(|d| d.name()),
                    SyntaxKind::LIST_DECL => ast::ListDecl::cast(node).and_then(|d| d.name()),
                    SyntaxKind::EXTERNAL_DECL => {
                        ast::ExternalDecl::cast(node).and_then(|d| d.name())
                    }
                    _ => None,
                };
            }
        }
    }
    None
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
        // not fall back to guessing a span — `was_tag_line` returns `None`
        // and the fixer offers nothing.
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

    /// Assert `fixes_at` offers nothing for the diagnostic covering `needle`
    /// — the negative counterpart of `one_fix_at`, for the review-narrowing
    /// refusals below.
    fn no_fixes_at(session: &IdeSession, path: &str, needle: &str, src: &str) {
        let file = session.file_id(path).expect("file id");
        let off = u32::try_from(src.find(needle).expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert!(
            fixes.is_empty(),
            "{:?}",
            fixes.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_fix_when_the_module_self_alias_line_also_feeds_a_differently_named_declaration() {
        // #3425 review finding 1(A): `#@was(town)` self-aliases the file's
        // `#@module(town)` — E095 fires at the module level — but the very
        // same physical line is ALSO read by `VAR gold`'s own
        // `directives_before` lookback as a live `town -> gold` rename.
        // Deleting the line would drop that declaration's real
        // `AliasEntry`, not just the redundant module-level self-alias.
        let src = "#@module(town)\n#@was(town)\nVAR gold = 0\nHello.\n-> DONE\n";
        let session = session_with("test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session.db().diagnostics(file).expect("diagnostics");
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E095),
            "{diags:?}"
        );
        no_fixes_at(&session, "test.ink", "#@was(town)", src);
    }

    #[test]
    fn no_fix_when_the_declaration_self_alias_line_also_feeds_a_differently_named_module() {
        // Mirror shape (#3425 review finding 1(B)): `#@was(gold)`
        // self-aliases `VAR gold` — E095 fires at the declaration level —
        // but the file's `#@module(town)` reads the same physical line as
        // a live `gold -> town` module rename. Deleting the line would
        // drop the module's real `AliasEntry` instead.
        let src = "#@module(town)\n#@was(gold)\nVAR gold = 0\nHello.\n-> DONE\n";
        let session = session_with("test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session.db().diagnostics(file).expect("diagnostics");
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E095),
            "{diags:?}"
        );
        no_fixes_at(&session, "test.ink", "#@was(gold)", src);
    }

    #[test]
    fn no_fix_when_e095_is_raised_by_an_annotation_line() {
        // #3425 review finding 4: NS-A2's `@[was(name)]` annotation
        // spelling also self-aliases and raises E095
        // (`leading_body_directives` folds annotation lines into the same
        // `dirs` list `was_from_directives` reads, and that function never
        // checks `from_annotation`) — but an `ANNOTATION_LINE` is not a
        // `TAG_LINE`, so `was_tag_line` finds nothing to delete. Real
        // source through the actual pipeline (this module's doc, "Deletion
        // shape"), not only the synthetic `TextRange::new(0, 1)` case
        // above.
        let src = "=== greet ===\n@[was(greet)]\nHello!\n-> DONE\n";
        let session = session_with("test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let diags = session.db().diagnostics(file).expect("diagnostics");
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E095),
            "{diags:?}"
        );
        no_fixes_at(&session, "test.ink", "@[was(greet)]", src);
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
