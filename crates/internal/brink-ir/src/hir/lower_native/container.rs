//! `flow`/`fn` declaration heads → `Knot`/`Stitch` (`docs/b0-sequencing.md`
//! §B0.6). Bodies lower through [`lower_body`], dialect-dispatched on the
//! body-dialect selector (charter §4, #1309): prose rides B0.7's
//! `body::lower_block`, code rides B0.8's `body::lower_stmt_block_as_body`.
//!
//! Nesting fence (Q4(b), `docs/hir-admission-contract.md` §5 Q4): exactly
//! two container levels. A top-level `flow`/`fn` → `Knot`; a `flow` nested
//! one level inside a `Knot`'s body → `Stitch` (native has no "promoted
//! top-level stitch" concept — unlike ink, nesting position alone decides
//! Knot vs Stitch, so every native `Knot`'s provenance class is always
//! [`NodeClass::Knot`], never [`NodeClass::Stitch`]). A `flow` nested a
//! third level deep, or an `fn` nested below top level at all (no HIR
//! container carries `is_function` below `Knot`), is rejected loudly
//! (E130 / E129 respectively) rather than silently flattened or dropped.

use brink_syntax_native::ast::{self, AstNode as _};

use crate::hir::FileId;
use crate::hir::doc_block::DocPolicy;
use crate::provenance::NodeClass;
use crate::{Block, Diagnostic, DiagnosticCode, DocBlock, Knot, Name, Param, Stitch};

use super::doc_comment::lower_doc_comment;
use super::provenance::native_provenance;

/// A `flow`/`fn`'s doc comment: the leading `///` form if present, else the
/// body's inner `//!` form (B0.6b judgment call — the two forms are not
/// merged; the outer form wins when both are somehow present, since it's
/// the one visible from outside the container without opening its body).
///
/// The inner `//!` form only exists for a prose-ground body — a code-ground
/// `STMT_BLOCK` has no inner-doc attachment point (`parser::block::
/// braced_item_list`'s `maybe_consume_inner_run` call is gated on `kind ==
/// BLOCK`) — so a `Body::Code` body simply contributes none, same as an
/// absent body.
fn container_doc(
    file_id: FileId,
    outer: Option<ast::DocComment>,
    body: Option<&ast::Body>,
    policy: DocPolicy,
    diags: &mut Vec<Diagnostic>,
) -> Option<DocBlock> {
    if let Some(doc) = lower_doc_comment(file_id, outer, policy, diags) {
        return Some(doc);
    }
    let inner = match body {
        Some(ast::Body::Prose(b)) => b.doc(),
        Some(ast::Body::Code(_)) | None => None,
    };
    lower_doc_comment(file_id, inner, policy, diags)
}

/// Lower a `flow`/`fn`'s body — whichever body-dialect the selector chose
/// (charter §4) — to the HIR `Block` a `Knot`/`Stitch` carries. A prose body
/// rides B0.7's `body::lower_block` unchanged; a code body (`fn`'s default,
/// or a `flow`'s `~{ }` "Compound guard" override) lowers its statements via
/// the existing B0.8 `control_flow::lower_stmt_block` and wraps the result
/// as the container's sole statement — a single `Stmt::LogicBlock`, exactly
/// the shape a brink-dialect container whose entire body is one `~ { … }`
/// block already produces (`hir::lower::content::logic_line`). No new HIR
/// node: NF-2's existing-HIR-only fence, and the differential partner this
/// reuses is already fully wired to LIR (`lir::lower::blocks`).
fn lower_body(file_id: FileId, body: &ast::Body, diags: &mut Vec<Diagnostic>) -> Block {
    match body {
        ast::Body::Prose(b) => super::body::lower_block(file_id, b, diags),
        ast::Body::Code(sb) => super::body::lower_stmt_block_as_body(file_id, sb, diags),
    }
}

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

fn lower_params(param_list: Option<ast::ParamList>) -> Vec<Param> {
    param_list
        .into_iter()
        .flat_map(|pl| pl.params().collect::<Vec<_>>())
        .filter_map(|p| {
            name_from(p.name_token()).map(|name| Param {
                name,
                is_ref: p.is_ref(),
                // Neither a `->`-typed divert param nor a `: type`
                // annotation exists in this grammar skeleton
                // (`parser/decl.rs::param`) — always false/`None`.
                is_divert: false,
                annotation: None,
            })
        })
        .collect()
}

/// Lower a top-level `flow name(params) { … }` or `fn name(params) { … }`
/// to a `Knot`. `is_function` distinguishes the two; both stamp
/// [`NodeClass::Knot`] provenance (native never produces a "promoted
/// stitch" — see the module doc).
pub(super) fn lower_top_level_container(
    file_id: FileId,
    node: &super::FlowOrFn,
    diags: &mut Vec<Diagnostic>,
) -> Option<Knot> {
    let syntax = node.syntax();
    let range = syntax.text_range();
    let Some(name) = name_from(node.name_token()) else {
        // E001 covers both spellings — ink has no separate fn-vs-flow
        // missing-name code (its own `== knot ==` grammar has no `is_
        // function` split at the *name* position either).
        diags.push(diag(file_id, range, DiagnosticCode::E001));
        return None;
    };
    let params = lower_params(node.param_list());

    // A nested `flow`/`fn` declaration can only appear inside a
    // *prose*-ground body — `parser/stmt.rs`'s code-ground statement
    // dispatch has no declaration arm, so a `~{ }`-bodied flow's `STMT_BLOCK`
    // structurally cannot contain one. Only `Body::Prose` is scanned.
    let mut stitches = Vec::new();
    if let Some(ast::Body::Prose(body)) = node.body() {
        for child in body.items() {
            match child.kind() {
                brink_syntax_native::SyntaxKind::FLOW_DECL => {
                    if let Some(nested) = ast::FlowDecl::cast(child.clone())
                        && let Some(stitch) = lower_stitch(file_id, &nested, &name.text, diags)
                    {
                        stitches.push(stitch);
                    }
                }
                brink_syntax_native::SyntaxKind::FN_DECL => {
                    // No HIR container carries `is_function` below `Knot` —
                    // a nested `fn` has nowhere to go yet.
                    diags.push(diag(file_id, child.text_range(), DiagnosticCode::E129));
                }
                _ => {}
            }
        }
    }

    // B0.7 (`docs/b0-sequencing.md` §B0.7): the body is now the real
    // dialect-appropriate lowering, not the empty stub — a prose body rides
    // `super::body::lower_block`, walking the same `body.items()` the loop
    // above just scanned for nested `flow`/`fn` declarations (skipping
    // those, and every other declaration kind, as body-item statements, so
    // there is no double lowering); a code body (B0.8/#1309) rides
    // `lower_body`'s `Body::Code` arm instead — see that function's doc.
    let doc = container_doc(
        file_id,
        node.doc(),
        node.body().as_ref(),
        DocPolicy::CALLABLE,
        diags,
    );
    let mut body_block = node
        .body()
        .map_or_else(Block::default, |b| lower_body(file_id, &b, diags));
    super::body::fixup_return_kind(node.is_function(), &mut body_block);
    // Non-function flows inherit ink's root-content implicit-end grace
    // (RULED 2026-07-22; charter §15): a body falling off the end gets a
    // synthesized `-> DONE`. Functions are excluded — they implicitly
    // *return* on exhaustion, not DONE.
    if !node.is_function() {
        super::body::apply_implicit_done(&mut body_block);
    }

    Some(Knot {
        ptr: native_provenance(file_id, NodeClass::Knot, syntax),
        name,
        is_function: node.is_function(),
        params,
        body: body_block,
        stitches,
        is_local: false,
        effects_assertion: None,
        return_type: None,
        doc,
        visibility: None,
        was: None,
    })
}

/// Lower a `flow` nested one level inside a `Knot`'s body to a `Stitch`.
/// `enclosing_knot_name` is unused for the HIR node itself (a `Stitch`'s
/// `name` stays bare — `project_manifest` does the `knot.stitch`
/// qualification, see `symbols/project.rs::project_knot`) but is threaded
/// through so a depth-3 diagnostic can name the full path.
fn lower_stitch(
    file_id: FileId,
    node: &ast::FlowDecl,
    enclosing_knot_name: &str,
    diags: &mut Vec<Diagnostic>,
) -> Option<Stitch> {
    let syntax = node.syntax();
    let range = syntax.text_range();
    let Some(name) = name_from(node.name_token()) else {
        diags.push(diag(file_id, range, DiagnosticCode::E002));
        return None;
    };
    let params = lower_params(node.param_list());

    // Depth-3 fence (Q4(b)): a `flow` nested inside *this* stitch's body is
    // one level too deep. Reject each occurrence loudly; do not lower it,
    // do not flatten it into this stitch. Only a prose-ground body can
    // structurally contain one — see `lower_top_level_container`'s parallel
    // comment.
    if let Some(ast::Body::Prose(body)) = node.body() {
        for child in body.items() {
            match child.kind() {
                brink_syntax_native::SyntaxKind::FLOW_DECL => {
                    diags.push(diag(file_id, child.text_range(), DiagnosticCode::E130));
                }
                brink_syntax_native::SyntaxKind::FN_DECL => {
                    diags.push(diag(file_id, child.text_range(), DiagnosticCode::E129));
                }
                _ => {}
            }
        }
    }
    let _ = enclosing_knot_name; // threaded for future diagnostic-message use
    let doc = container_doc(
        file_id,
        node.doc(),
        node.body().as_ref(),
        DocPolicy::CALLABLE,
        diags,
    );
    let mut body_block = node
        .body()
        .map_or_else(Block::default, |b| lower_body(file_id, &b, diags));
    // Stitches never carry `is_function` (no HIR container below `Knot`
    // does — module doc judgment call #4), so a bare `return` inside a
    // stitch is always the tunnel-return spelling, never an explicit
    // function return.
    super::body::fixup_return_kind(false, &mut body_block);
    // Stitches are never functions, so they always inherit the implicit-end
    // grace (charter §15): fall off the end → synthesized `-> DONE`.
    super::body::apply_implicit_done(&mut body_block);
    Some(Stitch {
        ptr: native_provenance(file_id, NodeClass::Stitch, syntax),
        name,
        params,
        body: body_block,
        is_local: false,
        effects_assertion: None,
        doc,
        visibility: None,
        was: None,
    })
}
