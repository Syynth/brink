//! Core infrastructure for the trait-based HIR lowering.
//!
//! Defines the read-only [`LowerScope`], the write-only [`LowerSink`] trait,
//! the [`Diagnosed`] proof token, and the production [`EffectSink`].

use rowan::TextRange;

use crate::host_manifest::DocBlock;
use crate::symbols::{DeclaredSymbol, LocalSymbol, RefKind, UnresolvedRef, VisibilityMark};
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
        self.declare_full(kind, name, range, Vec::new(), None, None);
    }

    /// Declare a symbol with params and detail but no doc.
    fn declare_with(
        &mut self,
        kind: SymbolKind,
        name: &str,
        range: TextRange,
        params: Vec<ParamInfo>,
        detail: Option<String>,
    ) {
        self.declare_full(kind, name, range, params, detail, None);
    }

    /// Declare a symbol with full metadata, optionally carrying inline `///`
    /// doc-comment metadata.
    fn declare_full(
        &mut self,
        kind: SymbolKind,
        name: &str,
        range: TextRange,
        params: Vec<ParamInfo>,
        detail: Option<String>,
        doc: Option<DocBlock>,
    );

    /// Attach an explicit `#@private`/`#@public` visibility override (M-2,
    /// docs/modules-spec.md §4) to the most recently declared symbol of
    /// `kind` named `name`. A no-op by default; declarations call it after
    /// their directive run is parsed (which happens after the `declare_*`
    /// call). Keyed by `(kind, name)`, not position, so it is robust to the
    /// nested-declaration order of knot/stitch bodies.
    fn set_visibility(&mut self, _kind: SymbolKind, _name: &str, _visibility: VisibilityMark) {}

    /// Attach a `#@was(old_name)` rename record (M-3, docs/modules-spec.md
    /// §5) to the most recently declared symbol of `kind` named `name`. A
    /// no-op by default, same calling convention as [`set_visibility`].
    fn set_was(&mut self, _kind: SymbolKind, _name: &str, _old_name: String, _range: TextRange) {}

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

    fn declare_full(
        &mut self,
        kind: SymbolKind,
        name: &str,
        range: TextRange,
        params: Vec<ParamInfo>,
        detail: Option<String>,
        doc: Option<DocBlock>,
    ) {
        let sym = DeclaredSymbol {
            name: name.to_string(),
            range,
            params,
            detail,
            visibility: None,
            was: None,
        };
        match kind {
            SymbolKind::Knot => self.manifest.knots.push(sym),
            SymbolKind::Stitch => self.manifest.stitches.push(sym),
            SymbolKind::Variable => self.manifest.variables.push(sym),
            SymbolKind::Constant => self.manifest.constants.push(sym),
            SymbolKind::List => self.manifest.lists.push(sym),
            SymbolKind::Struct => self.manifest.structs.push(sym),
            SymbolKind::External => self.manifest.externals.push(sym),
            SymbolKind::Label => self.manifest.labels.push(sym),
            SymbolKind::ListItem => self.manifest.list_items.push(sym),
            // Param and Temp are registered via add_local, not declare.
            SymbolKind::Param | SymbolKind::Temp => {}
        }
        if let Some(doc) = doc {
            self.manifest.docs.insert((kind, name.to_string()), doc);
        }
    }

    fn set_visibility(&mut self, kind: SymbolKind, name: &str, visibility: VisibilityMark) {
        let vec = match kind {
            SymbolKind::Knot => &mut self.manifest.knots,
            SymbolKind::Stitch => &mut self.manifest.stitches,
            SymbolKind::Variable => &mut self.manifest.variables,
            SymbolKind::Constant => &mut self.manifest.constants,
            SymbolKind::List => &mut self.manifest.lists,
            SymbolKind::Struct => &mut self.manifest.structs,
            SymbolKind::External => &mut self.manifest.externals,
            SymbolKind::Label | SymbolKind::ListItem | SymbolKind::Param | SymbolKind::Temp => {
                return;
            }
        };
        if let Some(sym) = vec.iter_mut().rev().find(|s| s.name == name) {
            sym.visibility = Some(visibility);
        }
    }

    fn set_was(&mut self, kind: SymbolKind, name: &str, old_name: String, range: TextRange) {
        let vec = match kind {
            SymbolKind::Knot => &mut self.manifest.knots,
            SymbolKind::Stitch => &mut self.manifest.stitches,
            SymbolKind::Variable => &mut self.manifest.variables,
            SymbolKind::Constant => &mut self.manifest.constants,
            SymbolKind::List => &mut self.manifest.lists,
            SymbolKind::Struct => &mut self.manifest.structs,
            SymbolKind::External => &mut self.manifest.externals,
            SymbolKind::Label | SymbolKind::ListItem | SymbolKind::Param | SymbolKind::Temp => {
                return;
            }
        };
        if let Some(sym) = vec.iter_mut().rev().find(|s| s.name == name) {
            sym.was = Some((old_name, range));
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
