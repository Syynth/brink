//! Fragment model for locale-safe slots.
//!
//! A fragment is a captured sub-region of output whose parts are stored
//! structurally (not eagerly stringified), so it can be resolved against
//! whatever line tables/locale are active at read time — the same
//! locale-hot-swap property `OutputPart` documents at the module level.
//! Fragments are how string-typed slot values in a template line (e.g.
//! `"{~x}"` where `x` is itself templated) stay locale-safe rather than
//! collapsing to a fixed-locale string at push time.

use alloc::string::String;
use alloc::vec::Vec;

use brink_format::{LineEntry, PluralResolver};

use super::{OutputBuffer, OutputPart, resolve_parts};
use crate::program::Program;

/// A finalized fragment — structural output parts plus any associated tags.
#[derive(Debug, Clone, PartialEq)]
pub struct Fragment {
    pub parts: Vec<OutputPart>,
    pub tags: Vec<String>,
}

impl OutputBuffer {
    // ── Fragment capture ───────────────────────────────────────────────

    /// Begin capturing output into a new fragment.
    pub fn begin_fragment(&mut self) {
        self.fragment_depth += 1;
        self.fragment_capture.push(OutputPart::Checkpoint);
        self.fragment_pending_tags.push(Vec::new());
    }

    /// End the current fragment capture: drain from the last checkpoint,
    /// store the parts in the fragment store, return the fragment index.
    #[expect(clippy::cast_possible_truncation)]
    pub fn end_fragment(&mut self) -> Option<u32> {
        let cp_idx = self
            .fragment_capture
            .iter()
            .rposition(|p| matches!(p, OutputPart::Checkpoint))?;

        let captured: Vec<OutputPart> = self.fragment_capture.drain(cp_idx..).collect();
        // Skip the checkpoint itself (first element).
        let parts: Vec<OutputPart> = captured.into_iter().skip(1).collect();
        let tags = self.fragment_pending_tags.pop().unwrap_or_default();
        let idx = self.fragments.len() as u32;
        self.fragments.push(Fragment { parts, tags });

        self.fragment_depth = self.fragment_depth.saturating_sub(1);

        Some(idx)
    }

    /// Returns true if currently inside a fragment capture.
    pub fn in_fragment_capture(&self) -> bool {
        self.fragment_depth > 0
    }

    /// Push a tag onto the current fragment being captured.
    pub fn push_fragment_tag(&mut self, tag: String) {
        if let Some(pending) = self.fragment_pending_tags.last_mut() {
            pending.push(tag);
        }
    }

    /// Read access to a finalized fragment's tags.
    pub fn fragment_tags(&self, idx: u32) -> Option<&[String]> {
        self.fragments.get(idx as usize).map(|f| f.tags.as_slice())
    }

    /// Read access to all finalized fragments.
    pub fn fragments(&self) -> &[Fragment] {
        &self.fragments
    }

    /// Read access to a finalized fragment's parts.
    pub fn fragment(&self, idx: u32) -> Option<&[OutputPart]> {
        self.fragments.get(idx as usize).map(|f| f.parts.as_slice())
    }

    /// Where a fragment's text came from (#3435): the FIRST `LineRef`'s
    /// line-table `source_location` — the same "first wins" rule a
    /// delivered line uses in `flush_lines`, through the same scope-table
    /// selection (`scope_table_idx`, never `line_tables` directly).
    pub fn fragment_source(
        &self,
        idx: u32,
        program: &Program,
        line_tables: &[Vec<LineEntry>],
    ) -> Option<brink_format::SourceLocation> {
        self.fragment(idx)?.iter().find_map(|part| match part {
            OutputPart::LineRef {
                container_idx,
                line_idx,
                ..
            } => {
                let scope_idx = program.scope_table_idx(*container_idx) as usize;
                line_tables
                    .get(scope_idx)
                    .and_then(|t| t.get(*line_idx as usize))
                    .and_then(|entry| entry.source_location.clone())
            }
            _ => None,
        })
    }

    /// Resolve a fragment's parts against the current line tables.
    pub fn resolve_fragment(
        &self,
        idx: u32,
        program: &Program,
        line_tables: &[Vec<LineEntry>],
        resolver: Option<&dyn PluralResolver>,
    ) -> String {
        match self.fragment(idx) {
            Some(parts) => resolve_parts(parts, program, line_tables, resolver, &self.fragments),
            None => String::new(),
        }
    }
}
