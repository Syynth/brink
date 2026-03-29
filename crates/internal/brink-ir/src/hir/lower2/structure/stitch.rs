//! Stitch lowering: `lower_stitch`, `lower_top_level_stitch`.

use brink_syntax::ast::{self, AstNode, AstPtr};

use crate::{Block, ContainerPtr, DiagnosticCode, Knot, ParamInfo, Stitch, SymbolKind};

use super::super::block::LowerBlock;
use super::super::context::{LowerScope, LowerSink};
use super::super::helpers::make_name;
use super::knot::lower_knot_params;

use crate::symbols::LocalSymbol;

pub(super) fn lower_top_level_stitch(
    scope: &mut LowerScope,
    sink: &mut impl LowerSink,
    stitch: &ast::StitchDef,
) -> Option<Knot> {
    let header = stitch.header()?;
    let ident = header.identifier()?;
    let name_text = header.name()?;
    let name = make_name(name_text.clone(), ident.syntax().text_range());

    let params = lower_knot_params(header.params(), sink);
    let param_infos: Vec<ParamInfo> = params
        .iter()
        .map(|p| ParamInfo {
            name: p.name.text.clone(),
            is_ref: p.is_ref,
            is_divert: p.is_divert,
        })
        .collect();
    sink.declare_with(
        SymbolKind::Stitch,
        &name_text,
        ident.syntax().text_range(),
        param_infos,
        None,
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
    let body = stitch
        .body()
        .map_or_else(Block::default, |b| b.lower_block(scope, sink));
    scope.current_knot = None;

    Some(Knot {
        ptr: ContainerPtr::Stitch(AstPtr::new(stitch)),
        name,
        is_function: false,
        params,
        body,
        stitches: Vec::new(),
    })
}

pub(super) fn lower_stitch(
    scope: &mut LowerScope,
    sink: &mut impl LowerSink,
    stitch: &ast::StitchDef,
    knot_name: &str,
) -> Option<Stitch> {
    let range = stitch.syntax().text_range();
    let header = stitch.header().or_else(|| {
        sink.diagnose(range, DiagnosticCode::E002);
        None
    })?;
    let ident = header.identifier().or_else(|| {
        sink.diagnose(range, DiagnosticCode::E002);
        None
    })?;
    let name_text = header.name().or_else(|| {
        sink.diagnose(range, DiagnosticCode::E002);
        None
    })?;
    let name = make_name(name_text.clone(), ident.syntax().text_range());
    let qualified = format!("{knot_name}.{name_text}");

    scope.current_stitch = Some(name_text.clone());
    let params = lower_knot_params(header.params(), sink);
    let param_infos: Vec<ParamInfo> = params
        .iter()
        .map(|p| ParamInfo {
            name: p.name.text.clone(),
            is_ref: p.is_ref,
            is_divert: p.is_divert,
        })
        .collect();
    sink.declare_with(
        SymbolKind::Stitch,
        &qualified,
        ident.syntax().text_range(),
        param_infos,
        None,
    );
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
    let body = stitch
        .body()
        .map_or_else(Block::default, |b| b.lower_block(scope, sink));
    scope.current_stitch = None;

    Some(Stitch {
        ptr: AstPtr::new(stitch),
        name,
        params,
        body,
    })
}
