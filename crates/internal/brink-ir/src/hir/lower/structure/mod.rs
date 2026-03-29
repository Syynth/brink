//! Structural lowering: source files, knots, stitches, includes.
//!
//! This module produces `HirFile`, `Knot`, `Stitch`, and `IncludeSite` and
//! is the entry point for the full lowering pipeline.

mod include;
mod knot;
mod stitch;

use brink_syntax::ast::{self, AstNode};

use crate::{Block, FileId, HirFile, IncludeSite, Knot, SymbolManifest};

use super::block::lower_weave_body;
use super::context::{EffectSink, LowerScope, LowerSink};
use super::decl::DeclareSymbols;

use include::lower_include;
use knot::lower_knot;
use stitch::lower_top_level_stitch;

// ─── Public API ─────────────────────────────────────────────────────

/// Lower a complete source file to HIR.
///
/// Produces an `(HirFile, SymbolManifest, Vec<Diagnostic>)` tuple.
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

/// Lower a single knot definition in isolation.
///
/// Returns `None` for the knot if the AST node is malformed (e.g. missing header).
pub fn lower_single_knot(
    file_id: FileId,
    knot: &ast::KnotDef,
) -> (Option<Knot>, SymbolManifest, Vec<crate::Diagnostic>) {
    let mut scope = LowerScope::new(file_id);
    let mut sink = EffectSink::new(file_id);

    let result = lower_knot(&mut scope, &mut sink, knot).ok();
    let (manifest, diagnostics) = sink.finish();
    (result, manifest, diagnostics)
}

/// Lower only the top-level content and declarations of a file, skipping knots.
///
/// Useful for incremental analysis where knots are lowered separately.
pub fn lower_top_level(
    file_id: FileId,
    file: &ast::SourceFile,
) -> (Block, Vec<Knot>, SymbolManifest, Vec<crate::Diagnostic>) {
    let mut scope = LowerScope::new(file_id);
    let mut sink = EffectSink::new(file_id);

    // Lower declarations (registers symbols in manifest).
    // Walk descendants — VAR/CONST/LIST are global regardless of nesting.
    let _variables: Vec<_> = file
        .syntax()
        .descendants()
        .filter_map(ast::VarDecl::cast)
        .filter_map(|v| v.declare_and_lower(&scope, &mut sink).ok())
        .collect();
    let _constants: Vec<_> = file
        .syntax()
        .descendants()
        .filter_map(ast::ConstDecl::cast)
        .filter_map(|c| c.declare_and_lower(&scope, &mut sink).ok())
        .collect();
    let _lists: Vec<_> = file
        .syntax()
        .descendants()
        .filter_map(ast::ListDecl::cast)
        .filter_map(|l| l.declare_and_lower(&scope, &mut sink).ok())
        .collect();
    let _externals: Vec<_> = file
        .externals()
        .filter_map(|e| e.declare_and_lower(&scope, &mut sink).ok())
        .collect();

    // Top-level stitches (no parent knot) — promoted to knots.
    let top_level_knots: Vec<_> = file
        .stitches()
        .filter_map(|stitch| lower_top_level_stitch(&mut scope, &mut sink, &stitch).ok())
        .collect();

    let root_content = lower_weave_body(file.syntax(), &scope, &mut sink);
    let (manifest, diagnostics) = sink.finish();
    (root_content, top_level_knots, manifest, diagnostics)
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
        .filter_map(|i| lower_include(&i, sink).ok())
        .collect();
    let mut knots: Vec<Knot> = file
        .knots()
        .filter_map(|k| lower_knot(scope, sink, &k).ok())
        .collect();
    for stitch in file.stitches() {
        if let Ok(knot) = lower_top_level_stitch(scope, sink, &stitch) {
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
