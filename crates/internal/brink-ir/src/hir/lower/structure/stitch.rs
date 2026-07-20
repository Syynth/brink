//! Stitch lowering: `lower_stitch`, `lower_top_level_stitch`.

use brink_syntax::ast::{self, AstNode};

use crate::provenance::NodeClass;
use crate::{Block, DiagnosticCode, Knot, Stitch};

use super::super::block::LowerBlock;
use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::directive::{
    DirectiveTarget, apply_scope_directives, effects_assertion_from_directives,
    leading_body_directives,
};
use super::super::doc_comment::{DocPolicy, parse_doc_comment};
use super::super::helpers::make_name;
use super::knot::lower_knot_params;

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
    let (doc, issues) = parse_doc_comment(stitch.syntax(), DocPolicy::CALLABLE);
    issues.diagnose(sink);

    scope.current_knot = Some(name_text.clone());
    let body = stitch.body().map_or_else(Block::default, |b| {
        b.lower_block(scope, sink).unwrap_or_default()
    });
    scope.current_knot = None;

    // `#@local`, `#@private`/`#@public`, and `#@effects(…)` directives in
    // the leading tag-line run of the body.
    let mut is_local = false;
    let mut effects_assertion = None;
    let mut visibility = None;
    let mut was = None;
    if let Some(b) = stitch.body() {
        let dirs = leading_body_directives(b.syntax());
        is_local = apply_scope_directives(&dirs, DirectiveTarget::Stitch, sink);
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

    Ok(Knot {
        ptr: scope.prov(NodeClass::Stitch, stitch.syntax()),
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
        doc,
        visibility,
        was,
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
    let (doc, issues) = parse_doc_comment(stitch.syntax(), DocPolicy::CALLABLE);
    issues.diagnose(sink);
    let body = stitch.body().map_or_else(Block::default, |b| {
        b.lower_block(scope, sink).unwrap_or_default()
    });
    scope.current_stitch = None;

    // `#@local`, `#@private`/`#@public`, and `#@effects(…)` directives in
    // the leading tag-line run of the body.
    let mut is_local = false;
    let mut effects_assertion = None;
    let mut visibility = None;
    let mut was = None;
    if let Some(b) = stitch.body() {
        let dirs = leading_body_directives(b.syntax());
        is_local = apply_scope_directives(&dirs, DirectiveTarget::Stitch, sink);
        effects_assertion = effects_assertion_from_directives(&dirs, sink);
        if let Some(vis) = super::super::directive::visibility_from_directives(&dirs, sink) {
            visibility = Some(vis);
        }
        // `#@was(old_name)` on a nested stitch takes the bare old stitch
        // name (the enclosing knot isn't being renamed) — qualify it the
        // same way the manifest projection qualifies the current name
        // before comparing/storing (`knot.old_name`).
        if let Some((old_name, was_range)) =
            super::super::directive::was_from_directives(&dirs, sink)
        {
            let old_qualified = format!("{knot_name}.{old_name}");
            if old_qualified == qualified {
                sink.diagnose(was_range, DiagnosticCode::E095);
            } else {
                was = Some((old_qualified, was_range));
            }
        }
    }

    Ok(Stitch {
        ptr: scope.prov(NodeClass::Stitch, stitch.syntax()),
        name,
        params,
        body,
        is_local,
        effects_assertion,
        doc,
        visibility,
        was,
    })
}
