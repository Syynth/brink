//! Which diagnostics of the compilation a batch acts on —
//! `docs/autofix-spec.md` §4.
//!
//! The *scope* never varies: [`FixCx`](super::FixCx) is the whole compilation
//! and a fixer's edits land wherever the fix needs them, so a selection
//! restricted to one file may still produce edits in another (§4, documented
//! rather than special-cased). What varies is which diagnostics are picked up.
//!
//! [`Select`] is a filter, not a menu of pre-baked scopes: `codes` restricts
//! by diagnostic code, `tiers` by the offered fix's [`Applicability`], and
//! `range` to one file's byte range. All three absent means the whole
//! compilation, every code, every tier — the `brink fix` / LSP `fixAll`
//! selection. `Select::at_offset` is the cursor-menu selection the
//! [`fixes_at`](super::fixes_at) pull already implements.

use brink_db::ProjectDb;
use brink_ir::{Diagnostic, DiagnosticCode, FileId};
use rowan::{TextRange, TextSize};

use super::Applicability;

/// A diagnostic selection for a batch.
#[derive(Debug, Clone, Default)]
pub struct Select {
    /// Only these diagnostic codes. `None` ⇒ every code.
    pub codes: Option<Vec<DiagnosticCode>>,
    /// Only fixes at these tiers. `None` ⇒ every tier the policy admits.
    pub tiers: Option<Vec<Applicability>>,
    /// Only diagnostics of this file whose own range meets this byte range.
    /// `None` ⇒ the whole compilation.
    pub range: Option<(FileId, TextRange)>,
}

impl Select {
    /// Every diagnostic of the compilation.
    #[must_use]
    pub fn all() -> Self {
        Self::default()
    }

    /// Restrict to these codes.
    #[must_use]
    pub fn with_codes(mut self, codes: Vec<DiagnosticCode>) -> Self {
        self.codes = Some(codes);
        self
    }

    /// Restrict to these fix tiers.
    #[must_use]
    pub fn with_tiers(mut self, tiers: Vec<Applicability>) -> Self {
        self.tiers = Some(tiers);
        self
    }

    /// Restrict to one byte range of one file.
    #[must_use]
    pub fn in_range(mut self, file: FileId, range: TextRange) -> Self {
        self.range = Some((file, range));
        self
    }

    /// Restrict to the diagnostics whose range covers `offset` — the
    /// cursor-menu selection. Inclusive at both ends, matching
    /// [`fixes_at`](super::fixes_at).
    #[must_use]
    pub fn at_offset(self, file: FileId, offset: TextSize) -> Self {
        self.in_range(file, TextRange::empty(offset))
    }

    /// Restrict to one whole file — "fix all in this file". The file's own
    /// length comes from the compilation, so the range is exact rather than
    /// an open-ended sentinel; an unknown file selects nothing.
    #[must_use]
    pub fn in_file(self, db: &ProjectDb, file: FileId) -> Self {
        let len = db
            .source(file)
            .and_then(|s| u32::try_from(s.len()).ok())
            .unwrap_or(0);
        self.in_range(file, TextRange::up_to(TextSize::from(len)))
    }

    /// The files whose diagnostics this selection can reach, in a
    /// deterministic order.
    ///
    /// The compilation is [`ProjectDb::compilation_closure`] — an ink entry's
    /// `INCLUDE` closure in topological order, or every discovered `.brink`
    /// module for a native entry. A session with no entry set (the editor's
    /// usual shape, and every fixture here) has an empty closure; there the
    /// selection falls back to every loaded file, id-ordered.
    #[must_use]
    pub fn files(&self, db: &ProjectDb) -> Vec<FileId> {
        if let Some((file, _)) = self.range {
            return if db.source(file).is_some() {
                vec![file]
            } else {
                Vec::new()
            };
        }
        let closure = db.compilation_closure();
        if closure.is_empty() {
            db.file_ids().collect()
        } else {
            closure
        }
    }

    /// Whether this diagnostic is in the selection (code and range halves —
    /// the tier half needs a computed fix, see [`admits_tier`](Self::admits_tier)).
    #[must_use]
    pub fn matches(&self, d: &Diagnostic) -> bool {
        if let Some(codes) = &self.codes
            && !codes.contains(&d.code)
        {
            return false;
        }
        if let Some((file, range)) = self.range {
            // Inclusive on both ends, so an empty range at an offset behaves
            // exactly like `contains_inclusive` — the cursor-menu shape.
            if d.file != file || d.range.start() > range.end() || range.start() > d.range.end() {
                return false;
            }
        }
        true
    }

    /// Whether a fix at this tier is in the selection.
    #[must_use]
    pub fn admits_tier(&self, tier: Applicability) -> bool {
        self.tiers.as_ref().is_none_or(|t| t.contains(&tier))
    }
}
