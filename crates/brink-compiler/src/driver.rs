//! Compilation driver: file discovery, parsing, lowering, analysis, codegen.

use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;

use brink_driver::{AnalysisOptions, Driver, RealFs};
use brink_ir::Diagnostic;
use tracing::info;

use crate::{CompileError, CompileOutput, ResolvedDiagnostic};

/// Resolve a `FileId`-keyed [`Diagnostic`] to a [`ResolvedDiagnostic`] carrying
/// the file's source path. Must run while `driver` is still alive, since it
/// owns the `FileId`→path map. An id with no known path (which should not
/// happen for a diagnostic produced from a discovered file) resolves to an
/// empty path rather than dropping the diagnostic.
///
/// `severity` is resolved through `brink_driver::effective_severity` — the
/// project's actual `types`/`[lints]` policy, read off `driver`'s own
/// `AnalysisOptions` — not the code's raw default (issue #1162: a `[lints]`
/// code down-leveled to `Info`/`Hint` must carry that severity here, not
/// `Warning`, since this is what `CompileOutput::warnings`/`CompileError::
/// Diagnostics` — and in turn the CLI renderer — reads).
fn resolve_diagnostics(driver: &Driver, diags: Vec<Diagnostic>) -> Vec<ResolvedDiagnostic> {
    let db = driver.db();
    let opts = db.analysis_options();
    let types = opts.type_policy();
    diags
        .into_iter()
        .map(|d| ResolvedDiagnostic {
            path: db.file_path(d.file).unwrap_or_default().to_string(),
            file: d.file,
            range: d.range,
            message: d.message,
            severity: brink_driver::effective_severity(d.code, types, &opts.lints),
            code: d.code,
        })
        .collect()
}

/// Discover the project and point the db's queries at `entry`.
///
/// Batch compilation is now query-shaped (scripting-substrate spec §5): this
/// sets the layer-0 inputs (file texts, entry, analysis options); the caller
/// pulls the query it needs (`lir_product` or `story_data`).
///
/// Dispatches on `entry`'s extension (B0.10b, issue #1288): a `.brink` entry
/// discovers via [`brink_driver::Driver::discover_native`] — a `RealFs`
/// walk of the project's source root (a `brink.toml` walk-up, or the
/// entry's own directory if none exists — the single-file-project ruling),
/// `read_file` unused since native has no per-caller content-provider
/// need (the whole tree is read directly off disk). A `.ink` entry still
/// discovers via `read_file`-driven `INCLUDE` BFS, registering files under
/// whatever spelling the caller passed — but also registers that same
/// `brink.toml`-walk-up root (issue #1696) via `ProjectDb::set_ink_root`, so
/// [`hir::root_content_scope_path`](brink_ir::hir::root_content_scope_path)'s
/// qualifier is a root-relative key instead of the raw spelling.
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

    let entry_key = if brink_driver::is_native(Path::new(entry)) {
        let (root, warnings) = brink_driver::native_source_root_with_warnings(Path::new(entry));
        for warning in &warnings {
            let _ = writeln!(io::stderr(), "warning: {warning}");
        }
        let tree = RealFs::new(&root);
        driver.discover_native(&tree)?;
        brink_driver::relative_key(&root, Path::new(entry))
    } else {
        driver.discover(entry, read_file)?;
        // #1696: register the ink project root — the same `brink.toml`
        // walk-up (or entry's own directory, single-file-project ruling)
        // native compiles already use — so `hir::root_content_scope_path`'s
        // qualifier (read through `ProjectDb::ink_root`) is a root-relative
        // key rather than `entry`'s raw spelling. `main.ink`, `./main.ink`,
        // and an absolute spelling of the same file now mint the same
        // anonymous root-content `DefinitionId`s. A no-op for every
        // downstream reader when the registered root strips to nothing
        // (`root_relative_key` leaves an unrelated path unchanged), so this
        // never changes behavior for a project whose entry was already bare.
        let (root, warnings) = brink_driver::native_source_root_with_warnings(Path::new(entry));
        for warning in &warnings {
            let _ = writeln!(io::stderr(), "warning: {warning}");
        }
        driver
            .db_mut()
            .set_ink_root(Some(root.to_string_lossy().into_owned()));
        entry.to_string()
    };

    let file_count = driver.db().file_ids().count();
    info!(file_count, "all files discovered");

    let entry_id = driver.db_mut().set_entry(&entry_key).ok_or_else(|| {
        CompileError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            format!("entry file not found after discovery: {entry_key}"),
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
