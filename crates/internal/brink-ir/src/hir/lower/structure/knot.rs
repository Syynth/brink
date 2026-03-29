//! Knot lowering: `lower_knot`, `lower_knot_body`, `lower_knot_params`, `lower_param`.

use brink_syntax::ast::{self, AstNode, AstPtr};

use crate::{
    Block, ContainerPtr, DiagnosticCode, Divert, DivertPath, DivertTarget, Knot, Name, Param,
    ParamInfo, Path, Stitch, Stmt, SymbolKind,
};

use super::super::block::LowerBlock;
use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::helpers::{make_name, name_from_ident};
use super::stitch::lower_stitch;

use crate::symbols::LocalSymbol;

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
    let param_infos: Vec<ParamInfo> = params
        .iter()
        .map(|p| ParamInfo {
            name: p.name.text.clone(),
            is_ref: p.is_ref,
            is_divert: p.is_divert,
        })
        .collect();
    let detail = if is_function {
        Some("function".to_owned())
    } else {
        None
    };
    sink.declare_with(
        SymbolKind::Knot,
        &name_text,
        ident.syntax().text_range(),
        param_infos,
        detail,
    );

    scope.current_knot = Some(name_text.clone());
    for p in &params {
        sink.add_local(LocalSymbol {
            name: p.name.text.clone(),
            range: p.name.range,
            scope: scope.to_scope(),
            kind: crate::SymbolKind::Param,
            param_detail: Some(ParamInfo {
                name: p.name.text.clone(),
                is_ref: p.is_ref,
                is_divert: p.is_divert,
            }),
        });
    }
    let (body, stitches) = knot.body().map_or_else(
        || (Block::default(), Vec::new()),
        |b| lower_knot_body(scope, sink, &b, &name_text),
    );
    scope.current_knot = None;
    scope.current_stitch = None;

    Ok(Knot {
        ptr: ContainerPtr::Knot(AstPtr::new(knot)),
        name,
        is_function,
        params,
        body,
        stitches,
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

    // First-stitch auto-enter
    if block.stmts.is_empty()
        && let Some(first) = stitches.first()
        && first.params.is_empty()
    {
        sink.add_unresolved(
            &first.name.text,
            first.name.range,
            crate::symbols::RefKind::Divert,
            &scope.to_scope(),
            None,
        );
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
    Ok(Param {
        name,
        is_ref: p.is_ref(),
        is_divert: p.is_divert(),
    })
}
