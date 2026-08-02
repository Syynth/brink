//! Shared test harness for `lir_lowering`'s per-feature-area submodules.
//!
//! Moved verbatim out of the former monolithic `tests/lir_lowering.rs`
//! (issue #689) — every helper here is used by at least one sibling
//! module, so it lives in its own `pub(crate)` module rather than being
//! duplicated per file.

use brink_ir::lir;
use brink_ir::{FileId, HirFile, SymbolManifest};

/// Parse ink source → HIR lower → analyze → LIR lower. Returns the full Program.
pub(crate) fn lower_ink(source: &str) -> lir::Program {
    let (program, _warnings) = lower_ink_with_warnings(source);
    // unwrap: `lower_to_program` only returns `None` when the
    // residual-extension backstop fires (E053) — this helper's callers all
    // pass plain ink, so this should never be `None`.
    program.unwrap()
}

/// Parse ink source → HIR lower → analyze → LIR lower. Returns program + warnings.
pub(crate) fn lower_ink_with_warnings(
    source: &str,
) -> (Option<lir::Program>, Vec<brink_ir::Diagnostic>) {
    lower_ink_with_type_mode(source, lir::TypeMode::Gradual)
}

/// [`lower_ink_with_warnings`] with an explicit `types` policy — TM-4c
/// (#666) tests exercising the strict-only static-offset path need this;
/// every other caller gets the gradual default.
pub(crate) fn lower_ink_with_type_mode(
    source: &str,
    type_mode: lir::TypeMode,
) -> (Option<lir::Program>, Vec<brink_ir::Diagnostic>) {
    let parsed = brink_syntax::parse(source);
    let tree = parsed.tree();
    let file_id = FileId(0);
    let (mut hir, manifest, _diags) = brink_ir::hir::lower(file_id, &tree);

    // Normalize HIR (lift inline sequences/conditionals) — mirrors what
    // `lower_to_program` does internally so the test pipeline is consistent.
    brink_ir::hir::normalize_file(&mut hir);

    let files_for_analysis: Vec<(FileId, &HirFile, &SymbolManifest)> =
        vec![(file_id, &hir, &manifest)];
    let result = brink_analyzer::analyze(&files_for_analysis);

    let files_for_lir: Vec<(FileId, &HirFile)> = vec![(file_id, &hir)];
    lir::lower_to_program_with_type_mode(
        &files_for_lir,
        &result.index,
        &result.resolutions,
        &std::collections::HashMap::new(),
        type_mode,
        lir::AnalyzerTables {
            ufcs: &lir::UfcsLookup::new(),
            coalesce: &lir::CoalesceLookup::new(),
        },
    )
}

/// [`lower_ink`]'s native-dialect twin — parse `.brink` source → native HIR
/// lower → analyze → LIR lower. Issue #1774's decl-default lambda literal
/// tests need this: the feature is native-only (lambdas and bare-name fn
/// references are both native-surface constructs, #1685/#1862), so the ink
/// helpers above never reach it.
pub(crate) fn lower_native(source: &str) -> (Option<lir::Program>, Vec<brink_ir::Diagnostic>) {
    let parsed = brink_syntax_native::parse(source);
    let tree = parsed.tree();
    let file_id = FileId(0);
    let (mut hir, manifest, _diags) = brink_ir::hir::lower_native::lower(file_id, &tree);

    // Same normalize step `lower_ink_with_type_mode` does — dialect-agnostic.
    brink_ir::hir::normalize_file(&mut hir);

    let files_for_analysis: Vec<(FileId, &HirFile, &SymbolManifest)> =
        vec![(file_id, &hir, &manifest)];
    let result = brink_analyzer::analyze(&files_for_analysis);

    let files_for_lir: Vec<(FileId, &HirFile)> = vec![(file_id, &hir)];
    lir::lower_to_program_with_type_mode(
        &files_for_lir,
        &result.index,
        &result.resolutions,
        &std::collections::HashMap::new(),
        lir::TypeMode::Gradual,
        lir::AnalyzerTables {
            ufcs: &lir::UfcsLookup::new(),
            coalesce: &lir::CoalesceLookup::new(),
        },
    )
}

/// Parse and lower a multi-file project → LIR, mirroring [`lower_ink`] for a
/// project with `INCLUDE`s (issue #1502).
///
/// `sources` must be in the same **topological include order** the real
/// pipeline hands to `lower_to_program_with_type_mode` — included files first,
/// the entry file last (`IncludeGraph::topological_order` is a post-order DFS
/// from the entry, so the entry is always the final element). The `INCLUDE`
/// line itself is not interpreted here: discovery already happened by the time
/// LIR lowering runs, so the entry source may carry it purely for readability.
pub(crate) fn lower_ink_files(sources: &[&str]) -> lir::Program {
    let lowered: Vec<(FileId, HirFile, SymbolManifest)> = sources
        .iter()
        .enumerate()
        .map(|(i, source)| {
            // `usize as u32`: test sources, never more than a handful.
            let file_id = FileId(u32::try_from(i).unwrap());
            let parsed = brink_syntax::parse(source);
            let (mut hir, manifest, _diags) = brink_ir::hir::lower(file_id, &parsed.tree());
            brink_ir::hir::normalize_file(&mut hir);
            (file_id, hir, manifest)
        })
        .collect();

    let files_for_analysis: Vec<(FileId, &HirFile, &SymbolManifest)> = lowered
        .iter()
        .map(|(id, hir, manifest)| (*id, hir, manifest))
        .collect();
    let result = brink_analyzer::analyze(&files_for_analysis);

    let files_for_lir: Vec<(FileId, &HirFile)> =
        lowered.iter().map(|(id, hir, _)| (*id, hir)).collect();
    let (program, _diags) = lir::lower_to_program_with_type_mode(
        &files_for_lir,
        &result.index,
        &result.resolutions,
        &std::collections::HashMap::new(),
        lir::TypeMode::Gradual,
        lir::AnalyzerTables {
            ufcs: &lir::UfcsLookup::new(),
            coalesce: &lir::CoalesceLookup::new(),
        },
    );
    program.unwrap()
}

/// [`lower_ink_files`] with an explicit per-file path map, mirroring what
/// the real pipeline supplies (`chunk_lowering_ctx_query`,
/// `brink-db/src/queries/mod.rs:1838`, populates `file_paths` from each
/// file's real registered path). `lower_ink_files` always hands lowering an
/// *empty* map, so it cannot exercise anything keyed on a file's path or
/// module identity — this variant is for tests that need file identity to
/// actually differ between sources (e.g. #1504's root-content qualification).
pub(crate) fn lower_ink_files_with_paths(sources: &[(&str, &str)]) -> lir::Program {
    let lowered: Vec<(FileId, HirFile, SymbolManifest)> = sources
        .iter()
        .enumerate()
        .map(|(i, (_, source))| {
            // `usize as u32`: test sources, never more than a handful.
            let file_id = FileId(u32::try_from(i).unwrap());
            let parsed = brink_syntax::parse(source);
            let (mut hir, manifest, _diags) = brink_ir::hir::lower(file_id, &parsed.tree());
            brink_ir::hir::normalize_file(&mut hir);
            (file_id, hir, manifest)
        })
        .collect();

    let files_for_analysis: Vec<(FileId, &HirFile, &SymbolManifest)> = lowered
        .iter()
        .map(|(id, hir, manifest)| (*id, hir, manifest))
        .collect();
    let result = brink_analyzer::analyze(&files_for_analysis);

    let file_paths: std::collections::HashMap<FileId, String> = sources
        .iter()
        .enumerate()
        .map(|(i, (path, _))| (FileId(u32::try_from(i).unwrap()), (*path).to_string()))
        .collect();

    let files_for_lir: Vec<(FileId, &HirFile)> =
        lowered.iter().map(|(id, hir, _)| (*id, hir)).collect();
    let (program, _diags) = lir::lower_to_program_with_type_mode(
        &files_for_lir,
        &result.index,
        &result.resolutions,
        &file_paths,
        lir::TypeMode::Gradual,
        lir::AnalyzerTables {
            ufcs: &lir::UfcsLookup::new(),
            coalesce: &lir::CoalesceLookup::new(),
        },
    );
    program.unwrap()
}

/// [`lower_native`]'s multi-file twin, with a real per-file path map — the
/// native-dialect analogue of [`lower_ink_files_with_paths`], needed for
/// issue #1774's #1504 collision-avoidance test (two files' lambda-literal
/// decl defaults at the same source offset).
pub(crate) fn lower_native_files_with_paths(sources: &[(&str, &str)]) -> lir::Program {
    let lowered: Vec<(FileId, HirFile, SymbolManifest)> = sources
        .iter()
        .enumerate()
        .map(|(i, (_, source))| {
            // `usize as u32`: test sources, never more than a handful.
            let file_id = FileId(u32::try_from(i).unwrap());
            let parsed = brink_syntax_native::parse(source);
            let (mut hir, manifest, _diags) =
                brink_ir::hir::lower_native::lower(file_id, &parsed.tree());
            brink_ir::hir::normalize_file(&mut hir);
            (file_id, hir, manifest)
        })
        .collect();

    let files_for_analysis: Vec<(FileId, &HirFile, &SymbolManifest)> = lowered
        .iter()
        .map(|(id, hir, manifest)| (*id, hir, manifest))
        .collect();
    let result = brink_analyzer::analyze(&files_for_analysis);

    let file_paths: std::collections::HashMap<FileId, String> = sources
        .iter()
        .enumerate()
        .map(|(i, (path, _))| (FileId(u32::try_from(i).unwrap()), (*path).to_string()))
        .collect();

    let files_for_lir: Vec<(FileId, &HirFile)> =
        lowered.iter().map(|(id, hir, _)| (*id, hir)).collect();
    let (program, _diags) = lir::lower_to_program_with_type_mode(
        &files_for_lir,
        &result.index,
        &result.resolutions,
        &file_paths,
        lir::TypeMode::Gradual,
        lir::AnalyzerTables {
            ufcs: &lir::UfcsLookup::new(),
            coalesce: &lir::CoalesceLookup::new(),
        },
    );
    program.unwrap()
}

/// Get the root container.
pub(crate) fn root(program: &lir::Program) -> &lir::Container {
    &program.root
}

/// Find a direct child of a container by name.
pub(crate) fn find_child<'a>(container: &'a lir::Container, name: &str) -> &'a lir::Container {
    container
        .children
        .iter()
        .find(|c| c.name.as_deref() == Some(name))
        .unwrap_or_else(|| {
            let names: Vec<Option<&str>> = container
                .children
                .iter()
                .map(|c| c.name.as_deref())
                .collect();
            panic!("no child named {name:?}, available: {names:?}")
        })
}

/// Find a container by dot-separated path from root.
pub(crate) fn find_by_path<'a>(program: &'a lir::Program, path: &str) -> &'a lir::Container {
    if path.is_empty() {
        return &program.root;
    }
    let mut current = &program.root;
    for segment in path.split('.') {
        current = find_child(current, segment);
    }
    current
}

/// Find a global by checking if its name matches via the name table.
pub(crate) fn find_global<'a>(program: &'a lir::Program, name: &str) -> &'a lir::GlobalDef {
    program
        .globals
        .iter()
        .find(|g| program.name_table[g.name.0 as usize] == name)
        .unwrap_or_else(|| panic!("no global named {name:?}"))
}

/// The `ShapeId` the project's shape table assigned to the `STRUCT` named
/// `name` — so a test can assert a folded [`lir::ConstValue::Record`] names
/// the right shape without hard-coding the dense id.
pub(crate) fn shape_id_of(program: &lir::Program, name: &str) -> u32 {
    program
        .struct_shapes
        .iter()
        .find(|s| program.name_table[s.name.0 as usize] == name)
        .unwrap_or_else(|| panic!("no STRUCT named {name:?}"))
        .id
}

/// Recursively count containers of a given kind in the tree.
pub(crate) fn count_kind(container: &lir::Container, kind: lir::ContainerKind) -> usize {
    let mut count = usize::from(container.kind == kind);
    for child in &container.children {
        count += count_kind(child, kind);
    }
    count
}

/// Count all containers in the tree (including the root itself).
pub(crate) fn count_all(container: &lir::Container) -> usize {
    1 + container.children.iter().map(count_all).sum::<usize>()
}

/// Extract text from `EmitContent` statements.
pub(crate) fn collect_text(stmts: &[lir::Stmt]) -> Vec<String> {
    let mut texts = Vec::new();
    for stmt in stmts {
        match stmt {
            lir::Stmt::EmitContent(content) => {
                let mut line = String::new();
                for part in &content.parts {
                    if let lir::ContentPart::Text(t) = part {
                        line.push_str(t);
                    }
                }
                if !line.is_empty() {
                    texts.push(line);
                }
            }
            lir::Stmt::EmitLine(emission) => match &emission.line {
                lir::RecognizedLine::Plain(s) => {
                    if !s.is_empty() {
                        texts.push(s.clone());
                    }
                }
                lir::RecognizedLine::Template { parts, .. } => {
                    let mut line = String::new();
                    for part in parts {
                        if let brink_format::LinePart::Literal(s) = part {
                            line.push_str(s);
                        }
                    }
                    if !line.is_empty() {
                        texts.push(line);
                    }
                }
            },
            _ => {}
        }
    }
    texts
}

/// Check if a statement list ends with a divert.
pub(crate) fn ends_with_divert(stmts: &[lir::Stmt]) -> bool {
    stmts
        .last()
        .is_some_and(|s| matches!(s, lir::Stmt::Divert(_)))
}

/// Recursively find any container matching a predicate.
pub(crate) fn find_any<'a>(
    container: &'a lir::Container,
    pred: &dyn Fn(&lir::Container) -> bool,
) -> Option<&'a lir::Container> {
    if pred(container) {
        return Some(container);
    }
    for child in &container.children {
        if let Some(found) = find_any(child, pred) {
            return Some(found);
        }
    }
    None
}

/// Collect all containers of a given kind from the tree.
pub(crate) fn collect_kind(
    container: &lir::Container,
    kind: lir::ContainerKind,
) -> Vec<&lir::Container> {
    let mut result = Vec::new();
    if container.kind == kind {
        result.push(container);
    }
    for child in &container.children {
        result.extend(collect_kind(child, kind));
    }
    result
}

// The three helpers below were originally defined inline in a single
// section each, but their call sites span multiple of the post-split
// feature-area files (`find_diag` and `find_code` are literal duplicates
// of each other, both used across their own several sections; `find_e058`
// likewise) — moved here verbatim so every consumer keeps working via the
// shared `use crate::support::*;` import, per issue #689's "no assertion
// changes" discipline (not deduplicated, just relocated).

/// Used by `structs_lir_and_codegen`, `block_scoped_temp_read_after_block_closes`,
/// `t1e_1_path_projections`, and `t1e_2_path_projections`.
pub(crate) fn find_diag(
    diags: &[brink_ir::Diagnostic],
    code: brink_ir::DiagnosticCode,
) -> Option<&brink_ir::Diagnostic> {
    diags.iter().find(|d| d.code == code)
}

/// Used by `collection_mutator_arity_mismatch` and `rand_verb_surface`.
pub(crate) fn find_e058(diags: &[brink_ir::Diagnostic]) -> Option<&brink_ir::Diagnostic> {
    diags
        .iter()
        .find(|d| d.code == brink_ir::DiagnosticCode::E058)
}

/// Used by `rand_verb_surface` and `range_values`.
pub(crate) fn find_code(
    diags: &[brink_ir::Diagnostic],
    code: brink_ir::DiagnosticCode,
) -> Option<&brink_ir::Diagnostic> {
    diags.iter().find(|d| d.code == code)
}
