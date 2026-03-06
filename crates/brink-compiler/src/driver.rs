//! Compilation driver: file discovery, parsing, lowering, analysis, codegen.

use std::collections::{HashMap, HashSet};
use std::io;

use brink_analyzer::AnalysisResult;
use brink_format::StoryData;
use brink_ir::{FileId, HirFile, SymbolManifest};
use tracing::{debug, info};

use crate::CompileError;

/// Run the full compilation pipeline.
pub fn compile<F>(entry: &str, mut read_file: F) -> Result<StoryData, CompileError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    info!(entry, "starting compilation");

    // ── Pass 1-2: Discover, parse, and lower all files ──────────────
    let files = discover_and_lower(entry, &mut read_file)?;

    info!(file_count = files.len(), "all files parsed and lowered");

    // ── Pass 3-5: Analyze ───────────────────────────────────────────
    let file_count = files.len();
    let AnalysisResult {
        index,
        resolutions: _resolutions,
        diagnostics,
    } = brink_analyzer::analyze(files);

    info!(
        symbols = index.symbols.len(),
        diagnostics = diagnostics.len(),
        "analysis complete"
    );

    if !diagnostics.is_empty() {
        return Err(CompileError::Diagnostics(diagnostics));
    }

    // ── Pass 6: Codegen ─────────────────────────────────────────────
    // TODO: Build LIR from HIR + SymbolIndex, then run codegen backend.
    info!(
        files = file_count,
        "codegen stub — returning empty StoryData"
    );

    Ok(StoryData {
        containers: Vec::new(),
        line_tables: Vec::new(),
        variables: Vec::new(),
        list_defs: Vec::new(),
        list_items: Vec::new(),
        externals: Vec::new(),
        labels: Vec::new(),
        name_table: Vec::new(),
        list_literals: Vec::new(),
    })
}

/// Discover all files (following INCLUDEs), parse, and lower each.
///
/// Returns the full set of `(FileId, HirFile, SymbolManifest)` tuples
/// ready for the analyzer.
fn discover_and_lower<F>(
    entry: &str,
    read_file: &mut F,
) -> Result<Vec<(FileId, HirFile, SymbolManifest)>, CompileError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    let mut results: Vec<(FileId, HirFile, SymbolManifest)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = vec![entry.to_string()];
    let mut file_id_map: HashMap<String, FileId> = HashMap::new();
    let mut next_id: u32 = 0;

    while let Some(path) = queue.pop() {
        if !seen.insert(path.clone()) {
            debug!(path, "skipping already-seen file");
            continue;
        }

        let source = read_file(&path)?;

        let file_id = *file_id_map.entry(path.clone()).or_insert_with(|| {
            let id = FileId(next_id);
            next_id += 1;
            id
        });

        // Parse + Lower
        let parsed = brink_syntax::parse(&source);
        let parse_errors = parsed.errors().len();
        let (hir, manifest, lowering_diagnostics) = brink_ir::lower(&parsed.tree());

        let knots = hir.knots.len();
        let includes = hir.includes.len();
        let sym_count = manifest.knots.len()
            + manifest.stitches.len()
            + manifest.variables.len()
            + manifest.lists.len()
            + manifest.externals.len();
        let unresolved = manifest.unresolved.len();

        info!(
            path,
            file_id = file_id.0,
            parse_errors,
            lowering_diagnostics = lowering_diagnostics.len(),
            knots,
            includes,
            symbols = sym_count,
            unresolved,
            "processed file"
        );

        // Discover INCLUDEs and add to queue
        for include in &hir.includes {
            let included_path = resolve_include_path(&path, &include.file_path);
            if !seen.contains(&included_path) {
                debug!(from = path, include = included_path, "discovered INCLUDE");
                queue.push(included_path);
            }
        }

        results.push((file_id, hir, manifest));
    }

    Ok(results)
}

/// Resolve an INCLUDE path relative to the including file's directory.
fn resolve_include_path(from_file: &str, include_path: &str) -> String {
    if let Some(dir) = std::path::Path::new(from_file).parent() {
        dir.join(include_path).to_string_lossy().into_owned()
    } else {
        include_path.to_string()
    }
}
