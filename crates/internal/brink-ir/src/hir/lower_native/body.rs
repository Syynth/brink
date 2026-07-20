//! Prose-dialect body lowering, stage 1: content lines, tags, glue,
//! `{expr}` interpolation, diverts/tunnels/return, and the label-absorption
//! algorithm that dissolves G-1 content-line labels into
//! `Stmt::LabeledBlock` (`docs/b0-sequencing.md` §B0.7).
//!
//! **Scope note**: the annotated-brace family (`{if}`/`{match}`/`{~}`/
//! `{&}`/`{!}`/`{|}`, B0.7's second commit) and choice points (`{?}`, the
//! choice-set/dissolved-gather commit) are not wired yet — both fall
//! through to the generic "unrecognized construct" diagnostic (E129) below,
//! same as any other not-yet-lowered body-position construct. The item-
//! stream absorption mechanism this file introduces
//! ([`lower_items`]/[`lower_continuation`]) is written to serve both the
//! G-1 label case (wired now) and the `{?}` dissolved-gather case (wired in
//! the choice-set commit) — see that commit's changes to this file for the
//! `CHOICE_POINT` branch.
//!
//! # Label absorption, mechanically
//!
//! A `(name)` label is not itself a gather, but ink's own "standalone
//! labeled gather" is the same concept applied to a labeled *content line*
//! with no preceding choice block — see `lower/block/weave.rs`'s
//! `last_standalone_label`/`gather_stmts_start` retroactive
//! `Stmt::LabeledBlock` wrap, which [`lower_items`] mirrors directly:
//! everything from a labeled content line onward (through the rest of the
//! enclosing item stream) becomes that label's `LabeledBlock`, not a flat
//! sibling.

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
/// labeled content line **absorbs every item after it** (the G-1
/// mechanism — see the module doc) rather than being folded as an ordinary
/// sibling. Used for every body-shaped item list: knot/fn/stitch bodies,
/// and (from the choice-set commit onward) choice bodies and conditional/
/// alternation arm bodies.
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

        stmts.extend(lower_one_item(file_id, node, diags));
        i += 1;
    }
    stmts
}

/// Dispatch a single body item that is not a labeled content line (handled
/// by [`lower_items`] itself, since it absorbs the rest of the stream).
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
        // `{?}` choice points and the annotated-brace family
        // (`{if}`/`{match}`/`{~}`/`{&}`/`{!}`/`{|}`) land here until their
        // own commits wire real lowering — loud (E129), never silently
        // dropped. `@[…]` annotations at body position are the same story
        // permanently: no directive channel is wired yet (B0.6's judgment
        // call #5, still open).
        _ => {
            diags.push(diag(file_id, node.text_range(), DiagnosticCode::E129));
            Vec::new()
        }
    }
}

/// Lower one `CONTENT_LINE`'s own content (its `LABEL` child, if any, is
/// skipped here — [`lower_items`] already consumed it for the absorption
/// decision before calling this).
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
/// content line's own body-item lowering (and, from the choice-set/
/// alternation commits onward, choice text regions and alternation
/// branches). Handles inline `{expr}` interpolation, `<>` glue, and
/// embedded diverts/tunnels (N-1: a `->` mid-run is a real node, not
/// swallowed as text).
///
/// `line_prov` becomes the `ptr` of the (possibly only) `Content` statement
/// this run's trailing flush produces; interior flushes (before an embedded
/// divert) always carry `ptr: None`, matching old ink's own accumulator
/// convention (`content/accumulator.rs::flush` uses `ptr: None` for every
/// flush except the line's own top-level one).
///
/// `trailing_eol`: whether the run's *final* flush may append
/// `Stmt::EndOfLine` (when the content doesn't end with glue). `true` for a
/// genuine content line. `false` for a synthesized fragment that isn't a
/// whole line (wired by the alternation commit).
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
            N::ERROR => {
                i += 1;
            }
            // `{?}` choice points and the annotated-brace family, embedded
            // mid-line — same "not wired yet" story as `lower_one_item`'s
            // catch-all, until their own commits.
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
