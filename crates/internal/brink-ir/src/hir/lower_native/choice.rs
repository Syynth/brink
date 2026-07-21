//! `{? … }` choice points → `ChoiceSet`/`Choice` (`docs/b0-sequencing.md`
//! §B0.7, charter §5).
//!
//! Splice placement (`<- flow(args)` → `ThreadStart`) mirrors old ink's own
//! weave-fold treatment of a bare statement encountered mid-weave
//! (`lower/block/weave.rs::fold_weave_at_depth`'s `WeaveItem::Stmt` arm):
//! a splice reached **before** any choice line in the point becomes a plain
//! sibling statement immediately preceding the resulting `ChoiceSet`; a
//! splice reached **after** a choice line is appended to that choice's own
//! body (interspersed content "belongs to the previous choice", same
//! citation). This is "the same HIR machinery" charter §5 promises, not a
//! new placement rule invented for native.

use brink_syntax_native::SyntaxKind as N;
use brink_syntax_native::SyntaxNode;
use brink_syntax_native::ast::{self, AstNode as _};

use crate::hir::FileId;
use crate::provenance::NodeClass;
use crate::{
    Block, Choice, ChoiceSet, ChoiceSetContext, Content, ContentPart, Diagnostic, DiagnosticCode,
    DivertPath, Expr, Stmt, ThreadStart,
};

use super::body::{lower_divert_like, lower_items, push_text};
use super::cond::{lower_alternation, lower_conditional};
use super::expr::lower_path;
use super::provenance::native_provenance;

fn diag(file: FileId, range: rowan::TextRange, code: DiagnosticCode) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: code.title().to_string(),
        code,
    }
}

fn name_from(tok: Option<brink_syntax_native::SyntaxToken>) -> Option<crate::Name> {
    tok.map(|t| crate::Name {
        text: t.text().to_string(),
        range: t.text_range(),
    })
}

/// Lower a `{? … }` choice point into its statement(s): any leading splices
/// (as plain `ThreadStart` siblings), followed by the `ChoiceSet` itself.
/// `continuation` is built by the caller (`body::lower_continuation`) from
/// whatever follows the closed `{?}` block — the dissolved gather.
pub(super) fn lower_choice_point(
    file_id: FileId,
    cp: &ast::ChoicePoint,
    continuation: Block,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Stmt> {
    let mut preamble: Vec<Stmt> = Vec::new();
    let mut choices: Vec<Choice> = Vec::new();

    for child in cp.syntax().children() {
        match child.kind() {
            N::CHOICE => {
                if let Some(c) = ast::Choice::cast(child) {
                    choices.push(lower_choice(file_id, &c, diags));
                }
            }
            N::ELSE_BRANCH => {
                if let Some(eb) = ast::ElseBranch::cast(child) {
                    choices.push(lower_fallback_choice(file_id, &eb, diags));
                }
            }
            N::SPLICE => {
                if let Some(sp) = ast::Splice::cast(child)
                    && let Some(ts) = lower_splice(file_id, &sp, diags)
                {
                    if let Some(last) = choices.last_mut() {
                        last.body.stmts.push(Stmt::ThreadStart(ts));
                        // The splice is now the choice body's final
                        // statement (docs/block-effect-model.md §10 row j)
                        // — re-derive `tail`.
                        last.body.recompute_tail();
                    } else {
                        preamble.push(Stmt::ThreadStart(ts));
                    }
                }
            }
            _ => {}
        }
    }

    let mut out = preamble;
    out.push(Stmt::ChoiceSet(Box::new(ChoiceSet {
        choices,
        continuation,
        // D4 posture (`docs/hir-admission-contract.md` §3 D4,
        // `docs/b0-sequencing.md` §B0.7): native has no weave fold to
        // report depth/context from, so every native choice set stamps the
        // documented-neutral values uniformly, not only "inline" ones.
        context: ChoiceSetContext::Inline,
        depth: 0,
        gather_id: None,
    })));
    out
}

fn lower_choice(file_id: FileId, c: &ast::Choice, diags: &mut Vec<Diagnostic>) -> Choice {
    let is_sticky = c.is_sticky();
    let label = c.label().and_then(|l| name_from(l.name_token()));
    let condition = c
        .guard()
        .and_then(|g| g.expr())
        .map(|e| super::expr::lower_expr(file_id, &e, diags));

    let (start_content, start_divert) = lower_choice_region(
        file_id,
        c.start_content().map(|n| n.syntax().clone()),
        diags,
    );
    let (bracket_content, bracket_divert) = lower_choice_region(
        file_id,
        c.bracket_content().map(|n| n.syntax().clone()),
        diags,
    );
    let (inner_content, inner_divert) = lower_choice_region(
        file_id,
        c.inner_content().map(|n| n.syntax().clone()),
        diags,
    );
    // At most one region carries a divert in well-formed input (a choice
    // line ends once it diverts). If more than one somehow does, keep the
    // first in text order and flag the rest with E129 — silently dropping a
    // real divert is a bug (silent-drop rule), the same way a second divert
    // *within* a region is flagged above, even for a shape the grammar makes
    // hard to produce.
    let region_diverts = [start_divert, bracket_divert, inner_divert];
    if region_diverts.iter().filter(|d| d.is_some()).count() > 1 {
        diags.push(diag(file_id, c.syntax().text_range(), DiagnosticCode::E129));
    }
    let divert = region_diverts.into_iter().flatten().next();

    let mut stmts = Vec::new();
    if let Some(d) = divert {
        stmts.push(d);
    }
    // The list-display/echoed-text boundary marker — matches old ink's
    // choice body preamble (`lower/choice.rs::lower_choice`: `preamble =
    // [divert?, EndOfLine, ...body]`), preserved for runtime output parity
    // (charter: choice-line anatomy "kept as-is").
    stmts.push(Stmt::EndOfLine);
    if let Some(body) = c.body() {
        let items: Vec<SyntaxNode> = body.items().collect();
        stmts.extend(lower_items(file_id, &items, 0, diags));
    }

    Choice {
        ptr: native_provenance(file_id, NodeClass::Choice, c.syntax()),
        is_sticky,
        is_fallback: false,
        label,
        condition,
        start_content,
        bracket_content,
        inner_content,
        // Choice-line trailing tags have no parseable grammar slot today:
        // `choice.rs::choice` never calls `tag_line_tail` after
        // `choice_text`, so a trailing `#tag` on a choice line falls
        // through to the choice point's own `error_recover` instead of
        // attaching here (a B0.5 grammar gap, not a B0.7 lowering drop —
        // flagged for the coordinator, not fixed in this slice).
        tags: Vec::new(),
        body: {
            let tail = crate::tail_from_stmts(&stmts);
            Block {
                label: None,
                stmts,
                container_id: None,
                tail,
            }
        },
        container_id: None,
    }
}

/// `else { … }` → a synthetic fallback `Choice` (`is_fallback: true`) — the
/// HIR's only slot for "the point's else arm" (contract: "`is_fallback` ⟺
/// the point's else arm"). Unlike ink, native's fallback has no bullet, so
/// `is_sticky` is meaningless here and defaults `false`.
fn lower_fallback_choice(
    file_id: FileId,
    eb: &ast::ElseBranch,
    diags: &mut Vec<Diagnostic>,
) -> Choice {
    let mut stmts = vec![Stmt::EndOfLine];
    if let Some(body) = eb.choice_body() {
        let items: Vec<SyntaxNode> = body.items().collect();
        stmts.extend(lower_items(file_id, &items, 0, diags));
    }
    Choice {
        ptr: native_provenance(file_id, NodeClass::Choice, eb.syntax()),
        is_sticky: false,
        is_fallback: true,
        label: None,
        condition: None,
        start_content: None,
        bracket_content: None,
        inner_content: None,
        tags: Vec::new(),
        body: {
            let tail = crate::tail_from_stmts(&stmts);
            Block {
                label: None,
                stmts,
                container_id: None,
                tail,
            }
        },
        container_id: None,
    }
}

/// Lower one of a choice's `text[bracket]inner` content regions
/// (`CHOICE_START_CONTENT`/`CHOICE_BRACKET_CONTENT`/`CHOICE_INNER_CONTENT`)
/// into `(content, extracted divert)`. `HIR::Content` has no slot for a
/// divert, so a `DIVERT_STMT`/`TUNNEL_CALL` nested here (N-1: content
/// scanning dispatches diverts uniformly, including inside choice-text
/// regions) is pulled out and returned separately — folded into the
/// choice's body preamble by [`lower_choice`], mirroring old ink's own
/// `self.divert()` accessor being a sibling of the content regions, never
/// part of them.
fn lower_choice_region(
    file_id: FileId,
    node: Option<SyntaxNode>,
    diags: &mut Vec<Diagnostic>,
) -> (Option<Content>, Option<Stmt>) {
    let Some(node) = node else {
        return (None, None);
    };
    let mut parts = Vec::new();
    let mut divert = None;
    for child in node.children() {
        match child.kind() {
            N::TEXT => push_text(&mut parts, &child),
            N::INTERPOLATION => {
                parts.push(super::body::lower_interpolation(file_id, &child, diags));
            }
            N::GLUE_NODE => parts.push(ContentPart::Glue),
            N::DIVERT_STMT | N::TUNNEL_CALL => {
                if divert.is_none() {
                    divert = lower_divert_like(file_id, &child, diags);
                } else {
                    diags.push(diag(file_id, child.text_range(), DiagnosticCode::E129));
                }
            }
            N::CONDITIONAL_BLOCK => {
                if let Some(cb) = ast::ConditionalBlock::cast(child) {
                    parts.push(ContentPart::InlineConditional(lower_conditional(
                        file_id, &cb, diags,
                    )));
                }
            }
            N::ALTERNATION_BLOCK => {
                if let Some(ab) = ast::AlternationBlock::cast(child) {
                    parts.push(ContentPart::InlineSequence(lower_alternation(
                        file_id, &ab, diags, false,
                    )));
                }
            }
            N::ERROR => {}
            // A nested `{?}` point inside choice-list text has no HIR shape
            // to hold it (`Content`/`ContentPart` cannot carry a
            // `ChoiceSet`) — loud, not a silent drop; a genuinely
            // pathological shape the surface doesn't seem intended to
            // support (a choice's own display text spawning another choice
            // point inline).
            _ => diags.push(diag(file_id, child.text_range(), DiagnosticCode::E129)),
        }
    }
    (
        Some(Content {
            ptr: None,
            parts,
            tags: Vec::new(),
        }),
        divert,
    )
}

fn lower_splice(
    file_id: FileId,
    sp: &ast::Splice,
    diags: &mut Vec<Diagnostic>,
) -> Option<ThreadStart> {
    let Some(p) = sp.path() else {
        diags.push(diag(
            file_id,
            sp.syntax().text_range(),
            DiagnosticCode::E012,
        ));
        return None;
    };
    let path = lower_path(&p);
    let args: Vec<Expr> = sp
        .arg_list()
        .into_iter()
        .flat_map(|al| al.syntax().children().collect::<Vec<_>>())
        .map(|n| super::expr::lower_expr(file_id, &n, diags))
        .collect();
    Some(ThreadStart {
        ptr: native_provenance(file_id, NodeClass::ThreadStart, sp.syntax()),
        target: crate::DivertTarget {
            path: DivertPath::Path(path),
            args,
        },
    })
}
