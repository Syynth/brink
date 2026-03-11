//! Compilation driver: file discovery, parsing, lowering, analysis, codegen.

use std::io;

use brink_db::ProjectDb;
use brink_format::StoryData;
use tracing::info;

use crate::CompileError;

/// Run the full compilation pipeline through LIR lowering.
fn compile_lir<F>(entry: &str, mut read_file: F) -> Result<brink_ir::lir::Program, CompileError>
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
    let mut all_diagnostics: Vec<brink_ir::Diagnostic> = db
        .file_ids()
        .flat_map(|id| {
            db.file_diagnostics(id)
                .unwrap_or_default()
                .iter()
                .filter(|d| d.code.severity() == brink_ir::Severity::Error)
                .cloned()
        })
        .collect();

    // ── Pass 3-5: Analyze ───────────────────────────────────────────
    let result = db.analyze().clone();

    info!(
        symbols = result.index.symbols.len(),
        diagnostics = result.diagnostics.len(),
        "analysis complete"
    );

    all_diagnostics.extend(result.diagnostics.clone());

    if !all_diagnostics.is_empty() {
        return Err(CompileError::Diagnostics(all_diagnostics));
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
    let program = brink_ir::lir::lower_to_program(&files, &result.index, &result.resolutions);

    info!(globals = program.globals.len(), "LIR lowering complete");

    Ok(program)
}

/// Compile to LIR — public for the JSON backend.
pub fn compile_to_lir<F>(entry: &str, read_file: F) -> Result<brink_ir::lir::Program, CompileError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    compile_lir(entry, read_file)
}

/// Run the full compilation pipeline.
pub fn compile<F>(entry: &str, read_file: F) -> Result<StoryData, CompileError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    let program = compile_lir(entry, read_file)?;

    // ── Pass 6b: Codegen ────────────────────────────────────────────
    Ok(brink_codegen_inkb::emit(&program))
}
