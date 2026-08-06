//! Compiler for inkle's ink narrative scripting language.
//!
//! Orchestrates the full compilation pipeline: file discovery, parsing
//! (`brink-syntax`), HIR lowering (`brink-ir`), semantic analysis
//! (`brink-analyzer`), and codegen into the `brink-format` binary
//! representation consumed by `brink-runtime`.
//!
//! ## The `test-util`-gated entry points (issue #2168)
//!
//! `brink_environment::compile(&Environment)` (#1306) is the ruled
//! determinism boundary and the **sole production road** into compilation —
//! `brink-cli` and `brink-web` both go through it. This crate's `compile`,
//! `compile_path`, `compile_with_options`, and `compile_path_with_options`
//! take a `read_file` closure (or read straight off disk) and bypass
//! `Environment` entirely, so once stdlib source is mounted into the
//! `Environment` manifest (#2080), anything reached through them will not
//! see the stdlib — the story compiles and conventions silently do not
//! classify.
//!
//! Every call site of these four functions is test/bench/example code
//! compiling an inline or fixture ink source with no need for a real
//! `Environment`. They stay available for exactly that under the
//! `test-util` feature (off by default). `#[cfg(test)]` cannot do this job —
//! the callers span separate integration-test crates that cannot see this
//! crate's own `#[cfg(test)]`. A test/bench/example target that needs them
//! opts in with a `dev-dependencies` edge enabling the feature, e.g.:
//!
//! The guarantee this actually gives: an external crates.io consumer, or
//! any isolated `cargo check -p <crate>` build that does not resolve
//! `brink-test-harness`, cannot reach these functions without opting in.
//! It is **not** a guarantee inside a `--workspace` build of this repo —
//! `brink-test-harness` takes `brink-compiler` with `features =
//! ["test-util"]` as a **normal** (non-dev) dependency, because its own
//! `[[bin]]` targets and `src/corpus.rs` call these functions
//! unconditionally, not just under a test cfg. Cargo's feature unification
//! then enables `test-util` for every crate sharing that `brink-compiler`
//! instance across the whole workspace resolve, so a production fn added to
//! e.g. `brink-cli` would still compile under `cargo check --workspace`.
//! The CI job's isolated `-p brink-cli -p brink-web -p brink-lsp -p
//! bevy-brink -p brink-environment` check (added alongside this note) is
//! what actually proves the fence for those crates, since it never resolves
//! `brink-test-harness`.
//!
//! ```toml
//! [dev-dependencies]
//! brink-compiler = { workspace = true, features = ["test-util"] }
//! ```

#[cfg(feature = "test-util")]
mod driver;

pub use brink_driver::{AnalysisOptions, Dialect, TypePolicy};
pub use brink_ir::{DiagnosticCode, FileId, Severity};

use brink_format::StoryData;
use std::io;
#[cfg(feature = "test-util")]
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
    /// The severity this diagnostic was actually resolved at
    /// (`brink_analyzer::effective_severity`, not the raw
    /// [`DiagnosticCode::severity`] default) — a `[lints]` re-leveled code
    /// (including a down-level to `Info`/`Hint`, issue #1162) carries its
    /// overridden severity here, so a renderer never has to re-derive it
    /// (and never has to assume every `CompileOutput::warnings` entry is
    /// actually `Severity::Warning`).
    pub severity: Severity,
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
///
/// **Test/bench/example use only** — gated behind the `test-util` feature;
/// see the module docs. Bypasses `Environment` entirely, so a real consumer
/// should use `brink_environment::compile(&Environment)` instead.
#[cfg(feature = "test-util")]
pub fn compile_path(path: &Path) -> Result<CompileOutput, CompileError> {
    compile(path.to_string_lossy().as_ref(), |p| {
        std::fs::read_to_string(p).map_err(|e| io::Error::new(e.kind(), format!("{p}: {e}")))
    })
}

/// Compile an ink story from an entry-point file path with explicit analysis
/// options — e.g. the T1b `--dialect` flag (`AnalysisOptions::dialect`).
///
/// **Test/bench/example use only** — gated behind the `test-util` feature;
/// see the module docs. Bypasses `Environment` entirely, so a real consumer
/// should use `brink_environment::compile(&Environment)` instead.
#[cfg(feature = "test-util")]
pub fn compile_path_with_options(
    path: &Path,
    options: AnalysisOptions,
) -> Result<CompileOutput, CompileError> {
    compile_with_options(
        path.to_string_lossy().as_ref(),
        |p| std::fs::read_to_string(p).map_err(|e| io::Error::new(e.kind(), format!("{p}: {e}"))),
        options,
    )
}

/// Compile an ink story with caller-provided file reading.
///
/// The `read_file` callback is called for the entry point and each
/// `INCLUDE`d file discovered during parsing. This enables compilation in
/// WASM, tests, and editor contexts where files are not on disk.
///
/// **Test/bench/example use only** — gated behind the `test-util` feature;
/// see the module docs. Bypasses `Environment` entirely, so a real consumer
/// should use `brink_environment::compile(&Environment)` instead.
#[cfg(feature = "test-util")]
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
///
/// **Test/bench/example use only** — gated behind the `test-util` feature;
/// see the module docs. Bypasses `Environment` entirely, so a real consumer
/// should use `brink_environment::compile(&Environment)` instead.
#[cfg(feature = "test-util")]
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
    /// Codegen (`brink-codegen-inkb`) refused a `Program` that violates an
    /// invariant an earlier compiler stage is supposed to guarantee — a
    /// compiler bug, not an authoring mistake. See
    /// `brink_codegen_inkb::CodegenError` and #586.
    #[error("internal codegen error: {0}")]
    Codegen(#[from] brink_codegen_inkb::CodegenError),
    /// Native (`.brink`) discovery produced a source key that is not
    /// root-relative (contains a `..` segment) — see
    /// `brink_driver::DiscoverError::InvalidKey` (issue #1288 review note
    /// (a)). Not reachable through `RealFs`/`GitRev` today; a save-key-
    /// identity guardrail against a future `SourceTree` impl that doesn't
    /// uphold the contract.
    #[error("invalid source key `{0}` (must be root-relative, no `..`)")]
    InvalidSourceKey(String),
    /// Native (`.brink`) discovery was handed a `SourceTree` that listed a
    /// non-`.brink` key — see `brink_driver::DiscoverError::NonNativeKey`
    /// (issue #1371). Not reachable through `prepare_driver`'s `RealFs::new`
    /// today (native-scoped, `.brink`-only); a guardrail against a future
    /// caller mistakenly widening the tree it hands to native discovery.
    #[error("source key `{0}` is not a native `.brink` file")]
    NonNativeSourceKey(String),
}

impl From<brink_driver::DiscoverError> for CompileError {
    fn from(err: brink_driver::DiscoverError) -> Self {
        match err {
            brink_driver::DiscoverError::Io(e) => Self::Io(e),
            brink_driver::DiscoverError::CircularInclude(msg) => Self::CircularInclude(msg),
            brink_driver::DiscoverError::InvalidKey(key) => Self::InvalidSourceKey(key),
            brink_driver::DiscoverError::NonNativeKey(key) => Self::NonNativeSourceKey(key),
        }
    }
}
