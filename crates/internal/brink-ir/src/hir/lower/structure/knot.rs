//! Knot lowering: `lower_knot`, `lower_knot_body`, `lower_knot_params`, `lower_param`.

use brink_syntax::ast::{self, AstNode};

use crate::provenance::NodeClass;
use crate::{
    Block, DiagnosticCode, Divert, DivertPath, DivertTarget, Knot, Name, Param, Path, Stitch, Stmt,
};

use super::super::block::LowerBlock;
use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::directive::{
    DirectiveTarget, apply_scope_directives, effects_assertion_from_directives,
    leading_body_directives,
};
use super::super::doc_comment::{DocPolicy, parse_doc_comment};
use super::super::helpers::{make_name, name_from_ident};
use super::super::types::lower_type_annotation;
use super::stitch::lower_stitch;

pub(super) fn lower_knot(
    scope: &mut LowerScope,
    sink: &mut impl LowerSink,
    knot: &ast::KnotDef,
) -> Lowered<Knot> {
    let range = knot.syntax().text_range();
    let header = knot
        .header()
        .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E001))?;
    let ident = header
        .identifier()
        .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E001))?;
    let name_text = header
        .name()
        .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E001))?;
    let name = make_name(name_text.clone(), ident.syntax().text_range());

    let is_function = header.is_function();
    let params = lower_knot_params(header.params(), sink);
    let (doc, issues) = parse_doc_comment(knot.syntax(), DocPolicy::CALLABLE);
    issues.diagnose(sink);

    scope.current_knot = Some(name_text.clone());
    let (body, stitches) = knot.body().map_or_else(
        || (Block::default(), Vec::new()),
        |b| lower_knot_body(scope, sink, &b, &name_text),
    );
    scope.current_knot = None;
    scope.current_stitch = None;

    // `#@local`, `#@private`/`#@public`, and `#@effects(…)` directive
    // line(s) in the leading tag-line run of the body.
    let mut is_local = false;
    let mut effects_assertion = None;
    let mut visibility = None;
    let mut was = None;
    if let Some(b) = knot.body() {
        let dirs = leading_body_directives(b.syntax());
        is_local = apply_scope_directives(&dirs, DirectiveTarget::Knot, sink);
        effects_assertion = effects_assertion_from_directives(&dirs, sink);
        if let Some(vis) = super::super::directive::visibility_from_directives(&dirs, sink) {
            visibility = Some(vis);
        }
        if let Some((old_name, was_range)) =
            super::super::directive::was_from_directives(&dirs, sink)
        {
            if old_name == name_text {
                sink.diagnose(was_range, DiagnosticCode::E095);
            } else {
                was = Some((old_name, was_range));
            }
        }
    }

    let return_type = header
        .return_type()
        .and_then(|ta| lower_type_annotation(&ta));

    Ok(Knot {
        ptr: scope.prov(NodeClass::Knot, knot.syntax()),
        name,
        is_function,
        params,
        body,
        stitches,
        is_local,
        effects_assertion,
        return_type,
        doc,
        visibility,
        was,
    })
}

pub(super) fn lower_knot_body(
    scope: &mut LowerScope,
    sink: &mut impl LowerSink,
    body: &ast::KnotBody,
    knot_name: &str,
) -> (Block, Vec<Stitch>) {
    let stitches: Vec<Stitch> = body
        .stitches()
        .filter_map(|s| lower_stitch(scope, sink, &s, knot_name).ok())
        .collect();
    let mut block = body.lower_block(scope, sink).unwrap_or_default();

    // First-stitch auto-enter — the synthesized `Divert` below is itself
    // the reference `project_manifest` picks up (its `DivertTarget.path`
    // walk), so there is no separate ref to register here.
    if block.stmts.is_empty()
        && let Some(first) = stitches.first()
        && first.params.is_empty()
    {
        block.stmts.push(Stmt::Divert(Divert {
            ptr: None,
            target: DivertTarget {
                path: DivertPath::Path(Path {
                    segments: vec![Name {
                        text: first.name.text.clone(),
                        range: first.name.range,
                    }],
                    range: first.name.range,
                }),
                args: Vec::new(),
            },
        }));
        // The synthesized divert is now the block's final statement
        // (docs/block-effect-model.md §10 row j) — re-derive `tail`.
        block.recompute_tail();
    }

    (block, stitches)
}

pub(super) fn lower_knot_params(
    params: Option<ast::KnotParams>,
    sink: &mut impl LowerSink,
) -> Vec<Param> {
    params
        .map(|p| {
            p.params()
                .filter_map(|pd| lower_param(&pd, sink).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn lower_param(p: &ast::KnotParamDecl, sink: &mut impl LowerSink) -> Lowered<Param> {
    let range = p.syntax().text_range();
    let ident = p
        .identifier()
        .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E003))?;
    let name = name_from_ident(&ident).ok_or_else(|| sink.diagnose(range, DiagnosticCode::E003))?;
    let annotation = p
        .type_annotation()
        .and_then(|ta| lower_type_annotation(&ta));
    Ok(Param {
        name,
        is_ref: p.is_ref(),
        is_divert: p.is_divert(),
        annotation,
    })
}
