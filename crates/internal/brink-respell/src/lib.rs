//! `brink-respell` — the mechanical corpus converter half of issue #1178
//! (`docs/b0-sequencing.md` §B0.8b).
//!
//! This is a **dev/converter tool, not a runtime dependency** (`publish =
//! false` in its `Cargo.toml`, so it never trips the crates.io publishable
//! guard). It has exactly one job: take an existing ink corpus fixture
//! (`.ink` source), lower it to HIR through the **existing, trusted ink
//! frontend** (`brink_syntax` + `brink_ir::hir::lower`), and hand that HIR
//! to [`brink_ir::hir::emit_native::emit_file`] — the shared native-surface
//! pretty-printer — to produce `.brink` text.
//!
//! This crate does not implement its own translation logic: the emitter it
//! calls is deliberately frontend-agnostic (it walks the shared
//! `HirFile`/`Stmt`/`Expr` types both frontends produce), so "convert an
//! ink fixture" and "round-trip a native fixture" are the same code path
//! exercised from two different starting frontends. See the emitter's own
//! module doc for exactly which constructs are supported — an ink fixture
//! using anything outside that subset fails loudly with
//! [`RespellError::Emit`] rather than producing partial/invalid `.brink`.
//!
//! One ink-specific accommodation lives in the emitter, not here: root
//! (pre-first-knot) content has no native equivalent, so a non-empty
//! `HirFile.root_content` is wrapped in a synthesized top-level
//! `flow main() { … }`, mirroring the native story-entry convention
//! (`lower_native::entry_root_content`'s doc) and exactly what this
//! program's hand-curated `tests/tier1-brink-respell/` fixtures already do
//! by hand.

use brink_ir::FileId;
pub use brink_ir::hir::emit_native::{EmitError, emit_file};
use brink_ir::hir::lower;

/// Why [`respell_ink_source`] could not produce `.brink` text for a given
/// ink source string.
#[derive(Debug, thiserror::Error)]
pub enum RespellError {
    /// The ink source itself doesn't parse cleanly.
    #[error("ink parse errors: {0:?}")]
    InkParse(Vec<String>),
    /// The ink source parses but doesn't lower to clean HIR (a real ink
    /// semantic error, or a construct the ink frontend itself diagnoses).
    #[error("ink HIR lowering diagnostics: {0:?}")]
    InkLowering(Vec<String>),
    /// The HIR lowered cleanly but uses a construct the native emitter
    /// cannot faithfully respell — see [`EmitError`] and the emitter's own
    /// module doc for the exact supported subset.
    #[error(transparent)]
    Emit(#[from] EmitError),
}

/// Mechanically respell one ink source file into `.brink` native source.
///
/// Composes the **existing, trusted ink frontend**
/// (`brink_syntax::parse` and `brink_ir::hir::lower::lower`) with the
/// shared native-surface emitter. Bypasses `brink-db`/salsa and the
/// `INCLUDE` graph entirely (a single-file operation, mirroring
/// `brink_test_harness::corpus::compile_and_explore_from_brink_native`'s
/// own "honest minimal path" posture) — every tier-1 fixture this crate
/// targets is a single `.ink` file with no `INCLUDE`s, and a non-empty
/// `includes` list is one of the emitter's own hard-refused shapes anyway
/// ([`EmitError::Unsupported`]).
///
/// # Errors
///
/// Fails loudly (never partially) at the first unsupported stage: ink
/// parse errors, ink lowering diagnostics, or an unsupported HIR shape at
/// emission time.
pub fn respell_ink_source(src: &str) -> Result<String, RespellError> {
    let file_id = FileId(0);

    let parsed = brink_syntax::parse(src);
    if !parsed.errors().is_empty() {
        return Err(RespellError::InkParse(
            parsed.errors().iter().map(|e| format!("{e:?}")).collect(),
        ));
    }
    let tree = parsed.tree();

    let (hir, _manifest, diags) = lower::lower(file_id, &tree);
    if !diags.is_empty() {
        return Err(RespellError::InkLowering(
            diags.iter().map(|d| format!("{d:?}")).collect(),
        ));
    }

    Ok(emit_file(&hir)?)
}
