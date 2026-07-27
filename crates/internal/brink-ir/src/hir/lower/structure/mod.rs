//! Structural lowering: source files, knots, stitches, includes.
//!
//! This module produces `HirFile`, `Knot`, `Stitch`, and `IncludeSite` and
//! is the entry point for the full lowering pipeline.

mod import;
mod include;
mod knot;
mod stitch;

use brink_syntax::ast::{self, AstNode};

use crate::symbols::project_manifest;
use crate::{Block, DiagnosticCode, FileId, HirFile, Import, IncludeSite, Knot, SymbolManifest};

use super::block::lower_weave_body;
use super::context::{EffectSink, LowerScope, LowerSink};
use super::decl::DeclareSymbols;

use import::lower_import;
use include::lower_include;
use knot::lower_knot;
use stitch::lower_top_level_stitch;

// ─── Public API ─────────────────────────────────────────────────────

/// Lower a complete source file to HIR.
///
/// Produces an `(HirFile, SymbolManifest, Vec<Diagnostic>)` tuple — the
/// manifest is [`project_manifest`]'s pure projection of the just-built
/// `HirFile` (B0.4, docs/hir-admission-contract.md Q3(b)), not a
/// hand-accumulated side effect of lowering.
pub fn lower(
    file_id: FileId,
    file: &ast::SourceFile,
) -> (HirFile, SymbolManifest, Vec<crate::Diagnostic>) {
    let mut scope = LowerScope::new(file_id);
    let mut sink = EffectSink::new(file_id);

    let hir = lower_source_file(&mut scope, &mut sink, file);
    let manifest = project_manifest(&hir);
    let diagnostics = sink.finish();
    (hir, manifest, diagnostics)
}

/// Lower a single knot definition in isolation.
///
/// Returns `None` for the knot if the AST node is malformed (e.g. missing
/// header). No manifest — a per-knot manifest fragment has no `HirFile` to
/// project from; callers that need one (`brink-db`'s `lower_file`) project
/// it once from the fully assembled `HirFile` instead.
pub fn lower_single_knot(
    file_id: FileId,
    knot: &ast::KnotDef,
) -> (Option<Knot>, Vec<crate::Diagnostic>) {
    let mut scope = LowerScope::new(file_id);
    let mut sink = EffectSink::new(file_id);

    let result = lower_knot(&mut scope, &mut sink, knot).ok();
    let diagnostics = sink.finish();
    (result, diagnostics)
}

/// Lower only the top-level content and declarations of a file, skipping
/// knots. No manifest — see [`lower_single_knot`]'s doc.
///
/// Useful for incremental analysis where knots are lowered separately.
pub fn lower_top_level(
    file_id: FileId,
    file: &ast::SourceFile,
) -> (Block, Vec<Knot>, Vec<crate::Diagnostic>) {
    let mut scope = LowerScope::new(file_id);
    let mut sink = EffectSink::new(file_id);

    // Lower declarations.
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
    // TM-4b (docs/typed-mode-spec.md §6): `STRUCT` is top-level only, unlike
    // `VAR`/`CONST`/`LIST` — `file.struct_decls()` is a direct-children scan.
    let _structs: Vec<_> = file
        .struct_decls()
        .filter_map(|s| s.declare_and_lower(&scope, &mut sink).ok())
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
    let diagnostics = sink.finish();
    (root_content, top_level_knots, diagnostics)
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
    // TM-4b (docs/typed-mode-spec.md §6): `STRUCT` is top-level only, unlike
    // `VAR`/`CONST`/`LIST` — `file.struct_decls()` is a direct-children scan.
    let structs = file
        .struct_decls()
        .filter_map(|s| s.declare_and_lower(scope, sink).ok())
        .collect();
    let externals = file
        .externals()
        .filter_map(|e| e.declare_and_lower(scope, sink).ok())
        .collect();
    let includes: Vec<IncludeSite> = file
        .includes()
        .filter_map(|i| lower_include(scope, &i, sink).ok())
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

    // M-1 (docs/modules-spec.md §1): a file-level `#@module(name)`
    // directive declares the module explicitly. Absent, the file is an
    // undeclared stem-module (identity hashing stays byte-identical).
    let module =
        super::directive::file_module_declaration(file.syntax(), sink).map(|(name, range)| {
            crate::ModuleDecl {
                name,
                range,
                was: None,
            }
        });

    // M-3 (docs/modules-spec.md §5): a file-level `#@was(old_name)` records
    // the module's rename. Only meaningful alongside `#@module` — a
    // self-alias (`old_name` equals the current module name) is `E095`
    // ("nothing to migrate"); a `#@was` with no `#@module` to attach to is
    // `E049` ("directive not supported on this target").
    let module_was = super::directive::file_module_was(file.syntax(), sink);
    let module = match (module, module_was) {
        (Some(mut m), Some((old_name, was_range))) => {
            if old_name == m.name {
                sink.diagnose(was_range, DiagnosticCode::E095);
            } else {
                m.was = Some((old_name, was_range));
            }
            Some(m)
        }
        (Some(m), None) => Some(m),
        (None, Some((_, was_range))) => {
            sink.diagnose(was_range, DiagnosticCode::E049);
            None
        }
        (None, None) => None,
    };

    // M-2 (docs/modules-spec.md §2/§4): `IMPORT` statements and the
    // `#@private`/`#@public` directive occurrences (for the dialect gate).
    let imports: Vec<Import> = file.imports().map(|i| lower_import(&i)).collect();
    let visibility = super::directive::collect_visibility_directives(file.syntax());
    let was_directives = super::directive::collect_was_directives(file.syntax());

    HirFile {
        root_content,
        knots,
        variables,
        constants,
        lists,
        structs,
        externals,
        includes,
        module,
        imports,
        visibility,
        was_directives,
        // The `@[allow(…)]` suppression channel (issue #1161) is native-only
        // — ink's `@[…]` placement is the top of a knot/stitch *body*, which
        // has no ruled `allow` tenant. Ink authors keep the line-scoped
        // `//brink-disable` comment channel and the project `[lints]` table.
        allow_scopes: Vec::new(),
    }
}
