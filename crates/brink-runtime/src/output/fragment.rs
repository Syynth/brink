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

/// A borrowed view of one fragment inside a [`Fragments`] store.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FragmentRef<'a> {
    pub parts: &'a [OutputPart],
    pub tags: &'a [String],
}

/// Where one fragment's parts live in the store's arena, plus its tags.
#[derive(Debug, Clone, Default)]
struct FragmentSpan {
    start: u32,
    end: u32,
    tags: Vec<String>,
}

/// The fragment store: every finalized fragment's parts laid end to end in
/// ONE arena, addressed by a per-fragment span.
///
/// Fragments are append-only and immutable once finalized (they exist so a
/// choice's text or a computed substring can be re-rendered later, in
/// another locale), so nothing ever needs to grow or drop one in place —
/// which is what makes a shared arena safe. Before this, each fragment
/// owned its own `Vec<OutputPart>`, and `end_fragment` built two of them
/// (`drain().collect()` then `skip(1).collect()`) per capture: 182K of
/// `hanoi-10`'s ~1M heap blocks were exactly those vectors. Now a capture
/// moves its parts once, into the arena's spare capacity.
///
/// Indices are unchanged by the layout: fragment `i` is the `i`-th span,
/// exactly as it was the `i`-th `Vec` — `Value::FragmentRef(i)` and the
/// persisted `.brkt` numbering mean the same thing they always did.
/// [`Fragment`] remains the materialized, owning form the codec and tests
/// speak; [`Self::to_vec`] / `From<Vec<Fragment>>` convert.
#[derive(Debug, Clone, Default)]
pub struct Fragments {
    arena: Vec<OutputPart>,
    spans: Vec<FragmentSpan>,
}

impl Fragments {
    /// Number of fragments in the store.
    #[must_use]
    pub fn len(&self) -> usize {
        self.spans.len()
    }

    /// Whether the store holds no fragments.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// The parts of fragment `idx`, if it exists.
    #[must_use]
    pub fn parts(&self, idx: u32) -> Option<&[OutputPart]> {
        let span = self.spans.get(idx as usize)?;
        self.arena.get(span.start as usize..span.end as usize)
    }

    /// The tags of fragment `idx`, if it exists.
    #[must_use]
    pub fn tags(&self, idx: u32) -> Option<&[String]> {
        self.spans.get(idx as usize).map(|s| s.tags.as_slice())
    }

    /// Fragment `idx` as a borrowed view, if it exists.
    #[must_use]
    pub fn get(&self, idx: u32) -> Option<FragmentRef<'_>> {
        Some(FragmentRef {
            parts: self.parts(idx)?,
            tags: self.tags(idx)?,
        })
    }

    /// Every fragment, in index order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = FragmentRef<'_>> + '_ {
        self.spans.iter().map(|span| FragmentRef {
            parts: &self.arena[span.start as usize..span.end as usize],
            tags: &span.tags,
        })
    }

    /// The owning form, one `Fragment` per entry.
    #[must_use]
    pub fn to_vec(&self) -> Vec<Fragment> {
        self.iter()
            .map(|f| Fragment {
                parts: f.parts.to_vec(),
                tags: f.tags.to_vec(),
            })
            .collect()
    }

    /// Append a fragment whose parts are `parts`, returning its index.
    #[expect(clippy::cast_possible_truncation)]
    pub(crate) fn push(
        &mut self,
        parts: impl Iterator<Item = OutputPart>,
        tags: Vec<String>,
    ) -> u32 {
        let start = self.arena.len() as u32;
        self.arena.extend(parts);
        let end = self.arena.len() as u32;
        let idx = self.spans.len() as u32;
        self.spans.push(FragmentSpan { start, end, tags });
        idx
    }
}

impl From<Vec<Fragment>> for Fragments {
    fn from(fragments: Vec<Fragment>) -> Self {
        let mut store = Self::default();
        for Fragment { parts, tags } in fragments {
            store.push(parts.into_iter(), tags);
        }
        store
    }
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
    pub fn end_fragment(&mut self) -> Option<u32> {
        let cp_idx = self
            .fragment_capture
            .iter()
            .rposition(|p| matches!(p, OutputPart::Checkpoint))?;

        let tags = self.fragment_pending_tags.pop().unwrap_or_default();
        // Move the captured parts (everything after the Checkpoint) straight
        // into the arena, then drop the Checkpoint itself — no intermediate
        // vector on either side.
        let idx = self
            .fragments
            .push(self.fragment_capture.drain(cp_idx + 1..), tags);
        self.fragment_capture.pop();

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
        self.fragments.tags(idx)
    }

    /// Read access to all finalized fragments.
    pub fn fragments(&self) -> &Fragments {
        &self.fragments
    }

    /// Read access to a finalized fragment's parts.
    pub fn fragment(&self, idx: u32) -> Option<&[OutputPart]> {
        self.fragments.parts(idx)
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

#[cfg(test)]
mod tests {
    use alloc::string::ToString;
    use alloc::vec;

    use super::*;

    fn text(s: &str) -> OutputPart {
        OutputPart::Text(s.to_string())
    }

    /// Nested captures land in one arena, each addressed by its own span:
    /// the inner fragment finalizes first (index 0), the outer keeps
    /// capturing and finalizes second (index 1) with only its own parts —
    /// the Checkpoint markers never reach the store.
    #[test]
    fn nested_captures_share_the_arena_with_disjoint_spans() {
        let mut buf = OutputBuffer::new();
        buf.begin_fragment();
        buf.push_text("outer-a");
        buf.begin_fragment();
        buf.push_text("inner");
        buf.push_fragment_tag("t".to_string());
        let inner = buf.end_fragment().expect("inner capture open");
        buf.push_text("outer-b");
        let outer = buf.end_fragment().expect("outer capture open");

        assert_eq!((inner, outer), (0, 1));
        assert_eq!(buf.fragments().len(), 2);
        assert_eq!(buf.fragment(inner), Some(&[text("inner")][..]));
        assert_eq!(buf.fragment_tags(inner), Some(&["t".to_string()][..]));
        assert_eq!(
            buf.fragment(outer),
            Some(&[text("outer-a"), text("outer-b")][..])
        );
        assert_eq!(buf.fragment_tags(outer), Some(&[][..]));
        assert!(buf.fragment(2).is_none());
        assert!(!buf.in_fragment_capture());
    }

    /// The owning form round-trips through the store in both directions,
    /// preserving order, parts and tags.
    #[test]
    fn owning_form_round_trips_through_the_store() {
        let owned = vec![
            Fragment {
                parts: vec![text("a"), OutputPart::Newline],
                tags: vec!["x".to_string()],
            },
            Fragment {
                parts: vec![],
                tags: vec![],
            },
            Fragment {
                parts: vec![OutputPart::Spring, text("c")],
                tags: vec!["y".to_string(), "z".to_string()],
            },
        ];
        let store = Fragments::from(owned.clone());
        assert_eq!(store.len(), 3);
        assert_eq!(store.parts(1), Some(&[][..]));
        assert_eq!(store.get(2).map(|f| f.tags.len()), Some(2));
        assert_eq!(store.to_vec(), owned);
        assert_eq!(store.iter().count(), 3);
    }
}
