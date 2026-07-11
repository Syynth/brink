//! Compiler for inkle's ink narrative scripting language.
//!
//! Orchestrates the full compilation pipeline: file discovery, parsing
//! (`brink-syntax`), HIR lowering (`brink-ir`), semantic analysis
//! (`brink-analyzer`), and codegen into the `brink-format` binary
//! representation consumed by `brink-runtime`.

mod driver;

pub use brink_driver::AnalysisOptions;
pub use brink_ir::{DiagnosticCode, FileId};

use brink_format::StoryData;
use std::io;
use std::path::Path;

/// A diagnostic resolved for consumption outside the compiler.
///
/// The internal [`Diagnostic`] keys a file by [`FileId`] — an interning index
/// that is only meaningful inside the compiler instance that produced it and
/// is not stable across recompiles. A consumer (an editor, a host integration,
/// an LSP) cannot map that id back to a file on its own. `ResolvedDiagnostic`
/// carries the file's `path` — byte-identical to the string the host used as
/// the entry point / answered the `read_file` callback with — so a diagnostic
/// can always be located. `file` is retained for in-result correlation only.
///
/// `range` is left as byte offsets into the file's source. Line/column
/// resolution is deliberately not baked in: column units are consumer-specific
/// (LSP uses UTF-16 code units, a terminal uses bytes or chars), and the
/// consumer already holds the source text to resolve them in the unit it needs.
#[derive(Debug, Clone)]
pub struct ResolvedDiagnostic {
    /// The file this diagnostic belongs to, keyed by its source path.
    pub path: String,
    /// The originating file's interning id — for in-result correlation only.
    pub file: FileId,
    /// The source span this diagnostic points at, as byte offsets.
    pub range: rowan::TextRange,
    /// Human-readable message describing the problem.
    pub message: String,
    /// Structured error code for documentation and tooling.
    pub code: DiagnosticCode,
}

/// Successful compilation output, including any non-fatal warnings.
#[derive(Debug)]
pub struct CompileOutput {
    pub data: StoryData,
    pub warnings: Vec<ResolvedDiagnostic>,
}

/// Compile an ink story from an entry-point file path.
///
/// Reads files from disk, follows INCLUDEs, and runs the full compilation
/// pipeline. Returns the compiled story data or a list of diagnostics.
pub fn compile_path(path: &Path) -> Result<CompileOutput, CompileError> {
    compile(path.to_string_lossy().as_ref(), |p| {
        std::fs::read_to_string(p).map_err(|e| io::Error::new(e.kind(), format!("{p}: {e}")))
    })
}

/// Compile an ink story with caller-provided file reading.
///
/// The `read_file` callback is called for the entry point and each
/// `INCLUDE`d file discovered during parsing. This enables compilation in
/// WASM, tests, and editor contexts where files are not on disk.
pub fn compile<F>(entry: &str, read_file: F) -> Result<CompileOutput, CompileError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    driver::compile_with_options(entry, read_file, AnalysisOptions::default())
}

/// Compile with explicit analysis options — e.g. a registered host-capability
/// manifest and external-check severity (the "compiler flag, error by
/// default"). Manifest-driven diagnostics are surfaced as compile warnings or
/// errors per the severity policy.
pub fn compile_with_options<F>(
    entry: &str,
    read_file: F,
    options: AnalysisOptions,
) -> Result<CompileOutput, CompileError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    driver::compile_with_options(entry, read_file, options)
}

/// Errors that can occur during compilation.
#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    /// File I/O error (missing file, permission denied, etc.).
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    /// One or more diagnostics prevented compilation.
    #[error("{} diagnostic(s) prevented compilation", .0.len())]
    Diagnostics(Vec<ResolvedDiagnostic>),
    /// Circular INCLUDE dependency detected.
    #[error("circular INCLUDE dependency: {0}")]
    CircularInclude(String),
}

impl From<brink_driver::DiscoverError> for CompileError {
    fn from(err: brink_driver::DiscoverError) -> Self {
        match err {
            brink_driver::DiscoverError::Io(e) => Self::Io(e),
            brink_driver::DiscoverError::CircularInclude(msg) => Self::CircularInclude(msg),
        }
    }
}
