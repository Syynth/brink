//! Structural lowering: source files, knots, stitches, includes.
//!
//! This module produces `HirFile`, `Knot`, `Stitch`, and `IncludeSite` and
//! is the entry point for the full lowering pipeline.

mod import;
mod include;
mod knot;
mod stitch;

use brink_syntax::ast::{self, AstNode};
use brink_syntax::{SyntaxKind, SyntaxNode};

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
    emit_author_warnings(&mut sink, file.syntax(), SkipInsideKnots::No);
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
    emit_author_warnings(&mut sink, knot.syntax(), SkipInsideKnots::No);
    let diagnostics = sink.finish();
    (result, diagnostics)
}

/// Lower only a file's DECLARATION surface — everything [`lower`] computes
/// except knots, top-level stitches, and root content (left empty), and
/// without the manifest projection or the author-warning walk. The db
/// road's `lower_file` assembler (#3088) uses this to harvest declarations,
/// module identity, imports, and directive collections without re-lowering
/// every knot it already lowered via [`lower_single_knot`]. The returned
/// diagnostics — decl lowering plus the file-level `#@module`/`#@was`
/// arbitration (`E095`/`E049`) — are the assembler's KEPT decl-diagnostic
/// source. Before #3088 the arbitration pair was silently dropped on the
/// db road (the whole-file [`lower`] call that emitted it was discarded),
/// while the analyzer skipped re-diagnosing E095 on the assumption
/// lowering had surfaced it; keeping this sink closes that hole.
/// Composed from the same [`lower_decl_head`]/[`assemble_hir_file`] pieces
/// as [`lower`] itself, so the two surfaces cannot drift apart again.
pub fn lower_declarations(
    file_id: FileId,
    file: &ast::SourceFile,
) -> (HirFile, Vec<crate::Diagnostic>) {
    let mut scope = LowerScope::new(file_id);
    let mut sink = EffectSink::new(file_id);
    let head = lower_decl_head(&mut scope, &mut sink, file);
    let hir = assemble_hir_file(&mut sink, file, head, Vec::new(), Block::default());
    let diagnostics = sink.finish();
    (hir, diagnostics)
}

/// Lower only the top-level CONTENT of a file — top-level stitches, root
/// weave content, and top-level `TODO:` notes — skipping knots. No
/// manifest — see [`lower_single_knot`]'s doc.
///
/// Declarations are NOT lowered here (#3088): before the fix this
/// function re-walked every `VAR`/`CONST`/`LIST`/`STRUCT`/`EXTERNAL`
/// purely for their diagnostics (values discarded), duplicating the walk
/// the assembler's [`lower_declarations`] call performs — whose
/// diagnostics are now the kept copy.
pub fn lower_top_level(
    file_id: FileId,
    file: &ast::SourceFile,
) -> (Block, Vec<Knot>, Vec<crate::Diagnostic>) {
    let mut scope = LowerScope::new(file_id);
    let mut sink = EffectSink::new(file_id);

    // Top-level stitches (no parent knot) — promoted to knots.
    let top_level_knots: Vec<_> = file
        .stitches()
        .filter_map(|stitch| lower_top_level_stitch(&mut scope, &mut sink, &stitch).ok())
        .collect();

    let root_content = lower_weave_body(file.syntax(), &scope, &mut sink);
    emit_author_warnings(&mut sink, file.syntax(), SkipInsideKnots::Yes);
    let diagnostics = sink.finish();
    (root_content, top_level_knots, diagnostics)
}

// ─── Author warnings (`TODO:` notes, issue #3050) ───────────────────

/// Whether [`emit_author_warnings`] should skip notes nested inside a
/// `KNOT_DEF`. The db road lowers a file as [`lower_top_level`] plus one
/// [`lower_single_knot`] per knot — top-level emission must exclude
/// knot-nested notes or they would be diagnosed twice.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SkipInsideKnots {
    Yes,
    No,
}

/// Surface every ink `TODO:` author note under `node` as an `E189` `Info`
/// diagnostic (issue #3050). `AUTHOR_WARNING` produces no HIR — the note is
/// pure author-facing signal — so this walk is the construct's entire
/// lowering; the diagnostic's message carries the note's text for the
/// Problems and TODO panels.
fn emit_author_warnings(sink: &mut EffectSink, node: &SyntaxNode, skip: SkipInsideKnots) {
    for warning in node.descendants().filter_map(ast::AuthorWarning::cast) {
        if skip == SkipInsideKnots::Yes
            && warning
                .syntax()
                .ancestors()
                .any(|a| a.kind() == SyntaxKind::KNOT_DEF)
        {
            continue;
        }
        let text = warning.text();
        let message = if text.is_empty() {
            "TODO".to_owned()
        } else {
            format!("TODO: {text}")
        };
        sink.diagnose_with_message(warning.syntax().text_range(), message, DiagnosticCode::E189);
    }
}

// ─── Source file ────────────────────────────────────────────────────

/// The declaration half of [`lower_source_file`] that precedes knots in
/// emission order: `VAR`/`CONST`/`LIST` (whole-tree — global regardless of
/// nesting), `STRUCT`, `EXTERNAL`, and `INCLUDE`. Split out (#3088) so
/// [`lower_declarations`] can harvest exactly these without lowering every
/// knot; [`lower_source_file`] composes it back in the identical order.
struct DeclHead {
    variables: Vec<crate::VarDecl>,
    constants: Vec<crate::ConstDecl>,
    lists: Vec<crate::ListDecl>,
    structs: Vec<crate::StructDecl>,
    externals: Vec<crate::ExternalDecl>,
    includes: Vec<IncludeSite>,
}

fn lower_decl_head(
    scope: &mut LowerScope,
    sink: &mut impl LowerSink,
    file: &ast::SourceFile,
) -> DeclHead {
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
    DeclHead {
        variables,
        constants,
        lists,
        structs,
        externals,
        includes,
    }
}

fn lower_source_file(
    scope: &mut LowerScope,
    sink: &mut impl LowerSink,
    file: &ast::SourceFile,
) -> HirFile {
    let head = lower_decl_head(scope, sink, file);
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
    assemble_hir_file(sink, file, head, knots, root_content)
}

/// The tail of [`lower_source_file`]: file-level module/`#@was`
/// arbitration, imports, and directive collections, assembled around the
/// already-lowered declarations and knots. Emission order is unchanged —
/// the module diagnostics still land after knot/root lowering exactly as
/// before the #3088 split.
fn assemble_hir_file(
    sink: &mut impl LowerSink,
    file: &ast::SourceFile,
    head: DeclHead,
    knots: Vec<Knot>,
    root_content: Block,
) -> HirFile {
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
        variables: head.variables,
        constants: head.constants,
        lists: head.lists,
        structs: head.structs,
        externals: head.externals,
        includes: head.includes,
        module,
        imports,
        visibility,
        was_directives,
        // The `@[allow(…)]` suppression channel (issue #1161) is native-only
        // — ink's `@[…]` placement is the top of a knot/stitch *body*, which
        // has no ruled `allow` tenant. Ink authors keep the line-scoped
        // `//brink-disable` comment channel and the project `[lints]` table.
        allow_scopes: Vec::new(),
        // Natural-notation element dispatch (issue #1838) is native-only for
        // the same reason: ink's grammar has no `@[element(claims = "…")]`
        // channel to claim a line with.
        element_matches: Vec::new(),
        // Same reason, extended to the harvest obligation (issue #2114):
        // ink's grammar has no `@NAME` cue channel to harvest.
        cue_names: Vec::new(),
        // This module *is* the ink frontend — see `HirFile::native`.
        native: false,
        // Same reason, extended to the declaration record (issue #1844):
        // ink has no claiming handlers to declare.
        claim_handlers: Vec::new(),
        // Same reason, extended to the dispatch declaration record (issue
        // #2352): ink has no `!name` sigil dispatch channel either.
        dispatch_handlers: Vec::new(),
    }
}
