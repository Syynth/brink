//! Prose-dialect body lowering: content lines, tags, glue, `{expr}`
//! interpolation, diverts/tunnels/return, and the label-absorption
//! algorithm that dissolves both content-line labels (G-1) and choice-point
//! gathers into `Stmt::LabeledBlock`/`ChoiceSet.continuation`
//! (`docs/b0-sequencing.md` §B0.7).
//!
//! # The dissolved gather, mechanically
//!
//! Native has no gather-dash token; charter §5 says "after the choices
//! rejoin is simply the next line after the block." [`lower_items`]
//! implements this literally: when it meets a `{?}` choice point, it does
//! **not** keep iterating siblings — it recursively lowers everything that
//! follows in the same item stream as the choice set's own
//! `continuation: Block` (via [`lower_continuation`]), then returns. This
//! is exactly old ink's own weave-fold behavior once a gather is reached
//! (`lower/block/weave.rs::flush_choices`: "Gather after choices ... fold
//! them recursively, and nest everything into the continuation") — native
//! just never needs the depth-matching machinery that surrounds it there,
//! because a `{?}` block's extent is never ambiguous.
//!
//! The same absorption shape handles G-1 labeled content lines: a `(name)`
//! label is not itself a gather, but ink's own "standalone labeled gather"
//! is the same concept applied to a labeled *content line* with no
//! preceding choice block — see `weave.rs`'s
//! `last_standalone_label`/`gather_stmts_start` retroactive
//! `Stmt::LabeledBlock` wrap, which [`lower_items`] mirrors directly. One
//! refinement, also lifted from old ink: a label immediately following a
//! closed `{?}` block attaches to `continuation.label` directly rather than
//! wrapping in a nested `LabeledBlock` (`weave.rs`'s `WeaveItem::Continuation`
//! handling, built from `lower_gather_to_block`'s `label: gather.label()`)
//! — see [`lower_continuation`].

use brink_syntax_native::SyntaxKind as N;
use brink_syntax_native::SyntaxNode;
use brink_syntax_native::ast::{self, AstNode as _};

use crate::Provenance;
use crate::hir::FileId;
use crate::provenance::NodeClass;
use crate::{
    Block, Content, ContentPart, Diagnostic, DiagnosticCode, Divert, DivertPath, DivertTarget,
    Expr, Name, Return, ReturnKind, Stmt, Tag, TunnelCall,
};

use super::choice::lower_choice_point;
use super::cond::{lower_alternation, lower_conditional};
use super::expr::lower_expr;
use super::provenance::native_provenance;

fn diag(file: FileId, range: rowan::TextRange, code: DiagnosticCode) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: code.title().to_string(),
        code,
    }
}

fn name_from(tok: Option<brink_syntax_native::SyntaxToken>) -> Option<Name> {
    tok.map(|t| Name {
        text: t.text().to_string(),
        range: t.text_range(),
    })
}

/// Lower a `flow`/`fn`/nested-`flow` (stitch) body — the B0.7 entry point
/// `container.rs` calls in place of B0.6's `Block::default()` stub.
pub(super) fn lower_block(
    file_id: FileId,
    block: &ast::Block,
    diags: &mut Vec<Diagnostic>,
) -> Block {
    let items: Vec<SyntaxNode> = block.items().collect();
    Block {
        label: None,
        stmts: lower_items(file_id, &items, 0, diags),
        container_id: None,
    }
}

/// The shared item-stream lowering algorithm: dispatches each item, but a
/// labeled content line or a `{?}` choice point **absorbs every item after
/// it** (the dissolved-gather / G-1 mechanism — see the module doc) rather
/// than being folded as an ordinary sibling. Used for every body-shaped
/// item list: knot/fn/stitch bodies, choice bodies, conditional/alternation
/// arm bodies.
pub(super) fn lower_items(
    file_id: FileId,
    items: &[SyntaxNode],
    start: usize,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Stmt> {
    let mut stmts = Vec::new();
    let mut i = start;
    while i < items.len() {
        let node = &items[i];

        if node.kind() == N::CONTENT_LINE
            && let Some(cl) = ast::ContentLine::cast(node.clone())
            && let Some(label) = cl.label().and_then(|l| name_from(l.name_token()))
        {
            let mut inner = lower_content_line_body(file_id, &cl, diags);
            inner.extend(lower_items(file_id, items, i + 1, diags));
            stmts.push(Stmt::LabeledBlock(Box::new(Block {
                label: Some(label),
                stmts: inner,
                container_id: None,
            })));
            return stmts;
        }

        if node.kind() == N::CHOICE_POINT {
            if let Some(cp) = ast::ChoicePoint::cast(node.clone()) {
                let continuation = lower_continuation(file_id, items, i + 1, diags);
                stmts.extend(lower_choice_point(file_id, &cp, continuation, diags));
            }
            return stmts;
        }

        stmts.extend(lower_one_item(file_id, node, diags));
        i += 1;
    }
    stmts
}

/// Build a `{?}` choice point's continuation `Block` from whatever follows
/// it in the item stream. If the very next item is a labeled content line,
/// its label attaches directly to `continuation.label` (the gather-label
/// convention — see the module doc) instead of nesting a `LabeledBlock`
/// one level in.
fn lower_continuation(
    file_id: FileId,
    items: &[SyntaxNode],
    start: usize,
    diags: &mut Vec<Diagnostic>,
) -> Block {
    if let Some(node) = items.get(start)
        && node.kind() == N::CONTENT_LINE
        && let Some(cl) = ast::ContentLine::cast(node.clone())
        && let Some(label) = cl.label().and_then(|l| name_from(l.name_token()))
    {
        let mut stmts = lower_content_line_body(file_id, &cl, diags);
        stmts.extend(lower_items(file_id, items, start + 1, diags));
        return Block {
            label: Some(label),
            stmts,
            container_id: None,
        };
    }
    Block {
        label: None,
        stmts: lower_items(file_id, items, start, diags),
        container_id: None,
    }
}

/// Dispatch a single body item that is neither a labeled content line nor a
/// choice point (both handled by [`lower_items`] itself, since they can
/// absorb the rest of the stream).
fn lower_one_item(file_id: FileId, node: &SyntaxNode, diags: &mut Vec<Diagnostic>) -> Vec<Stmt> {
    match node.kind() {
        N::CONTENT_LINE => {
            let Some(cl) = ast::ContentLine::cast(node.clone()) else {
                return Vec::new();
            };
            lower_content_line_body(file_id, &cl, diags)
        }
        N::TAG_LINE => {
            let Some(tl) = ast::TagLine::cast(node.clone()) else {
                return Vec::new();
            };
            let tags: Vec<Tag> = tl.tags().map(|t| lower_tag(file_id, &t)).collect();
            if tags.is_empty() {
                Vec::new()
            } else {
                vec![
                    Stmt::Content(Content {
                        ptr: None,
                        parts: Vec::new(),
                        tags,
                    }),
                    Stmt::EndOfLine,
                ]
            }
        }
        N::DIVERT_STMT | N::TUNNEL_CALL => lower_divert_like(file_id, node, diags)
            .into_iter()
            .collect(),
        N::RETURN_STMT => vec![Stmt::Return(Return {
            ptr: Some(native_provenance(file_id, NodeClass::Return, node)),
            kind: ReturnKind::Explicit,
            value: None,
            onwards_args: Vec::new(),
        })],
        N::RETURN_REDIRECT => lower_return_redirect(file_id, node, diags),
        N::CONDITIONAL_BLOCK => {
            let Some(cb) = ast::ConditionalBlock::cast(node.clone()) else {
                return Vec::new();
            };
            vec![Stmt::Conditional(lower_conditional(file_id, &cb, diags))]
        }
        N::ALTERNATION_BLOCK => {
            let Some(ab) = ast::AlternationBlock::cast(node.clone()) else {
                return Vec::new();
            };
            vec![Stmt::Sequence(lower_alternation(file_id, &ab, diags, true))]
        }
        // Declarations reachable at body position are handled by other
        // passes: `flow`/`fn` become stitches (`container.rs`), `var`/
        // `const`/`flags` are hoisted flat by `lower_native::lower`'s
        // whole-tree walk, and `struct`/`extern`/`use`/`import`/`module`
        // nested here are already diagnosed E129 by that same function's
        // out-of-position pass. Re-emitting statements or diagnostics for
        // any of them here would double up, not fill a gap.
        N::FLOW_DECL
        | N::FN_DECL
        | N::VAR_DECL
        | N::CONST_DECL
        | N::FLAGS_DECL
        | N::STRUCT_DECL
        | N::EXTERN_DECL
        | N::USE_DECL
        | N::IMPORT_DECL
        | N::MODULE_DECL
        // Already diagnosed by the parser itself.
        | N::ERROR => Vec::new(),
        // `@[…]` annotations at body position: no directive channel is
        // wired yet (B0.6's judgment call #5, still open) — loud, not
        // dropped.
        _ => {
            diags.push(diag(file_id, node.text_range(), DiagnosticCode::E129));
            Vec::new()
        }
    }
}

/// Lower one `CONTENT_LINE`'s own content (its `LABEL` child, if any, is
/// skipped here — [`lower_items`]/[`lower_continuation`] already consumed
/// it for the absorption decision before calling this).
fn lower_content_line_body(
    file_id: FileId,
    cl: &ast::ContentLine,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Stmt> {
    let line_prov = native_provenance(file_id, NodeClass::Content, cl.syntax());
    let children: Vec<SyntaxNode> = cl
        .syntax()
        .children()
        .filter(|n| n.kind() != N::LABEL)
        .collect();
    lower_content_run(file_id, &children, Some(line_prov), diags, true)
}

/// The shared "run of content-shaped items" lowering engine — used for a
/// content line's own body-item lowering and for alternation branches
/// (`cond.rs`). Handles inline `{expr}` interpolation, `<>` glue, embedded
/// diverts/tunnels (N-1: a `->` mid-run is a real node, not swallowed as
/// text), inline conditional/alternation (`ContentPart::InlineConditional`/
/// `InlineSequence`), and — uniquely among the content-lowering helpers —
/// an embedded `{?}` choice point, which absorbs the remainder of `items`
/// as its continuation exactly like [`lower_items`] does at body-item
/// granularity (the same dissolved-gather mechanism, one level down).
///
/// `line_prov` becomes the `ptr` of the (possibly only) `Content` statement
/// this run's trailing flush produces; interior flushes (before an embedded
/// divert/choice-point) always carry `ptr: None`, matching old ink's own
/// accumulator convention (`content/accumulator.rs::flush` uses `ptr: None`
/// for every flush except the line's own top-level one).
///
/// `trailing_eol`: whether the run's *final* flush may append
/// `Stmt::EndOfLine` (when the content doesn't end with glue). `true` for a
/// genuine content line (and for a `{?}` continuation, which behaves like
/// ordinary subsequent lines). `false` for a synthesized fragment that
/// isn't a whole line — an inline alternation branch (`cond.rs`'s
/// `finish_inline_branch`): `{& a cat|a dog}.` must not force a line break
/// after "a cat" before the trailing "." resolves on the same line.
pub(super) fn lower_content_run(
    file_id: FileId,
    items: &[SyntaxNode],
    line_prov: Option<Provenance>,
    diags: &mut Vec<Diagnostic>,
    trailing_eol: bool,
) -> Vec<Stmt> {
    let mut out = Vec::new();
    let mut parts: Vec<ContentPart> = Vec::new();
    let mut tags: Vec<Tag> = Vec::new();
    let mut i = 0;

    while i < items.len() {
        let node = &items[i];
        match node.kind() {
            N::TEXT => {
                push_text(&mut parts, node);
                i += 1;
            }
            N::INTERPOLATION => {
                parts.push(lower_interpolation(file_id, node, diags));
                i += 1;
            }
            N::GLUE_NODE => {
                parts.push(ContentPart::Glue);
                i += 1;
            }
            N::TAG => {
                if let Some(t) = ast::Tag::cast(node.clone()) {
                    tags.push(lower_tag(file_id, &t));
                }
                i += 1;
            }
            N::DIVERT_STMT | N::TUNNEL_CALL => {
                flush_content(&mut parts, &mut tags, &mut out, None, false);
                out.extend(lower_divert_like(file_id, node, diags));
                i += 1;
            }
            N::CHOICE_POINT => {
                flush_content(&mut parts, &mut tags, &mut out, None, false);
                if let Some(cp) = ast::ChoicePoint::cast(node.clone()) {
                    let continuation = Block {
                        label: None,
                        stmts: lower_content_run(file_id, &items[i + 1..], line_prov, diags, true),
                        container_id: None,
                    };
                    out.extend(lower_choice_point(file_id, &cp, continuation, diags));
                }
                return out;
            }
            N::CONDITIONAL_BLOCK => {
                if let Some(cb) = ast::ConditionalBlock::cast(node.clone()) {
                    parts.push(ContentPart::InlineConditional(lower_conditional(
                        file_id, &cb, diags,
                    )));
                }
                i += 1;
            }
            N::ALTERNATION_BLOCK => {
                if let Some(ab) = ast::AlternationBlock::cast(node.clone()) {
                    parts.push(ContentPart::InlineSequence(lower_alternation(
                        file_id, &ab, diags, false,
                    )));
                }
                i += 1;
            }
            N::ERROR => {
                i += 1;
            }
            _ => {
                diags.push(diag(file_id, node.text_range(), DiagnosticCode::E129));
                i += 1;
            }
        }
    }

    flush_content(&mut parts, &mut tags, &mut out, line_prov, trailing_eol);
    out
}

fn flush_content(
    parts: &mut Vec<ContentPart>,
    tags: &mut Vec<Tag>,
    out: &mut Vec<Stmt>,
    ptr: Option<Provenance>,
    allow_eol: bool,
) {
    if parts.is_empty() && tags.is_empty() {
        return;
    }
    let ends_glue = matches!(parts.last(), Some(ContentPart::Glue));
    out.push(Stmt::Content(Content {
        ptr,
        parts: std::mem::take(parts),
        tags: std::mem::take(tags),
    }));
    if allow_eol && !ends_glue {
        out.push(Stmt::EndOfLine);
    }
}

pub(super) fn push_text(parts: &mut Vec<ContentPart>, node: &SyntaxNode) {
    let text = node.text().to_string();
    if !text.is_empty() {
        parts.push(ContentPart::Text(text));
    }
}

pub(super) fn lower_interpolation(
    file_id: FileId,
    node: &SyntaxNode,
    diags: &mut Vec<Diagnostic>,
) -> ContentPart {
    if let Some(inner) = node.children().next() {
        ContentPart::Interpolation(lower_expr(file_id, &inner, diags))
    } else {
        diags.push(diag(file_id, node.text_range(), DiagnosticCode::E015));
        ContentPart::Interpolation(Expr::Null)
    }
}

fn lower_tag(file_id: FileId, t: &ast::Tag) -> Tag {
    let mut text = String::new();
    for tok in t
        .syntax()
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
    {
        if tok.kind() == N::HASH {
            continue;
        }
        text.push_str(tok.text());
    }
    let trimmed = text.trim().to_string();
    let parts = if trimmed.is_empty() {
        Vec::new()
    } else {
        vec![ContentPart::Text(trimmed)]
    };
    Tag {
        parts,
        ptr: native_provenance(file_id, NodeClass::Tag, t.syntax()),
    }
}

/// `DIVERT_STMT` → `Stmt::Divert`, `TUNNEL_CALL` → `Stmt::TunnelCall`.
pub(super) fn lower_divert_like(
    file_id: FileId,
    node: &SyntaxNode,
    diags: &mut Vec<Diagnostic>,
) -> Option<Stmt> {
    match node.kind() {
        N::DIVERT_STMT => {
            let target = ast::DivertStmt::cast(node.clone())
                .and_then(|d| d.target())
                .and_then(|t| lower_divert_target(file_id, &t, diags))?;
            Some(Stmt::Divert(Divert {
                ptr: Some(native_provenance(file_id, NodeClass::Divert, node)),
                target,
            }))
        }
        N::TUNNEL_CALL => {
            let target = ast::TunnelCall::cast(node.clone())
                .and_then(|t| t.target())
                .and_then(|t| lower_divert_target(file_id, &t, diags))?;
            Some(Stmt::TunnelCall(TunnelCall {
                ptr: native_provenance(file_id, NodeClass::TunnelCall, node),
                targets: vec![target],
            }))
        }
        _ => None,
    }
}

fn lower_divert_target(
    file_id: FileId,
    t: &ast::DivertTarget,
    diags: &mut Vec<Diagnostic>,
) -> Option<DivertTarget> {
    let path = if t.is_end() {
        DivertPath::End
    } else if t.is_done() {
        DivertPath::Done
    } else if let Some(p) = t.path() {
        DivertPath::Path(super::expr::lower_path(&p))
    } else {
        diags.push(diag(file_id, t.syntax().text_range(), DiagnosticCode::E012));
        return None;
    };
    // Native's `DIVERT_TARGET` grammar (`parser/divert.rs::divert_target`)
    // never parses an arg list — unlike ink's `-> knot(args)`, there is no
    // call-argument syntax on a native divert target yet (a grammar gap,
    // not a lowering drop: nothing to consume).
    Some(DivertTarget {
        path,
        args: Vec::new(),
    })
}

/// `return -> x` (charter §11's tunnel-return respelling, B0.2's payoff).
/// `END`/`DONE` targets lower as a plain `Stmt::Divert` — matching old
/// ink's own `->-> DONE`/`->-> END` treatment
/// (`lower/divert.rs::LowerDivert`) exactly, because `Expr::DivertTarget`
/// only carries a `Path` and cannot represent either sentinel. A named-path
/// target lowers as `Stmt::Return { kind: TunnelRedirect, .. }`.
fn lower_return_redirect(
    file_id: FileId,
    node: &SyntaxNode,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Stmt> {
    let Some(target) = ast::ReturnRedirect::cast(node.clone())
        .and_then(|r| r.target())
        .and_then(|t| lower_divert_target(file_id, &t, diags))
    else {
        diags.push(diag(file_id, node.text_range(), DiagnosticCode::E012));
        return Vec::new();
    };
    match target.path {
        DivertPath::Path(p) => vec![Stmt::Return(Return {
            ptr: Some(native_provenance(file_id, NodeClass::Return, node)),
            kind: ReturnKind::TunnelRedirect,
            value: Some(Expr::DivertTarget(p)),
            onwards_args: target.args,
        })],
        DivertPath::Done => vec![Stmt::Divert(Divert {
            ptr: Some(native_provenance(file_id, NodeClass::Divert, node)),
            target: DivertTarget {
                path: DivertPath::Done,
                args: Vec::new(),
            },
        })],
        DivertPath::End => vec![Stmt::Divert(Divert {
            ptr: Some(native_provenance(file_id, NodeClass::Divert, node)),
            target: DivertTarget {
                path: DivertPath::End,
                args: Vec::new(),
            },
        })],
    }
}
