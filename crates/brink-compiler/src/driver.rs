//! Compilation driver: file discovery, parsing, lowering, analysis, codegen.

use std::io;

use brink_driver::{AnalysisOptions, Driver};
use brink_ir::Diagnostic;
use tracing::info;

use crate::{CompileError, CompileOutput, LirOutput, ResolvedDiagnostic};

/// Resolve a `FileId`-keyed [`Diagnostic`] to a [`ResolvedDiagnostic`] carrying
/// the file's source path. Must run while `driver` is still alive, since it
/// owns the `FileId`→path map. An id with no known path (which should not
/// happen for a diagnostic produced from a discovered file) resolves to an
/// empty path rather than dropping the diagnostic.
fn resolve_diagnostics(driver: &Driver, diags: Vec<Diagnostic>) -> Vec<ResolvedDiagnostic> {
    let db = driver.db();
    diags
        .into_iter()
        .map(|d| ResolvedDiagnostic {
            path: db.file_path(d.file).unwrap_or_default().to_string(),
            file: d.file,
            range: d.range,
            message: d.message,
            code: d.code,
        })
        .collect()
}

/// Discover the project and point the db's queries at `entry`.
///
/// Batch compilation is now query-shaped (scripting-substrate spec §5): this
/// sets the layer-0 inputs (file texts, entry, analysis options); the caller
/// pulls the query it needs (`lir_product` or `story_data`).
fn prepare_driver<F>(
    entry: &str,
    read_file: F,
    options: AnalysisOptions,
) -> Result<(Driver, brink_ir::FileId), CompileError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    info!(entry, "starting compilation");

    let mut driver = Driver::new();
    driver.set_analysis_options(options);
    driver.discover(entry, read_file)?;

    let file_count = driver.db().file_ids().count();
    info!(file_count, "all files discovered");

    let entry_id = driver.db_mut().set_entry(entry).ok_or_else(|| {
        CompileError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            format!("entry file not found after discovery: {entry}"),
        ))
    })?;
    Ok((driver, entry_id))
}

/// Run the full compilation pipeline through LIR lowering.
fn compile_lir<F>(
    entry: &str,
    read_file: F,
    options: AnalysisOptions,
) -> Result<LirOutput, CompileError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    let (driver, _entry_id) = prepare_driver(entry, read_file, options)?;

    let product = driver.db().lir_product().cloned().unwrap_or_default();

    let Some(program) = product.program else {
        let mut all = product.errors;
        all.extend(product.warnings);
        return Err(CompileError::Diagnostics(resolve_diagnostics(&driver, all)));
    };

    info!(globals = program.globals.len(), "LIR lowering complete");

    // Resolve FileId→path at the boundary, while the driver's map is alive.
    let warnings = resolve_diagnostics(&driver, product.warnings);
    Ok(LirOutput {
        program: std::sync::Arc::unwrap_or_clone(program),
        warnings,
    })
}

/// Compile to LIR — public for the JSON backend.
pub fn compile_to_lir<F>(entry: &str, read_file: F) -> Result<LirOutput, CompileError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    compile_lir(entry, read_file, AnalysisOptions::default())
}

/// Run the full compilation pipeline.
pub fn compile<F>(entry: &str, read_file: F) -> Result<CompileOutput, CompileError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    compile_with_options(entry, read_file, AnalysisOptions::default())
}

/// Run the full compilation pipeline with explicit analysis options — e.g. a
/// registered host-capability manifest and its external-check severity, so
/// manifest-driven diagnostics surface in the compile output.
///
/// Batch compile = pull the `story_data` query (spec §5).
pub fn compile_with_options<F>(
    entry: &str,
    read_file: F,
    options: AnalysisOptions,
) -> Result<CompileOutput, CompileError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    let (driver, _entry_id) = prepare_driver(entry, read_file, options)?;

    let product = driver.db().story_data().cloned().unwrap_or_default();

    let Some(story) = product.story else {
        let mut all = product.errors;
        all.extend(product.warnings);
        return Err(CompileError::Diagnostics(resolve_diagnostics(&driver, all)));
    };

    Ok(CompileOutput {
        data: std::sync::Arc::unwrap_or_clone(story),
        warnings: resolve_diagnostics(&driver, product.warnings),
    })
}
