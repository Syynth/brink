//! Compilation driver: file discovery, parsing, lowering, analysis, codegen.

use std::io;

use brink_db::ProjectDb;
use tracing::info;

use crate::{CompileError, CompileOutput, LirOutput};

/// Run the full compilation pipeline through LIR lowering.
fn compile_lir<F>(entry: &str, mut read_file: F) -> Result<LirOutput, CompileError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    info!(entry, "starting compilation");

    // ── Pass 1-2: Discover, parse, and lower all files ──────────────
    let mut db = ProjectDb::new();
    db.discover(entry, &mut read_file)?;

    let file_count = db.file_ids().count();
    info!(file_count, "all files parsed and lowered");

    // ── Pass 2b: Collect per-file diagnostics (parse + HIR lowering) ──
    let mut all_warnings: Vec<brink_ir::Diagnostic> = Vec::new();
    let mut all_errors: Vec<brink_ir::Diagnostic> = Vec::new();

    // Check if the entry file has brink-disable-all
    let entry_id = db.file_id(entry);
    let disable_all = entry_id
        .and_then(|id| db.suppressions(id))
        .is_some_and(|s| s.disable_all);

    for id in db.file_ids() {
        let raw: Vec<brink_ir::Diagnostic> = db.file_diagnostics(id).unwrap_or_default().to_vec();
        let source = db.source(id).unwrap_or_default();
        let suppressions = db.suppressions(id).cloned().unwrap_or_default();
        let filtered = brink_ir::suppressions::apply_suppressions(id, source, raw, &suppressions);
        for d in filtered {
            if d.code.severity() == brink_ir::Severity::Error {
                all_errors.push(d);
            } else {
                all_warnings.push(d);
            }
        }
    }

    // ── Pass 3-5: Analyze ───────────────────────────────────────────
    let result = db.analyze().clone();

    info!(
        symbols = result.index.symbols.len(),
        diagnostics = result.diagnostics.len(),
        "analysis complete"
    );

    if !disable_all {
        // Group analysis diagnostics by file and apply per-file suppressions
        let mut by_file: std::collections::HashMap<brink_ir::FileId, Vec<brink_ir::Diagnostic>> =
            std::collections::HashMap::new();
        for d in &result.diagnostics {
            by_file.entry(d.file).or_default().push(d.clone());
        }
        for (fid, diags) in by_file {
            let source = db.source(fid).unwrap_or_default();
            let suppressions = db.suppressions(fid).cloned().unwrap_or_default();
            let filtered =
                brink_ir::suppressions::apply_suppressions(fid, source, diags, &suppressions);
            for d in filtered {
                if d.code.severity() == brink_ir::Severity::Error {
                    all_errors.push(d);
                } else {
                    all_warnings.push(d);
                }
            }
        }
    }

    if !all_errors.is_empty() {
        // Include warnings alongside errors so callers can see both.
        all_errors.extend(all_warnings);
        return Err(CompileError::Diagnostics(all_errors));
    }

    // ── Pass 6a: Build LIR ────────────────────────────────────────
    let entry_id = db.file_id(entry).ok_or_else(|| {
        CompileError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            format!("entry file not found after discovery: {entry}"),
        ))
    })?;
    let files: Vec<_> = db
        .file_ids_topo(entry_id)
        .into_iter()
        .filter_map(|id| db.hir(id).map(|hir| (id, hir)))
        .collect();
    let file_paths: std::collections::HashMap<_, _> = files
        .iter()
        .filter_map(|(id, _)| db.file_path(*id).map(|p| (*id, p.to_string())))
        .collect();
    let (program, lir_warnings) =
        brink_ir::lir::lower_to_program(&files, &result.index, &result.resolutions, &file_paths);

    all_warnings.extend(lir_warnings);

    info!(globals = program.globals.len(), "LIR lowering complete");

    Ok(LirOutput {
        program,
        warnings: all_warnings,
    })
}

/// Compile to LIR — public for the JSON backend.
pub fn compile_to_lir<F>(entry: &str, read_file: F) -> Result<LirOutput, CompileError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    compile_lir(entry, read_file)
}

/// Run the full compilation pipeline.
pub fn compile<F>(entry: &str, read_file: F) -> Result<CompileOutput, CompileError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    let lir_output = compile_lir(entry, read_file)?;

    // ── Pass 6b: Codegen ────────────────────────────────────────────
    Ok(CompileOutput {
        data: brink_codegen_inkb::emit(&lir_output.program),
        warnings: lir_output.warnings,
    })
}
