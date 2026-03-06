//! Compiler for inkle's ink narrative scripting language.
//!
//! Orchestrates the full compilation pipeline: file discovery, parsing
//! (`brink-syntax`), HIR lowering (`brink-ir`), semantic analysis
//! (`brink-analyzer`), and codegen into the `brink-format` binary
//! representation consumed by `brink-runtime`.

mod driver;

pub use brink_ir::FileId;

use brink_format::StoryData;
use brink_ir::Diagnostic;
use std::io;
use std::path::Path;

/// Compile an ink story from an entry-point file path.
///
/// Reads files from disk, follows INCLUDEs, and runs the full compilation
/// pipeline. Returns the compiled story data or a list of diagnostics.
pub fn compile_path(path: &Path) -> Result<StoryData, CompileError> {
    compile(path.to_string_lossy().as_ref(), |p| {
        std::fs::read_to_string(p).map_err(|e| io::Error::new(e.kind(), format!("{p}: {e}")))
    })
}

/// Compile an ink story with caller-provided file reading.
///
/// The `read_file` callback is called for the entry point and each
/// `INCLUDE`d file discovered during parsing. This enables compilation in
/// WASM, tests, and editor contexts where files are not on disk.
pub fn compile<F>(entry: &str, read_file: F) -> Result<StoryData, CompileError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    driver::compile(entry, read_file)
}

/// Errors that can occur during compilation.
#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    /// File I/O error (missing file, permission denied, etc.).
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    /// One or more diagnostics prevented compilation.
    #[error("{} diagnostic(s) prevented compilation", .0.len())]
    Diagnostics(Vec<Diagnostic>),
}

impl From<brink_db::DiscoverError> for CompileError {
    fn from(err: brink_db::DiscoverError) -> Self {
        match err {
            brink_db::DiscoverError::Io(e) => Self::Io(e),
            brink_db::DiscoverError::CircularInclude(msg) => Self::Diagnostics(vec![Diagnostic {
                range: rowan::TextRange::default(),
                message: format!("circular INCLUDE dependency: {msg}"),
                code: brink_ir::DiagnosticCode::E028,
            }]),
        }
    }
}
