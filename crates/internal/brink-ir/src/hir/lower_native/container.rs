//! `flow`/`fn` declaration heads → `Knot`/`Stitch` (`docs/b0-sequencing.md`
//! §B0.6). Bodies are always the empty stub — see the `lower_native` module
//! doc's judgment call #2 (bodies deferred to B0.7/B0.8).
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
use crate::provenance::NodeClass;
use crate::{Block, Diagnostic, DiagnosticCode, Knot, Name, Param, Stitch};

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

    let mut stitches = Vec::new();
    if let Some(body) = node.body() {
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

    Some(Knot {
        ptr: native_provenance(file_id, NodeClass::Knot, syntax),
        name,
        is_function: node.is_function(),
        params,
        body: Block::default(),
        stitches,
        is_local: false,
        effects_assertion: None,
        return_type: None,
        doc: None,
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
    // do not flatten it into this stitch.
    if let Some(body) = node.body() {
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
    Some(Stitch {
        ptr: native_provenance(file_id, NodeClass::Stitch, syntax),
        name,
        params,
        body: Block::default(),
        is_local: false,
        effects_assertion: None,
        doc: None,
        visibility: None,
        was: None,
    })
}
