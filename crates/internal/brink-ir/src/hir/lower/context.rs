//! Core infrastructure for the trait-based HIR lowering.
//!
//! Defines the read-only [`LowerScope`], the write-only [`LowerSink`] trait,
//! the [`Diagnosed`] proof token, and the production [`EffectSink`].
//!
//! Before B0.4 (docs/hir-admission-contract.md Q3(b)), [`LowerSink`] also
//! carried a `SymbolManifest`-building surface (`declare_full`/`add_local`/
//! `add_unresolved`/`set_visibility`/`set_was`) that every lowering site had
//! to call by hand, in parallel with building the HIR node itself — the
//! hand-built manifest path D3 names. That surface is gone:
//! `brink_ir::symbols::project_manifest` now derives the whole
//! `SymbolManifest` from the finished `HirFile`, so a frontend only ever
//! needs to emit diagnostics through this sink.

use rowan::TextRange;

use crate::{Diagnostic, DiagnosticCode, FileId};

// ─── Read-only scope ────────────────────────────────────────────────

/// Read-only context for lowering. Tracks where we are in the container
/// hierarchy. Only the backbone mutates this; node impls receive `&LowerScope`.
pub struct LowerScope {
    pub file_id: FileId,
    pub current_knot: Option<String>,
    pub current_stitch: Option<String>,
}

impl LowerScope {
    pub fn new(file_id: FileId) -> Self {
        Self {
            file_id,
            current_knot: None,
            current_stitch: None,
        }
    }

    /// Stamp ink-frontend [`crate::Provenance`] for `syntax` in this file.
    pub fn prov(
        &self,
        class: crate::provenance::NodeClass,
        syntax: &brink_syntax::SyntaxNode,
    ) -> crate::Provenance {
        crate::hir::ink_provenance(self.file_id, class, syntax)
    }
}

// ─── Proof token ────────────────────────────────────────────────────

/// Proof that at least one diagnostic was emitted. Cannot be constructed
/// outside this module — the only way to obtain one is via [`LowerSink::diagnose`].
///
/// In test builds, [`Diagnosed::test_token`] provides a way to construct
/// one for mock sink implementations.
pub struct Diagnosed {
    _private: (),
}

impl Diagnosed {
    /// Construct a `Diagnosed` token for testing purposes only.
    #[cfg(test)]
    pub fn test_token() -> Self {
        Self { _private: () }
    }
}

/// Result type for lowering operations.
///
/// - `Ok(value)` — lowering succeeded, produced a well-formed HIR node.
/// - `Err(Diagnosed)` — lowering failed, but a diagnostic was emitted (or
///   propagated from a child that emitted one).
pub type Lowered<T> = Result<T, Diagnosed>;

// ─── Write-only sink ────────────────────────────────────────────────

/// Write-only interface for lowering diagnostics.
///
/// Node impls receive `&mut impl LowerSink`. They cannot read from the
/// sink — only push effects into it.
pub trait LowerSink {
    /// Emit a diagnostic and return a [`Diagnosed`] proof token.
    fn diagnose(&mut self, range: TextRange, code: DiagnosticCode) -> Diagnosed;
}

// ─── Production sink ────────────────────────────────────────────────

/// Production implementation of [`LowerSink`]. Collects diagnostics.
pub struct EffectSink {
    file_id: FileId,
    pub diagnostics: Vec<Diagnostic>,
}

impl EffectSink {
    pub fn new(file_id: FileId) -> Self {
        Self {
            file_id,
            diagnostics: Vec::new(),
        }
    }

    /// Consume the sink and return the collected diagnostics.
    pub fn finish(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    /// Emit a diagnostic whose message carries case-specific text.
    ///
    /// [`LowerSink::diagnose`] always uses the code's fixed
    /// [`DiagnosticCode::title`]; the one lowering emission that needs more
    /// (issue #3050 — `E189` carries the `TODO:` note's own text so the
    /// Problems panel and the TODO panel can show it) pushes through here.
    pub fn diagnose_with_message(
        &mut self,
        range: TextRange,
        message: String,
        code: DiagnosticCode,
    ) {
        self.diagnostics.push(Diagnostic {
            file: self.file_id,
            range,
            message,
            code,
        });
    }
}

impl LowerSink for EffectSink {
    fn diagnose(&mut self, range: TextRange, code: DiagnosticCode) -> Diagnosed {
        self.diagnostics.push(Diagnostic {
            file: self.file_id,
            range,
            message: code.title().to_string(),
            code,
        });
        Diagnosed { _private: () }
    }
}
