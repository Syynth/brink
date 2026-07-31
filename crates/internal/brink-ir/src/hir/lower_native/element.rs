//! Natural-notation element dispatch (issue #1838) — the slice that makes
//! the prose grammar *mean* something.
//!
//! `docs/decision-log.md`'s 2026-07-31 ruling ("Conventions are annotated
//! handlers: the declarative element surface is subsumed by the annotation
//! surface") collapsed two element mechanisms into one: **a preset element
//! is literally an annotated handler.** A scene heading is a matched line,
//! captures bound to params by name, and *exactly one call* — the same
//! three steps `!radio` takes, minus the sigil. The `lower:` column, the
//! `Conventions` type and the chain-rule engine are dissolved by that same
//! ruling and are deliberately absent here.
//!
//! ```brink
//! @[element(claims = "^INT\\. (?<place>.+)$")]
//! fn interior(place) { return "— inside " + place + " —"; }
//!
//! flow main() {
//!   INT. MARKET SQUARE
//! }
//! ```
//!
//! The heading line no longer reaches `body::lower_one_item`'s loud-`E129`
//! arm: it is claimed, `place` binds to `MARKET SQUARE`, and the line
//! lowers to one call whose value *is* the line.
//!
//! # No invisible expansion
//!
//! Every claimed line is recorded as a [`crate::ElementMatch`] on the
//! `HirFile` — the claimed range, the prose shape it was written as, the
//! handler's name **and the annotation's own source range**, and each
//! capture as a span. The ruling is explicit that a rewritten line must
//! point at real source; that record is how the `LineContext`/IDE query
//! family answers "what happened to this line, and where is the code that
//! did it" without re-running the match.
//!
//! # What claims, and what does not
//!
//! - A pattern is a *claim* only when spelled `claims = "…"`. The
//!   `args = "…"` form declares the `!name`-dispatched handler, whose
//!   sigil rewrite is still unimplemented (`docs/prose-dialect-spec.md`
//!   §3.5b Deferred).
//! - Only a **top-level `fn`** may claim: the rewrite is an expression
//!   call, and a `flow` is not callable as one. A `claims` annotation
//!   anywhere else is `E112` (misplaced), enforced by
//!   [`super::annotation::handle_line`]'s placement rule.
//! - Only a **wholly literal** prose line is a candidate — one with no
//!   interpolation, glue, markup, tags, label or embedded divert. A line
//!   carrying dynamic parts has no fixed text for a pattern to match, and
//!   capture spans over it would not point at anything real.
//! - A claiming handler's **own body is not claimable** (the staging rule
//!   §3.5 states for the conventions module: it cannot use the conventions
//!   it defines). Without this, a handler whose body repeats the shape it
//!   claims would rewrite into a call on itself.
//!
//! # Deliberately not here
//!
//! Block capture (the `block` param form) and `fn conventions()`
//! registration + comptime evaluation are the ruling's other two build
//! slices, filed separately (issues #1839/#1840). The confinement of
//! claiming to the `brink.toml`-named conventions module needs project
//! identity that single-file lowering does not have; see the issue thread.

use brink_syntax_native::SyntaxKind as N;
use brink_syntax_native::ast::{self, AstNode as _};
use rowan::{TextRange, TextSize};

use crate::hir::FileId;
use crate::{
    Content, ContentPart, Diagnostic, ElementCapture, ElementDisposition, ElementKind,
    ElementMatch, Expr, Name, Path, Stmt, StringExpr, StringPart,
};

use super::SyntaxNode;
use super::provenance::native_provenance;
use crate::provenance::NodeClass;

/// One declared natural-notation handler: a top-level `fn` whose
/// `@[element(claims = "…")]` pattern claims prose lines.
struct ClaimHandler {
    /// The handler's own name, carrying its declaration-site range.
    name: Name,
    /// Parameter names in declaration order — the argument order the
    /// rewritten call uses. Guaranteed by `E160`/`E166` to be exactly the
    /// pattern's named-capture set.
    params: Vec<String>,
    /// The compiled claiming pattern.
    pattern: regex::Regex,
    /// Range of the `@[element(claims = "…")]` line itself.
    annotation: TextRange,
    /// The handler declaration's own range — used to suppress claiming
    /// inside the handler's own body (the staging rule).
    decl: TextRange,
}

/// The dispatcher threaded through body lowering: the file's claiming
/// handlers plus the per-line classification records they produce.
///
/// Built once per file by [`collect`] and passed down by reference, rather
/// than re-derived per line: a per-line whole-tree scan would make body
/// lowering quadratic in file size.
pub(super) struct Elements {
    handlers: Vec<ClaimHandler>,
    /// Every claimed line, in the order lowering reached it.
    pub(super) matches: Vec<ElementMatch>,
}

impl Elements {
    /// `true` when no handler in this file claims anything, so callers can
    /// skip candidate testing entirely on the overwhelmingly common path.
    fn is_inert(&self) -> bool {
        self.handlers.is_empty()
    }
}

/// Collect every claiming handler declared in `root`.
///
/// Diagnostic-free by construction: each `@[element(…)]` line is parsed and
/// validated exactly once, by `container.rs`, when the annotated
/// declaration is lowered. This re-reads the same lines into a dispatch
/// table and drops anything that did not validate — reporting here as well
/// would double every `E159`/`E160`/`E166`.
pub(super) fn collect(file_id: FileId, root: &SyntaxNode) -> Elements {
    let mut handlers = Vec::new();
    for node in root.children() {
        // Top-level `fn` only — see the module doc. `handle_line`'s
        // placement rule reports every other position as `E112`, so a
        // claim this loop skips is never silently dropped.
        if node.kind() != N::FN_DECL {
            continue;
        }
        let Some(decl) = ast::FnDecl::cast(node.clone()) else {
            continue;
        };
        let Some(name) = super::container::name_from(decl.name_token()) else {
            continue;
        };
        let params = super::container::lower_params(decl.param_list());
        let mut scratch: Vec<Diagnostic> = Vec::new();
        let Some(element) =
            super::annotation::element_annotation(file_id, &node, &params, &mut scratch)
        else {
            continue;
        };
        if !element.claims {
            continue;
        }
        let Ok(pattern) = regex::Regex::new(&element.pattern) else {
            continue;
        };
        handlers.push(ClaimHandler {
            name,
            params: params.into_iter().map(|p| p.name.text).collect(),
            pattern,
            annotation: element.range,
            decl: node.text_range(),
        });
    }
    Elements {
        handlers,
        matches: Vec::new(),
    }
}

/// Try to claim one body item, returning the statements that replace it.
///
/// `None` means "nothing claimed this" and the caller lowers the item the
/// way it always did — the fall-through that keeps every unclaimed line,
/// and every file with no claiming handler, byte-identical.
pub(super) fn try_claim(
    file_id: FileId,
    node: &SyntaxNode,
    elements: &mut Elements,
) -> Option<Vec<Stmt>> {
    if elements.is_inert() {
        return None;
    }
    let (kind, text_node) = candidate(node)?;
    let text = text_node.text().to_string();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Where `trimmed` starts inside `text_node`, so capture offsets land on
    // real source bytes rather than on the untrimmed run's start.
    let lead = u32::try_from(text.len() - text.trim_start().len()).unwrap_or(0);
    let base = text_node.text_range().start() + TextSize::from(lead);

    let claimed = node.text_range();
    let handler = elements
        .handlers
        .iter()
        .find(|h| !h.decl.contains_range(claimed) && h.pattern.is_match(trimmed))?;

    let caps = handler.pattern.captures(trimmed)?;
    let mut captures = Vec::with_capacity(handler.params.len());
    for param in &handler.params {
        // `E160`/`E166` already pinned params ≡ named captures at the
        // declaration, so a miss here means the group did not participate
        // in this particular match (an alternation branch). Declining the
        // claim is the honest answer: a call with a missing argument is
        // not "exactly one call", it is a broken one.
        let m = caps.name(param)?;
        captures.push(ElementCapture {
            name: param.clone(),
            text: m.as_str().to_string(),
            range: TextRange::new(
                base + TextSize::from(u32::try_from(m.start()).ok()?),
                base + TextSize::from(u32::try_from(m.end()).ok()?),
            ),
        });
    }

    let call = Expr::Call(
        Path {
            segments: vec![Name {
                text: handler.name.text.clone(),
                // The call is written at the claimed line, not at the
                // handler's declaration — the range a reader clicking the
                // rewritten call should land on.
                range: claimed,
            }],
            range: claimed,
        },
        captures
            .iter()
            .map(|c| {
                Expr::String(StringExpr {
                    parts: vec![StringPart::Literal(c.text.clone())],
                })
            })
            .collect(),
    );

    elements.matches.push(ElementMatch {
        line: claimed,
        kind,
        handler: handler.name.clone(),
        annotation: handler.annotation,
        captures,
        disposition: ElementDisposition::Call,
    });

    Some(vec![
        Stmt::Content(Content {
            ptr: Some(native_provenance(file_id, NodeClass::Content, node)),
            parts: vec![ContentPart::Interpolation(call)],
            tags: Vec::new(),
        }),
        Stmt::EndOfLine,
    ])
}

/// Classify a body item as a claim candidate, yielding the node whose text
/// a pattern is matched against.
///
/// A `CONTENT_LINE` qualifies only when it is *wholly* literal — exactly
/// one `TEXT` child and nothing else (no `LABEL`, `INTERPOLATION`, `SPAN`,
/// `TAG`, `GLUE_NODE`, `ESCAPE`, embedded divert or choice point). A
/// `SCENE_HEADING`'s title run qualifies the same way; the heading's
/// optional `[slug]` and trailing tags are structure the pattern is not
/// shown, so a heading carrying either is declined rather than matched
/// against a partial line.
fn candidate(node: &SyntaxNode) -> Option<(ElementKind, SyntaxNode)> {
    match node.kind() {
        N::CONTENT_LINE => {
            let mut children = node.children();
            let first = children.next()?;
            (first.kind() == N::TEXT && children.next().is_none())
                .then_some((ElementKind::ContentLine, first))
        }
        N::SCENE_HEADING => {
            let mut children = node.children();
            let first = children.next()?;
            (first.kind() == N::SCENE_TITLE && children.next().is_none())
                .then_some((ElementKind::SceneHeading, first))
        }
        _ => None,
    }
}
