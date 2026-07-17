//! Stitch lowering: `lower_stitch`, `lower_top_level_stitch`.

use brink_syntax::ast::{self, AstNode, AstPtr};

use crate::{Block, ContainerPtr, DiagnosticCode, Knot, ParamInfo, Stitch, SymbolKind};

use super::super::block::LowerBlock;
use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::directive::{
    DirectiveTarget, apply_scope_directives, effects_assertion_from_directives,
    leading_body_directives,
};
use super::super::doc_comment::{DocPolicy, parse_doc_comment};
use super::super::helpers::make_name;
use super::knot::lower_knot_params;

use crate::symbols::LocalSymbol;

pub(super) fn lower_top_level_stitch(
    scope: &mut LowerScope,
    sink: &mut impl LowerSink,
    stitch: &ast::StitchDef,
) -> Lowered<Knot> {
    let range = stitch.syntax().text_range();
    let header = stitch
        .header()
        .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E002))?;
    let ident = header
        .identifier()
        .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E002))?;
    let name_text = header
        .name()
        .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E002))?;
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
    let (doc, issues) = parse_doc_comment(stitch.syntax(), DocPolicy::CALLABLE);
    issues.diagnose(sink);
    sink.declare_full(
        SymbolKind::Stitch,
        &name_text,
        ident.syntax().text_range(),
        param_infos,
        None,
        doc,
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
    let body = stitch.body().map_or_else(Block::default, |b| {
        b.lower_block(scope, sink).unwrap_or_default()
    });
    scope.current_knot = None;

    // `#@local`, `#@private`/`#@public`, and `#@effects(…)` directives in
    // the leading tag-line run of the body.
    let mut is_local = false;
    let mut effects_assertion = None;
    if let Some(b) = stitch.body() {
        let dirs = leading_body_directives(b.syntax());
        is_local = apply_scope_directives(&dirs, DirectiveTarget::Stitch, sink);
        effects_assertion = effects_assertion_from_directives(&dirs, sink);
        if let Some(vis) = super::super::directive::visibility_from_directives(&dirs, sink) {
            sink.set_visibility(crate::SymbolKind::Stitch, &name_text, vis);
        }
        if let Some((old_name, was_range)) =
            super::super::directive::was_from_directives(&dirs, sink)
        {
            if old_name == name_text {
                sink.diagnose(was_range, DiagnosticCode::E095);
            } else {
                sink.set_was(crate::SymbolKind::Stitch, &name_text, old_name, was_range);
            }
        }
    }

    Ok(Knot {
        ptr: ContainerPtr::Stitch(AstPtr::new(stitch)),
        name,
        is_function: false,
        params,
        body,
        stitches: Vec::new(),
        is_local,
        effects_assertion,
        // `= stitch` headers never carry a return-type annotation — that
        // grammar only exists on `== knot ==` headers (TM-2, docs/typed-mode-spec.md
        // §3: `): type ===`), which this promoted-top-level-stitch path
        // never parses through.
        return_type: None,
    })
}

pub(super) fn lower_stitch(
    scope: &mut LowerScope,
    sink: &mut impl LowerSink,
    stitch: &ast::StitchDef,
    knot_name: &str,
) -> Lowered<Stitch> {
    let range = stitch.syntax().text_range();
    let header = stitch
        .header()
        .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E002))?;
    let ident = header
        .identifier()
        .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E002))?;
    let name_text = header
        .name()
        .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E002))?;
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
    let (doc, issues) = parse_doc_comment(stitch.syntax(), DocPolicy::CALLABLE);
    issues.diagnose(sink);
    sink.declare_full(
        SymbolKind::Stitch,
        &qualified,
        ident.syntax().text_range(),
        param_infos,
        None,
        doc,
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
    let body = stitch.body().map_or_else(Block::default, |b| {
        b.lower_block(scope, sink).unwrap_or_default()
    });
    scope.current_stitch = None;

    // `#@local`, `#@private`/`#@public`, and `#@effects(…)` directives in
    // the leading tag-line run of the body.
    let mut is_local = false;
    let mut effects_assertion = None;
    if let Some(b) = stitch.body() {
        let dirs = leading_body_directives(b.syntax());
        is_local = apply_scope_directives(&dirs, DirectiveTarget::Stitch, sink);
        effects_assertion = effects_assertion_from_directives(&dirs, sink);
        if let Some(vis) = super::super::directive::visibility_from_directives(&dirs, sink) {
            sink.set_visibility(crate::SymbolKind::Stitch, &qualified, vis);
        }
        // `#@was(old_name)` on a nested stitch takes the bare old stitch
        // name (the enclosing knot isn't being renamed) — qualify it the
        // same way `declare_full` qualified the current name before
        // comparing/storing.
        if let Some((old_name, was_range)) =
            super::super::directive::was_from_directives(&dirs, sink)
        {
            let old_qualified = format!("{knot_name}.{old_name}");
            if old_qualified == qualified {
                sink.diagnose(was_range, DiagnosticCode::E095);
            } else {
                sink.set_was(
                    crate::SymbolKind::Stitch,
                    &qualified,
                    old_qualified,
                    was_range,
                );
            }
        }
    }

    Ok(Stitch {
        ptr: AstPtr::new(stitch),
        name,
        params,
        body,
        is_local,
        effects_assertion,
    })
}
