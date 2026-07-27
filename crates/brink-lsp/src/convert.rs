use rowan::{TextRange, TextSize};
use tower_lsp::lsp_types;

pub use brink_ide::LineIndex;

pub fn to_lsp_range(range: TextRange, idx: &LineIndex) -> lsp_types::Range {
    let (start_line, start_col) = idx.line_col(range.start());
    let (end_line, end_col) = idx.line_col(range.end());
    lsp_types::Range {
        start: lsp_types::Position::new(start_line, start_col),
        end: lsp_types::Position::new(end_line, end_col),
    }
}

pub fn to_text_size(pos: lsp_types::Position, idx: &LineIndex) -> TextSize {
    idx.offset(pos.line, pos.character)
}

pub fn symbol_kind_to_lsp(kind: brink_ir::SymbolKind) -> lsp_types::SymbolKind {
    match kind {
        brink_ir::SymbolKind::Knot | brink_ir::SymbolKind::External => {
            lsp_types::SymbolKind::FUNCTION
        }
        brink_ir::SymbolKind::Stitch => lsp_types::SymbolKind::METHOD,
        brink_ir::SymbolKind::Variable
        | brink_ir::SymbolKind::Param
        | brink_ir::SymbolKind::Temp => lsp_types::SymbolKind::VARIABLE,
        brink_ir::SymbolKind::Constant => lsp_types::SymbolKind::CONSTANT,
        brink_ir::SymbolKind::List => lsp_types::SymbolKind::ENUM,
        brink_ir::SymbolKind::ListItem => lsp_types::SymbolKind::ENUM_MEMBER,
        brink_ir::SymbolKind::Label => lsp_types::SymbolKind::KEY,
        brink_ir::SymbolKind::Struct => lsp_types::SymbolKind::STRUCT,
    }
}

/// `brink_ir::Severity::Info`/`Hint` map to LSP's `INFORMATION`/`HINT`
/// respectively (issue #1162) — the LSP spec keeps these as two distinct
/// severities (`textDocument/publishDiagnostics`'s `DiagnosticSeverity`), so
/// this maps both explicitly rather than collapsing them onto one.
pub fn severity_to_lsp(sev: brink_ir::Severity) -> lsp_types::DiagnosticSeverity {
    match sev {
        brink_ir::Severity::Error => lsp_types::DiagnosticSeverity::ERROR,
        brink_ir::Severity::Warning => lsp_types::DiagnosticSeverity::WARNING,
        brink_ir::Severity::Info => lsp_types::DiagnosticSeverity::INFORMATION,
        brink_ir::Severity::Hint => lsp_types::DiagnosticSeverity::HINT,
    }
}

/// Diagnostic codes whose flagged source range is literally unnecessary —
/// safe to delete without changing behavior — get LSP's
/// `DiagnosticTag::UNNECESSARY` (issue #1618, the tagging half of #1162 that
/// PR #1615 deferred). Clients such as VS Code render `Unnecessary`-tagged
/// diagnostics as dimmed/faded text instead of an underline, which is the
/// actual UX #1162 asked for on top of the `Info`/`Hint` severity tier.
///
/// Deliberately narrow. Included:
/// - [`E033`](brink_ir::DiagnosticCode::E033) — unreachable code after a
///   divert: the flagged statements can never execute.
/// - [`E095`](brink_ir::DiagnosticCode::E095) — `#@was(name)` naming the
///   definition's own current name: a no-op alias entry.
///
/// Deliberately excluded, despite sounding similar:
/// - `E014` ("logic line has no effect") also fires on a malformed
///   temp-decl/assignment with a missing identifier or value — that needs
///   fixing, not deleting, so dimming it would misdirect the author.
/// - [`E092`](brink_ir::DiagnosticCode::E092) — a `#@public`/`#@private`
///   override that restates the module's own default. The *directive* is
///   what's removable, but `E092`'s emission site
///   (`brink-analyzer::manifest::insert_symbol`) reports on `sym.range`,
///   which is `DeclaredSymbol::range` — the declaration's *name* span
///   (`Knot::name.range`/`Stitch::name.range` at HIR-lowering time), not the
///   directive's own range. HIR does carry a directive-level range
///   (`VisibilityDirective::range`), but it never reaches `DeclaredSymbol`:
///   `Knot::visibility`/`DeclaredSymbol::visibility` keep only
///   `Option<VisibilityMark>`, unlike `was: Option<(String, TextRange)>`,
///   which is exactly why `E095` above *can* be included. Tagging `E092`
///   today would dim the knot/stitch/VAR *name* — telling a client "this
///   definition is dead, delete it", which is false. Re-include once
///   `VisibilityDirective::range` is plumbed through to
///   `DeclaredSymbol::visibility` so `E092` can anchor on the directive
///   (tracked as a follow-up).
/// - `E131` (`<-` splice used outside a choice point) is documented as
///   ambiguous with literal dialogue punctuation the author may have meant
///   to keep — tagging it Unnecessary would tell a client to fade text that
///   might not be dead at all.
/// - `E151` (asymmetric choice-branch dead end, issue #1219) flags a branch
///   that is *missing* a divert; the flagged text itself is exactly what the
///   author needs to keep and extend, not delete.
///
/// This is independent of severity: the tag is orthogonal and applies to a
/// code's diagnostics at whatever severity they end up published at. Issue
/// #1617 has since settled `E095`'s *default* severity to `Hint` (previously
/// `Warning`, like `E033` still is) — the tag assignment above didn't need
/// to change, since it never depended on which tier the code happened to
/// default to.
fn is_unnecessary(code: brink_ir::DiagnosticCode) -> bool {
    matches!(
        code,
        brink_ir::DiagnosticCode::E033 | brink_ir::DiagnosticCode::E095
    )
}

/// `types`/`lints` are the resolved [`brink_analyzer::TypePolicy`]/
/// [`brink_analyzer::LintPolicy`] the diagnostic was produced under —
/// `severity` publishes [`brink_analyzer::effective_severity`], not the raw
/// [`brink_ir::DiagnosticCode::severity`] default, so a `[lints]` re-leveled
/// code shows at its overridden severity in the client (issue #1367).
/// `tags` carries `DiagnosticTag::UNNECESSARY` for the narrow set of codes
/// [`is_unnecessary`] recognizes (issue #1618).
pub fn diagnostic_to_lsp(
    diag: &brink_ir::Diagnostic,
    idx: &LineIndex,
    types: brink_analyzer::TypePolicy,
    lints: &brink_analyzer::LintPolicy,
) -> lsp_types::Diagnostic {
    lsp_types::Diagnostic {
        range: to_lsp_range(diag.range, idx),
        severity: Some(severity_to_lsp(brink_analyzer::effective_severity(
            diag.code, types, lints,
        ))),
        code: Some(lsp_types::NumberOrString::String(
            diag.code.as_str().to_owned(),
        )),
        source: Some("brink".to_owned()),
        message: diag.message.clone(),
        tags: is_unnecessary(diag.code).then(|| vec![lsp_types::DiagnosticTag::UNNECESSARY]),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_lsp_range_roundtrip() {
        let src = "abc\ndef\nghi";
        let idx = LineIndex::new(src);
        let range = TextRange::new(TextSize::from(4), TextSize::from(7));
        let lsp = to_lsp_range(range, &idx);
        assert_eq!(lsp.start, lsp_types::Position::new(1, 0));
        assert_eq!(lsp.end, lsp_types::Position::new(1, 3));

        let start = to_text_size(lsp.start, &idx);
        let end = to_text_size(lsp.end, &idx);
        assert_eq!(start, TextSize::from(4));
        assert_eq!(end, TextSize::from(7));
    }

    #[test]
    fn symbol_kind_mapping() {
        assert_eq!(
            symbol_kind_to_lsp(brink_ir::SymbolKind::Knot),
            lsp_types::SymbolKind::FUNCTION,
        );
        assert_eq!(
            symbol_kind_to_lsp(brink_ir::SymbolKind::List),
            lsp_types::SymbolKind::ENUM,
        );
        assert_eq!(
            symbol_kind_to_lsp(brink_ir::SymbolKind::Label),
            lsp_types::SymbolKind::KEY,
        );
    }

    #[test]
    fn severity_mapping() {
        assert_eq!(
            severity_to_lsp(brink_ir::Severity::Error),
            lsp_types::DiagnosticSeverity::ERROR,
        );
        assert_eq!(
            severity_to_lsp(brink_ir::Severity::Warning),
            lsp_types::DiagnosticSeverity::WARNING,
        );
    }

    /// #1162: `Info`/`Hint` must map to LSP's `INFORMATION`/`HINT`
    /// respectively, not collapse onto `WARNING` or onto each other.
    #[test]
    fn severity_mapping_info_and_hint_are_distinct() {
        assert_eq!(
            severity_to_lsp(brink_ir::Severity::Info),
            lsp_types::DiagnosticSeverity::INFORMATION,
        );
        assert_eq!(
            severity_to_lsp(brink_ir::Severity::Hint),
            lsp_types::DiagnosticSeverity::HINT,
        );
    }

    /// #1163 regression: a `DiagnosticCode` whose default severity is
    /// `Warning` (E014 is one of the 19 warning-default codes, after issue
    /// #1617 moved `E092`/`E095` to `Hint`) must surface
    /// as `DiagnosticSeverity::WARNING`, not `ERROR`, once routed through
    /// `diagnostic_to_lsp` with no `[lints]` override in play.
    #[test]
    fn diagnostic_to_lsp_respects_warning_default_code() {
        assert_eq!(
            brink_ir::DiagnosticCode::E014.severity(),
            brink_ir::Severity::Warning
        );

        let diag = brink_ir::Diagnostic {
            file: brink_ir::FileId(0),
            range: TextRange::new(TextSize::from(0), TextSize::from(1)),
            message: "test".to_owned(),
            code: brink_ir::DiagnosticCode::E014,
        };
        let idx = LineIndex::new("x");
        let lsp = diagnostic_to_lsp(
            &diag,
            &idx,
            brink_analyzer::TypePolicy::Gradual,
            &brink_analyzer::LintPolicy::default(),
        );
        assert_eq!(lsp.severity, Some(lsp_types::DiagnosticSeverity::WARNING));
    }

    /// #1367: a `[lints] E014 = "deny"` override must publish as
    /// `ERROR`, not the code's raw `Warning` default — `diagnostic_to_lsp`
    /// must route severity through `effective_severity`, not
    /// `diag.code.severity()`.
    #[test]
    fn diagnostic_to_lsp_respects_lints_override() {
        let diag = brink_ir::Diagnostic {
            file: brink_ir::FileId(0),
            range: TextRange::new(TextSize::from(0), TextSize::from(1)),
            message: "test".to_owned(),
            code: brink_ir::DiagnosticCode::E014,
        };
        let idx = LineIndex::new("x");
        let mut lints = brink_analyzer::LintPolicy::default();
        lints
            .overrides
            .insert("E014".to_owned(), brink_analyzer::LintLevel::Deny);
        let lsp = diagnostic_to_lsp(&diag, &idx, brink_analyzer::TypePolicy::Gradual, &lints);
        assert_eq!(lsp.severity, Some(lsp_types::DiagnosticSeverity::ERROR));
    }

    /// #1162: a `[lints] E014 = "hint"` override must publish `HINT` through
    /// the same `effective_severity` seam `diagnostic_to_lsp_respects_lints_override`
    /// exercises for `deny`.
    #[test]
    fn diagnostic_to_lsp_respects_lints_hint_override() {
        let diag = brink_ir::Diagnostic {
            file: brink_ir::FileId(0),
            range: TextRange::new(TextSize::from(0), TextSize::from(1)),
            message: "test".to_owned(),
            code: brink_ir::DiagnosticCode::E014,
        };
        let idx = LineIndex::new("x");
        let mut lints = brink_analyzer::LintPolicy::default();
        lints
            .overrides
            .insert("E014".to_owned(), brink_analyzer::LintLevel::Hint);
        let lsp = diagnostic_to_lsp(&diag, &idx, brink_analyzer::TypePolicy::Gradual, &lints);
        assert_eq!(lsp.severity, Some(lsp_types::DiagnosticSeverity::HINT));
    }

    /// #1618: `E033` (unreachable code after divert) is one of the narrow
    /// unnecessary-class codes and must carry `DiagnosticTag::UNNECESSARY`
    /// so an LSP client dims the flagged range instead of underlining it.
    #[test]
    fn diagnostic_to_lsp_tags_unreachable_code_as_unnecessary() {
        let diag = brink_ir::Diagnostic {
            file: brink_ir::FileId(0),
            range: TextRange::new(TextSize::from(0), TextSize::from(1)),
            message: "test".to_owned(),
            code: brink_ir::DiagnosticCode::E033,
        };
        let idx = LineIndex::new("x");
        let lsp = diagnostic_to_lsp(
            &diag,
            &idx,
            brink_analyzer::TypePolicy::Gradual,
            &brink_analyzer::LintPolicy::default(),
        );
        assert_eq!(lsp.tags, Some(vec![lsp_types::DiagnosticTag::UNNECESSARY]));
    }

    /// #1618: `E095` (`#@was` self-alias) must carry the Unnecessary tag —
    /// unlike `E092`, its emission site anchors on a directive-level range
    /// (`was_range`, HIR `Knot::was`/`Stitch::was` carry
    /// `Option<(String, TextRange)>`), so the flagged range genuinely is the
    /// removable text.
    #[test]
    fn diagnostic_to_lsp_tags_was_self_alias_as_unnecessary() {
        let diag = brink_ir::Diagnostic {
            file: brink_ir::FileId(0),
            range: TextRange::new(TextSize::from(0), TextSize::from(1)),
            message: "test".to_owned(),
            code: brink_ir::DiagnosticCode::E095,
        };
        let idx = LineIndex::new("x");
        let lsp = diagnostic_to_lsp(
            &diag,
            &idx,
            brink_analyzer::TypePolicy::Gradual,
            &brink_analyzer::LintPolicy::default(),
        );
        assert_eq!(lsp.tags, Some(vec![lsp_types::DiagnosticTag::UNNECESSARY]));
    }

    /// #1618 review finding: `E092` (redundant `#@public`/`#@private`
    /// override) must NOT carry the Unnecessary tag, despite sounding like a
    /// no-op-directive code akin to `E095`. Its emission site
    /// (`brink-analyzer::manifest::insert_symbol`) reports on
    /// `DeclaredSymbol::range`, which is the declaration's *name* span, not
    /// the directive's own range — `Knot::visibility`/`DeclaredSymbol::visibility`
    /// keep only `Option<VisibilityMark>`, with no range to anchor on. Tagging
    /// it today would dim the knot/stitch/VAR name itself, falsely telling a
    /// client the *definition* is dead. See [`is_unnecessary`]'s doc comment.
    #[test]
    fn diagnostic_to_lsp_does_not_tag_redundant_visibility_override() {
        let diag = brink_ir::Diagnostic {
            file: brink_ir::FileId(0),
            range: TextRange::new(TextSize::from(0), TextSize::from(1)),
            message: "test".to_owned(),
            code: brink_ir::DiagnosticCode::E092,
        };
        let idx = LineIndex::new("x");
        let lsp = diagnostic_to_lsp(
            &diag,
            &idx,
            brink_analyzer::TypePolicy::Gradual,
            &brink_analyzer::LintPolicy::default(),
        );
        assert_eq!(lsp.tags, None);
    }

    /// #1617: `E092`/`E095` now default to `Hint`, so with no `[lints]`
    /// override in play `diagnostic_to_lsp` must publish `HINT`, not
    /// `WARNING` — proving the reclassification actually reaches the LSP
    /// client, not just `DiagnosticCode::severity()` in isolation.
    #[test]
    fn diagnostic_to_lsp_publishes_hint_for_reclassified_codes() {
        for code in [
            brink_ir::DiagnosticCode::E092,
            brink_ir::DiagnosticCode::E095,
        ] {
            let diag = brink_ir::Diagnostic {
                file: brink_ir::FileId(0),
                range: TextRange::new(TextSize::from(0), TextSize::from(1)),
                message: "test".to_owned(),
                code,
            };
            let idx = LineIndex::new("x");
            let lsp = diagnostic_to_lsp(
                &diag,
                &idx,
                brink_analyzer::TypePolicy::Gradual,
                &brink_analyzer::LintPolicy::default(),
            );
            assert_eq!(
                lsp.severity,
                Some(lsp_types::DiagnosticSeverity::HINT),
                "{code:?} must default to Hint"
            );
        }
    }

    /// #1617: an `Info`/`Hint`-*default* code must still be re-levelable by
    /// `[lints]` in both directions — the `effective_severity` early-return
    /// bug this issue's implementation found and fixed would have made
    /// `E092 = "deny"` silently do nothing, because a base severity below
    /// `Warning` used to skip the `[lints]` table entirely.
    #[test]
    fn diagnostic_to_lsp_lints_override_still_applies_to_hint_default_code() {
        let diag = brink_ir::Diagnostic {
            file: brink_ir::FileId(0),
            range: TextRange::new(TextSize::from(0), TextSize::from(1)),
            message: "test".to_owned(),
            code: brink_ir::DiagnosticCode::E092,
        };
        let idx = LineIndex::new("x");
        let mut lints = brink_analyzer::LintPolicy::default();
        lints
            .overrides
            .insert("E092".to_owned(), brink_analyzer::LintLevel::Deny);
        let lsp = diagnostic_to_lsp(&diag, &idx, brink_analyzer::TypePolicy::Gradual, &lints);
        assert_eq!(lsp.severity, Some(lsp_types::DiagnosticSeverity::ERROR));
    }

    /// #1618: a code that is *not* in the unnecessary class (e.g. `E014`,
    /// deliberately excluded — see [`is_unnecessary`]) must publish with no
    /// tags at all, not an empty vec — `Diagnostic::tags` is `Option<Vec<_>>`
    /// and clients treat `None` and `Some(vec![])` differently in principle,
    /// so this pins the "not tagged" case to `None`.
    #[test]
    fn diagnostic_to_lsp_does_not_tag_unrelated_codes() {
        let diag = brink_ir::Diagnostic {
            file: brink_ir::FileId(0),
            range: TextRange::new(TextSize::from(0), TextSize::from(1)),
            message: "test".to_owned(),
            code: brink_ir::DiagnosticCode::E014,
        };
        let idx = LineIndex::new("x");
        let lsp = diagnostic_to_lsp(
            &diag,
            &idx,
            brink_analyzer::TypePolicy::Gradual,
            &brink_analyzer::LintPolicy::default(),
        );
        assert_eq!(lsp.tags, None);
    }

    /// #1618: `E131` sounds like a "no effect" no-op akin to `E033`, but its
    /// own doc comment flags it as ambiguous with literal dialogue
    /// punctuation — it must NOT be tagged Unnecessary (see
    /// [`is_unnecessary`]'s exclusion list). Pinned as a regression guard
    /// against widening the tag to every `Warning`-default code.
    #[test]
    fn diagnostic_to_lsp_does_not_tag_ambiguous_splice_warning() {
        let diag = brink_ir::Diagnostic {
            file: brink_ir::FileId(0),
            range: TextRange::new(TextSize::from(0), TextSize::from(1)),
            message: "test".to_owned(),
            code: brink_ir::DiagnosticCode::E131,
        };
        let idx = LineIndex::new("x");
        let lsp = diagnostic_to_lsp(
            &diag,
            &idx,
            brink_analyzer::TypePolicy::Gradual,
            &brink_analyzer::LintPolicy::default(),
        );
        assert_eq!(lsp.tags, None);
    }
}
