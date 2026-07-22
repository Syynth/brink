//! Format-agnostic doc-comment content parsing (B0.6b, `docs/decision-log.md`
//! 2026-07-20): "what a doc block *means*" (`@param`/`@returns`/`@kind`
//! tag handling, E038/E043 diagnostics) — shared by both the OLD ink
//! frontend's trivia-walk attachment (`hir::lower::doc_comment`,
//! `collect_doc_lines`) and the native frontend's CST-node attachment
//! (`hir::lower_native::doc_comment`, `ast::DocComment::lines`). Factored
//! out per the ruling so "what a doc block means" stays independent of "how
//! it gets ATTACHED to a declaration" — a concern each frontend owns for
//! itself (trivia walk vs. structural CST child), all funneling into this
//! one `parse_lines`.
//!
//! Deliberately has no dependency on either frontend's `SyntaxNode` type
//! (`brink_syntax` or `brink_syntax_native`) — every function here operates
//! on plain `(String, TextRange)` lines, already extracted by the caller.

use rowan::TextRange;

use crate::{DocBlock, ExternalKind, TypeRef};

/// Which doc-comment tags are meaningful for a declaration kind. Tags that
/// are well-formed but not allowed by the policy are dropped and reported as
/// inapplicable (E043).
#[derive(Debug, Clone, Copy)]
pub struct DocPolicy {
    /// `@param` / `@returns` — signature tags for callables.
    pub allow_params: bool,
    /// `@kind` — the host-capability category, externals only.
    pub allow_kind: bool,
}

impl DocPolicy {
    /// `EXTERNAL` declarations: all tags.
    pub const EXTERNAL: Self = Self {
        allow_params: true,
        allow_kind: true,
    };
    /// Knots and stitches: signature tags, but no `@kind`.
    pub const CALLABLE: Self = Self {
        allow_params: true,
        allow_kind: false,
    };
    /// `VAR` / `CONST` / `LIST` (`flags`, native): free text only.
    pub const VALUE: Self = Self {
        allow_params: false,
        allow_kind: false,
    };
}

/// Problem ranges found while parsing a doc block, for the caller to
/// diagnose. Deliberately has no `diagnose` method here — turning these
/// ranges into `Diagnostic`s needs frontend-specific machinery (the OLD
/// parser's `LowerSink`; native's plain `Vec<Diagnostic>` + `FileId`) that
/// this format-agnostic module has no business depending on. Each frontend
/// consumes the two `Vec<TextRange>` fields directly
/// (`hir::lower::doc_comment`'s `impl DocIssues { fn diagnose }`;
/// `hir::lower_native::doc_comment::emit_issues`).
#[derive(Debug, Default)]
pub struct DocIssues {
    /// Tags that failed to parse (→ E038).
    pub malformed: Vec<TextRange>,
    /// Well-formed tags not applicable to this declaration kind (→ E043).
    pub inapplicable: Vec<TextRange>,
}

/// Parse already-collected doc-comment lines into a [`DocBlock`], returning
/// the ranges of malformed and policy-inapplicable tags. `lines` is
/// `(text-after-marker, source-range)` pairs in source order — the shape
/// both frontends' attachment step produces (`collect_doc_lines`'s trivia
/// walk; `ast::DocComment::lines`'s CST-node read).
#[must_use]
pub fn parse_lines(
    lines: &[(String, TextRange)],
    policy: DocPolicy,
) -> (Option<DocBlock>, DocIssues) {
    let mut doc = DocBlock::default();
    let mut free: Vec<String> = Vec::new();
    let mut issues = DocIssues::default();

    for (line, range) in lines {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix('@') {
            let mut it = rest.splitn(2, char::is_whitespace);
            let tag = it.next().unwrap_or("");
            let arg = it.next().unwrap_or("").trim();
            match tag {
                "param" if !policy.allow_params => issues.inapplicable.push(*range),
                "param" => match parse_param(arg) {
                    Some(entry) => doc.params.push(entry),
                    None => issues.malformed.push(*range),
                },
                "returns" | "return" if !policy.allow_params => {
                    issues.inapplicable.push(*range);
                }
                "returns" | "return" => match parse_braced_type(arg) {
                    Some(ty) => doc.returns = Some(ty),
                    None => issues.malformed.push(*range),
                },
                "kind" if !policy.allow_kind => issues.inapplicable.push(*range),
                "kind" => match ExternalKind::from_tag(arg) {
                    Some(kind) => doc.kind = Some(kind),
                    None => issues.malformed.push(*range),
                },
                // `@widget` is recognized but reserved for Tier 3, and unknown
                // tags are ignored leniently — both are no-ops at the MVP.
                _ => {}
            }
        } else if !line.is_empty() {
            free.push(line.to_string());
        }
    }

    if !free.is_empty() {
        doc.doc = Some(free.join("\n"));
    }
    let has_content =
        doc.doc.is_some() || !doc.params.is_empty() || doc.returns.is_some() || doc.kind.is_some();
    (has_content.then_some(doc), issues)
}

/// Parse a `@param` argument: `<name> {<type>}`.
fn parse_param(arg: &str) -> Option<(String, TypeRef)> {
    let mut it = arg.splitn(2, char::is_whitespace);
    let name = it.next().unwrap_or("").trim();
    if name.is_empty() {
        return None;
    }
    let ty = parse_braced_type(it.next().unwrap_or("").trim())?;
    Some((name.to_string(), ty))
}

/// Parse a `{<type>}`-braced type reference. `None` if not well-formed.
fn parse_braced_type(s: &str) -> Option<TypeRef> {
    let inner = s.trim().strip_prefix('{')?.strip_suffix('}')?.trim();
    if inner.is_empty() {
        return None;
    }
    Some(TypeRef(inner.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(n: u32) -> TextRange {
        TextRange::new(n.into(), (n + 1).into())
    }

    fn lines(strs: &[&str]) -> Vec<(String, TextRange)> {
        strs.iter()
            .enumerate()
            .map(|(i, s)| {
                let i = u32::try_from(i).expect("test fixtures stay well under u32::MAX lines");
                ((*s).to_string(), range(i))
            })
            .collect()
    }

    #[test]
    fn free_text_and_param_and_returns() {
        let (doc, issues) = parse_lines(
            &lines(&[
                "Whether the player holds an item.",
                "@param item {item_id}",
                "@returns {bool}",
            ]),
            DocPolicy::EXTERNAL,
        );
        let doc = doc.expect("doc present");
        assert!(issues.malformed.is_empty());
        assert_eq!(
            doc.doc.as_deref(),
            Some("Whether the player holds an item.")
        );
        assert_eq!(
            doc.params,
            vec![("item".to_string(), TypeRef("item_id".to_string()))]
        );
        assert_eq!(doc.returns, Some(TypeRef("bool".to_string())));
    }

    #[test]
    fn no_lines_is_none() {
        let (doc, issues) = parse_lines(&[], DocPolicy::VALUE);
        assert!(doc.is_none());
        assert!(issues.malformed.is_empty());
    }

    #[test]
    fn malformed_param_is_reported_and_dropped() {
        let (doc, issues) = parse_lines(&lines(&["@param item"]), DocPolicy::EXTERNAL);
        assert_eq!(issues.malformed.len(), 1);
        assert!(doc.is_none(), "no free text, no valid tags -> no DocBlock");
    }

    #[test]
    fn kind_tag_inapplicable_under_callable_policy() {
        let (doc, issues) = parse_lines(&lines(&["A knot.", "@kind query"]), DocPolicy::CALLABLE);
        let doc = doc.expect("doc present");
        assert_eq!(issues.inapplicable.len(), 1);
        assert!(doc.kind.is_none());
        assert_eq!(doc.doc.as_deref(), Some("A knot."));
    }

    #[test]
    fn param_inapplicable_under_value_policy() {
        let (doc, issues) = parse_lines(
            &lines(&["Player health.", "@param x {int}"]),
            DocPolicy::VALUE,
        );
        let doc = doc.expect("doc present");
        assert_eq!(issues.inapplicable.len(), 1);
        assert!(doc.params.is_empty());
        assert_eq!(doc.doc.as_deref(), Some("Player health."));
    }

    #[test]
    fn unknown_and_widget_tags_are_ignored_leniently() {
        let (doc, issues) = parse_lines(
            &lines(&[
                "@widget color_picker",
                "@nonsense whatever",
                "@param c {color}",
            ]),
            DocPolicy::EXTERNAL,
        );
        let doc = doc.expect("doc present");
        assert!(issues.malformed.is_empty());
        assert_eq!(
            doc.params,
            vec![("c".to_string(), TypeRef("color".to_string()))]
        );
    }
}
