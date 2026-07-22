//! Compilation driver: file discovery, parsing, lowering, analysis, codegen.

use std::io;
use std::sync::Arc;

use brink_driver::{AnalysisOptions, Driver};
use brink_ir::Diagnostic;
use tracing::info;

use crate::{CompileError, CompileOutput, ResolvedDiagnostic};

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

/// Run the full compilation pipeline with explicit analysis options — e.g. a
/// registered host-capability manifest and its external-check severity, so
/// manifest-driven diagnostics surface in the compile output.
///
/// FG-6 (#841, "single compile pipeline"): pulls the memoized `story_data`
/// query — the one canonical codegen site — rather than pulling `lir` and
/// running `brink_codegen_inkb::emit` a second time here. Every batch caller
/// (CLI, brink-web, intl, the oracle harness) reaches codegen through this
/// query now, so there is exactly one `emit` call on the compile path and no
/// way for a driver-local emit to drift from the query's. The owned
/// `StoryData` that `CompileOutput` needs is unwrapped from the memoized
/// `Arc` — deep-cloned when the memo still holds a reference (the ordinary
/// one-shot case). That fixed clone cost is the accepted, measured price of
/// collapsing the two pipelines into one (issue #841 gate note); it is not
/// hidden.
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
        data: Arc::unwrap_or_clone(story),
        warnings: resolve_diagnostics(&driver, product.warnings),
    })
}
