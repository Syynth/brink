//! Which diagnostics of the compilation a batch acts on —
//! `docs/autofix-spec.md` §4.
//!
//! The *scope* never varies: [`FixCx`](super::FixCx) is the whole compilation
//! and a fixer's edits land wherever the fix needs them, so a selection
//! restricted to one file may still produce edits in another (§4, documented
//! rather than special-cased). What varies is which diagnostics are picked up.
//!
//! [`Select`] is a filter, not a menu of pre-baked scopes: `codes` restricts
//! by diagnostic code, `excluded_codes` withdraws codes the caller's own
//! diagnostic surface does not show (`[lints] X = "allow"` — issue #3459),
//! `tiers` by the offered fix's [`Applicability`], and
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
    /// Never these diagnostic codes, whatever [`codes`](Self::codes) says.
    ///
    /// This is the seam a caller uses to withdraw a code that its own
    /// diagnostic surface does not show. `[lints] E014 = "allow"` suppresses
    /// a diagnostic outright (`brink_analyzer::effective_severity` returns
    /// `None`), and a batch must never offer, count or apply a fix for a
    /// problem the author cannot see (issue #3459) — the suppressed set is a
    /// *subtraction*, so it applies to an unrestricted selection too, which
    /// a `codes` whitelist could not express.
    ///
    /// Empty ⇒ nothing is withdrawn.
    pub excluded_codes: Vec<DiagnosticCode>,
    /// Only fixes at these tiers. `None` ⇒ every tier the policy admits.
    pub tiers: Option<Vec<Applicability>>,
    /// Only diagnostics of this file whose own range meets this byte range.
    /// The inner `None` means "the whole file, whatever its current length
    /// is" — re-derived from the compilation on every use ([`files`](Self::files)
    /// / [`matches`](Self::matches)) instead of a length frozen at
    /// construction, which is what [`in_file`](Self::in_file) needs under
    /// [`fix_all`](super::fix_all): the file grows round over round, and a
    /// stale end would silently strand any diagnostic that shifted past it.
    /// Outer `None` ⇒ the whole compilation.
    pub range: Option<(FileId, Option<TextRange>)>,
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

    /// Withdraw these codes from the selection — see
    /// [`excluded_codes`](Self::excluded_codes).
    #[must_use]
    pub fn excluding_codes(mut self, codes: Vec<DiagnosticCode>) -> Self {
        self.excluded_codes = codes;
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
        self.range = Some((file, Some(range)));
        self
    }

    /// Restrict to the diagnostics whose range covers `offset` — the
    /// cursor-menu selection. Inclusive at both ends, matching
    /// [`fixes_at`](super::fixes_at).
    #[must_use]
    pub fn at_offset(self, file: FileId, offset: TextSize) -> Self {
        self.in_range(file, TextRange::empty(offset))
    }

    /// Restrict to one whole file — "fix all in this file". The end of the
    /// range is not frozen here: [`files`](Self::files) and
    /// [`matches`](Self::matches) re-derive the file's current length from
    /// the compilation on every call, so a selection reused across
    /// [`fix_all`](super::fix_all)'s rounds keeps covering the whole file
    /// even as earlier rounds' edits grow it.
    #[must_use]
    pub fn in_file(mut self, file: FileId) -> Self {
        self.range = Some((file, None));
        self
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
    ///
    /// Takes `db` because an [`in_file`](Self::in_file) selection's range is
    /// not stored — it is re-derived from the file's *current* length on
    /// every call, so a `Select` reused across rounds by
    /// [`fix_all`](super::fix_all) keeps covering the whole file as it grows.
    #[must_use]
    pub fn matches(&self, db: &ProjectDb, d: &Diagnostic) -> bool {
        if self.excluded_codes.contains(&d.code) {
            return false;
        }
        if let Some(codes) = &self.codes
            && !codes.contains(&d.code)
        {
            return false;
        }
        if let Some((file, range)) = &self.range {
            let file = *file;
            let range = range.unwrap_or_else(|| {
                let len = db
                    .source(file)
                    .and_then(|s| u32::try_from(s.len()).ok())
                    .unwrap_or(0);
                TextRange::up_to(TextSize::from(len))
            });
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
