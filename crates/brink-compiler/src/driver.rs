//! Compilation driver: file discovery, parsing, lowering, analysis, codegen.

use std::io;

use brink_driver::Driver;
use tracing::info;

use crate::{CompileError, CompileOutput, LirOutput};

/// Run the full compilation pipeline through LIR lowering.
fn compile_lir<F>(entry: &str, read_file: F) -> Result<LirOutput, CompileError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    info!(entry, "starting compilation");

    // ── Pass 1-2: Discover, parse, and lower all files ──────────────
    let mut driver = Driver::new();
    driver.discover(entry, read_file)?;

    let file_count = driver.db().file_ids().count();
    info!(file_count, "all files parsed and lowered");

    // ── Pass 3-5: Analyze ───────────────────────────────────────────
    let analysis = driver.analyze().clone();

    info!(
        symbols = analysis.index.symbols.len(),
        diagnostics = analysis.diagnostics.len(),
        "analysis complete"
    );

    // ── Collect and partition diagnostics ────────────────────────────
    let entry_id = driver.db().file_id(entry).ok_or_else(|| {
        CompileError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            format!("entry file not found after discovery: {entry}"),
        ))
    })?;

    let report = driver.collect_diagnostics(&analysis, Some(entry_id));

    if !report.errors.is_empty() {
        let mut all = report.errors;
        all.extend(report.warnings);
        return Err(CompileError::Diagnostics(all));
    }

    // ── Pass 6a: Build LIR ────────────────────────────────────────
    let (files, file_paths) = driver.lir_inputs(entry_id);
    let (program, lir_warnings) = brink_ir::lir::lower_to_program(
        &files,
        &analysis.index,
        &analysis.resolutions,
        &file_paths,
    );

    let mut warnings = report.warnings;
    warnings.extend(lir_warnings);

    info!(globals = program.globals.len(), "LIR lowering complete");

    Ok(LirOutput { program, warnings })
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
