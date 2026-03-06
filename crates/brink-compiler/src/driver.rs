//! Compilation driver: file discovery, parsing, lowering, analysis, codegen.

use std::io;

use brink_db::ProjectDb;
use brink_format::StoryData;
use tracing::info;

use crate::CompileError;

/// Run the full compilation pipeline.
pub fn compile<F>(entry: &str, mut read_file: F) -> Result<StoryData, CompileError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    info!(entry, "starting compilation");

    // ── Pass 1-2: Discover, parse, and lower all files ──────────────
    let mut db = ProjectDb::new();
    db.discover(entry, &mut read_file)?;

    let file_count = db.file_ids().count();
    info!(file_count, "all files parsed and lowered");

    // ── Pass 3-5: Analyze ───────────────────────────────────────────
    let result = db.analyze().clone();

    info!(
        symbols = result.index.symbols.len(),
        diagnostics = result.diagnostics.len(),
        "analysis complete"
    );

    if !result.diagnostics.is_empty() {
        return Err(CompileError::Diagnostics(result.diagnostics));
    }

    // ── Pass 6a: Build LIR ────────────────────────────────────────
    let files: Vec<_> = db
        .file_ids()
        .filter_map(|id| db.hir(id).map(|hir| (id, hir)))
        .collect();
    let program = brink_ir::lir::lower_to_program(&files, &result.index, &result.resolutions);

    info!(
        containers = program.containers.len(),
        globals = program.globals.len(),
        "LIR lowering complete"
    );

    // ── Pass 6b: Codegen ────────────────────────────────────────────
    // TODO: bytecode::emit(&program) or json::emit(&program)
    // For now, return an empty StoryData — backends are a separate task.
    let _ = program;

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
