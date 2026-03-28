//! Core infrastructure for the trait-based HIR lowering.
//!
//! Defines the read-only [`LowerScope`], the write-only [`LowerSink`] trait,
//! the [`Diagnosed`] proof token, and the production [`EffectSink`].

use rowan::TextRange;

use crate::symbols::{DeclaredSymbol, LocalSymbol, RefKind, UnresolvedRef};
use crate::{Diagnostic, DiagnosticCode, FileId, ParamInfo, Scope, SymbolKind, SymbolManifest};

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

    /// Convert to the existing `Scope` type used by `UnresolvedRef` and `LocalSymbol`.
    pub fn to_scope(&self) -> Scope {
        Scope {
            knot: self.current_knot.clone(),
            stitch: self.current_stitch.clone(),
        }
    }

    pub fn qualify_label(&self, label: &str) -> String {
        match (&self.current_knot, &self.current_stitch) {
            (Some(knot), Some(stitch)) => format!("{knot}.{stitch}.{label}"),
            (Some(knot), None) => format!("{knot}.{label}"),
            _ => label.to_string(),
        }
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

/// Write-only interface for lowering side effects: diagnostics, symbol
/// declarations, and unresolved references.
///
/// Node impls receive `&mut impl LowerSink`. They cannot read from the
/// sink — only push effects into it.
pub trait LowerSink {
    /// Emit a diagnostic and return a [`Diagnosed`] proof token.
    fn diagnose(&mut self, range: TextRange, code: DiagnosticCode) -> Diagnosed;

    /// Declare a symbol with no params or detail.
    fn declare(&mut self, kind: SymbolKind, name: &str, range: TextRange) {
        self.declare_with(kind, name, range, Vec::new(), None);
    }

    /// Declare a symbol with full metadata.
    fn declare_with(
        &mut self,
        kind: SymbolKind,
        name: &str,
        range: TextRange,
        params: Vec<ParamInfo>,
        detail: Option<String>,
    );

    /// Register a local variable (param or temp) scoped to a container.
    fn add_local(&mut self, local: LocalSymbol);

    /// Register an unresolved reference for cross-file resolution.
    fn add_unresolved(
        &mut self,
        path: &str,
        range: TextRange,
        kind: RefKind,
        scope: &Scope,
        arg_count: Option<usize>,
    );
}

// ─── Production sink ────────────────────────────────────────────────

/// Production implementation of [`LowerSink`]. Collects diagnostics and
/// builds a [`SymbolManifest`].
pub struct EffectSink {
    file_id: FileId,
    pub diagnostics: Vec<Diagnostic>,
    pub manifest: SymbolManifest,
}

impl EffectSink {
    pub fn new(file_id: FileId) -> Self {
        Self {
            file_id,
            diagnostics: Vec::new(),
            manifest: SymbolManifest::default(),
        }
    }

    /// Consume the sink and return the collected effects.
    pub fn finish(self) -> (SymbolManifest, Vec<Diagnostic>) {
        (self.manifest, self.diagnostics)
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

    fn declare_with(
        &mut self,
        kind: SymbolKind,
        name: &str,
        range: TextRange,
        params: Vec<ParamInfo>,
        detail: Option<String>,
    ) {
        let sym = DeclaredSymbol {
            name: name.to_string(),
            range,
            params,
            detail,
        };
        match kind {
            SymbolKind::Knot => self.manifest.knots.push(sym),
            SymbolKind::Stitch => self.manifest.stitches.push(sym),
            SymbolKind::Variable => self.manifest.variables.push(sym),
            SymbolKind::Constant => self.manifest.constants.push(sym),
            SymbolKind::List => self.manifest.lists.push(sym),
            SymbolKind::External => self.manifest.externals.push(sym),
            SymbolKind::Label => self.manifest.labels.push(sym),
            SymbolKind::ListItem => self.manifest.list_items.push(sym),
            // Param and Temp are registered via add_local, not declare.
            SymbolKind::Param | SymbolKind::Temp => {}
        }
    }

    fn add_local(&mut self, local: LocalSymbol) {
        self.manifest.locals.push(local);
    }

    fn add_unresolved(
        &mut self,
        path: &str,
        range: TextRange,
        kind: RefKind,
        scope: &Scope,
        arg_count: Option<usize>,
    ) {
        if path.is_empty() {
            return;
        }
        self.manifest.unresolved.push(UnresolvedRef {
            path: path.to_string(),
            range,
            kind,
            scope: scope.clone(),
            arg_count,
        });
    }
}
