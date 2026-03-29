//! Structural lowering: source files, knots, stitches, includes.
//!
//! This module produces `HirFile`, `Knot`, `Stitch`, and `IncludeSite` and
//! is the entry point for the full lowering pipeline.

use brink_syntax::ast::{self, AstNode, AstPtr};

use crate::{
    Block, ContainerPtr, DiagnosticCode, Divert, DivertPath, DivertTarget, FileId, HirFile,
    IncludeSite, Knot, Name, Param, ParamInfo, Path, Stitch, Stmt, SymbolKind, SymbolManifest,
};

use super::block::{LowerBlock, lower_weave_body};
use super::context::{EffectSink, LowerScope, LowerSink};
use super::decl::DeclareSymbols;
use super::helpers::{make_name, name_from_ident};

use crate::symbols::LocalSymbol;

// ─── Public API ─────────────────────────────────────────────────────

/// Lower a complete source file to HIR.
///
/// This is the lower2 equivalent of [`crate::hir::lower::lower`]. It
/// produces the same `(HirFile, SymbolManifest, Vec<Diagnostic>)` tuple.
pub fn lower(
    file_id: FileId,
    file: &ast::SourceFile,
) -> (HirFile, SymbolManifest, Vec<crate::Diagnostic>) {
    let mut scope = LowerScope::new(file_id);
    let mut sink = EffectSink::new(file_id);

    let hir = lower_source_file(&mut scope, &mut sink, file);
    let (manifest, diagnostics) = sink.finish();
    (hir, manifest, diagnostics)
}

// ─── Source file ────────────────────────────────────────────────────

fn lower_source_file(
    scope: &mut LowerScope,
    sink: &mut impl LowerSink,
    file: &ast::SourceFile,
) -> HirFile {
    // In ink, VAR/CONST/LIST are always global regardless of where they
    // appear. Walk the entire tree to collect them all.
    let variables = file
        .syntax()
        .descendants()
        .filter_map(ast::VarDecl::cast)
        .filter_map(|v| v.declare_and_lower(scope, sink).ok())
        .collect();
    let constants = file
        .syntax()
        .descendants()
        .filter_map(ast::ConstDecl::cast)
        .filter_map(|c| c.declare_and_lower(scope, sink).ok())
        .collect();
    let lists = file
        .syntax()
        .descendants()
        .filter_map(ast::ListDecl::cast)
        .filter_map(|l| l.declare_and_lower(scope, sink).ok())
        .collect();
    let externals = file
        .externals()
        .filter_map(|e| e.declare_and_lower(scope, sink).ok())
        .collect();
    let includes: Vec<IncludeSite> = file
        .includes()
        .filter_map(|i| lower_include(&i, sink))
        .collect();
    let mut knots: Vec<Knot> = file
        .knots()
        .filter_map(|k| lower_knot(scope, sink, &k))
        .collect();
    for stitch in file.stitches() {
        if let Some(knot) = lower_top_level_stitch(scope, sink, &stitch) {
            knots.push(knot);
        }
    }
    let root_content = lower_weave_body(file.syntax(), scope, sink);

    HirFile {
        root_content,
        knots,
        variables,
        constants,
        lists,
        externals,
        includes,
    }
}

// ─── Knots and stitches ─────────────────────────────────────────────

fn lower_knot(
    scope: &mut LowerScope,
    sink: &mut impl LowerSink,
    knot: &ast::KnotDef,
) -> Option<Knot> {
    let range = knot.syntax().text_range();
    let header = knot.header().or_else(|| {
        sink.diagnose(range, DiagnosticCode::E001);
        None
    })?;
    let ident = header.identifier().or_else(|| {
        sink.diagnose(range, DiagnosticCode::E001);
        None
    })?;
    let name_text = header.name().or_else(|| {
        sink.diagnose(range, DiagnosticCode::E001);
        None
    })?;
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

    Some(Knot {
        ptr: ContainerPtr::Knot(AstPtr::new(knot)),
        name,
        is_function,
        params,
        body,
        stitches,
    })
}

fn lower_knot_body(
    scope: &mut LowerScope,
    sink: &mut impl LowerSink,
    body: &ast::KnotBody,
    knot_name: &str,
) -> (Block, Vec<Stitch>) {
    let stitches: Vec<Stitch> = body
        .stitches()
        .filter_map(|s| lower_stitch(scope, sink, &s, knot_name))
        .collect();
    let mut block = body.lower_block(scope, sink);

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

fn lower_top_level_stitch(
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

fn lower_stitch(
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

// ─── Params ─────────────────────────────────────────────────────────

fn lower_knot_params(params: Option<ast::KnotParams>, sink: &mut impl LowerSink) -> Vec<Param> {
    params
        .map(|p| p.params().filter_map(|pd| lower_param(&pd, sink)).collect())
        .unwrap_or_default()
}

fn lower_param(p: &ast::KnotParamDecl, sink: &mut impl LowerSink) -> Option<Param> {
    let range = p.syntax().text_range();
    let ident = p.identifier().or_else(|| {
        sink.diagnose(range, DiagnosticCode::E003);
        None
    })?;
    let name = name_from_ident(&ident).or_else(|| {
        sink.diagnose(range, DiagnosticCode::E003);
        None
    })?;
    Some(Param {
        name,
        is_ref: p.is_ref(),
        is_divert: p.is_divert(),
    })
}

// ─── Includes ───────────────────────────────────────────────────────

fn lower_include(inc: &ast::IncludeStmt, sink: &mut impl LowerSink) -> Option<IncludeSite> {
    let file_path = inc.file_path().or_else(|| {
        sink.diagnose(inc.syntax().text_range(), DiagnosticCode::E011);
        None
    })?;
    let raw = file_path.text();
    let cleaned = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(&raw);
    Some(IncludeSite {
        file_path: cleaned.to_owned(),
        ptr: AstPtr::new(inc),
    })
}
