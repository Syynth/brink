//! Per-segment token cache — the main-thread half of
//! `docs/gpui-studio-spec.md` §3.3.
//!
//! Painting must not cost O(file) per keystroke. Whole-file parse +
//! classify is already over 1 ms at 282 lines and 21.6 ms at 16.8k, so with
//! no debounce to hide behind (ruled 2026-09-04) the work has to be
//! O(edit).
//!
//! [`brink_syntax::segment_file`] (#3084) gives that: a lex-only pass splits
//! the file at top-level knot/stitch headers, and only the segments whose
//! bytes actually changed are reparsed. Measured, release:
//!
//! | file | whole-file | segment (lex) | one knot | incremental |
//! |---|---|---|---|---|
//! | 1,402 lines | 2.96 ms | 0.22 | 0.023 | **0.24 ms** |
//! | 5,602 | 7.17 ms | 0.65 | 0.018 | **0.67 ms** |
//! | 16,802 | 21.6 ms | 2.22 | 0.017 | **2.24 ms** |
//!
//! One knot costs 17–51 microseconds *regardless of file size*; the
//! residual O(file) term is the lex pass, ~10x cheaper than parsing.
//!
//! ## Two inputs, both part of the reuse key
//!
//! A segment's tokens depend on its **text** and on the slice of the
//! `kinds` join covering it — the identity map that refines an `IDENT` into
//! a knot, function or variable. Keying on text alone would leave stale
//! refinement on screen after an analysis lands; invalidating everything
//! whenever `kinds` changes would repaint the whole file once per analysis,
//! which is per keystroke, which is the cost we came here to avoid. So the
//! key is both, sliced per segment and rebased to segment-relative offsets
//! — a knot whose own resolutions did not move keeps its tokens across an
//! analysis that changed some other knot.
//!
//! ## Native files are not incremental yet
//!
//! `segment_file` is ink-only; `brink-db`'s own `semantic_tokens_query`
//! takes a whole-file walk for `.brink` and so does [`TokenCache::update`].
//! Where a native segment boundary falls is a language question, not an
//! implementation one. See [`TokenCache::is_incremental`].

use std::collections::BTreeMap;

use brink_ir::semantic_tokens::RawToken;
use brink_ir::{LineIndex, SymbolKind};

use crate::worker::Kinds;

/// One cached segment. `tokens` are relative to the segment's first line,
/// so a segment that merely shifted is rebased rather than recomputed.
struct Entry {
    text: String,
    kinds: Vec<((u32, u32), SymbolKind)>,
    tokens: Vec<RawToken>,
}

/// Whether a document's paint path is incremental, and why not if it isn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Incrementality {
    /// Segmented; only changed knots are reparsed.
    PerSegment,
    /// Whole-file reparse — `segment_file` has no native sibling, so
    /// `.brink` pays O(file) on every keystroke.
    WholeFileNative,
}

/// The token cache for one open document.
pub struct TokenCache {
    native: bool,
    entries: Vec<Entry>,
    /// Segments reparsed by the most recent [`update`](Self::update), for
    /// tests and instrumentation.
    last_recomputed: usize,
    last_segments: usize,
}

impl TokenCache {
    #[must_use]
    pub fn new(path: &str) -> Self {
        Self {
            native: !path.ends_with(".ink"),
            entries: Vec::new(),
            last_recomputed: 0,
            last_segments: 0,
        }
    }

    #[must_use]
    pub fn is_incremental(&self) -> Incrementality {
        if self.native {
            Incrementality::WholeFileNative
        } else {
            Incrementality::PerSegment
        }
    }

    /// How many segments the last [`update`](Self::update) had to reparse,
    /// out of how many the file has.
    #[must_use]
    pub fn last_work(&self) -> (usize, usize) {
        (self.last_recomputed, self.last_segments)
    }

    /// Recompute this document's tokens against `source` and `kinds`,
    /// reusing every segment whose text and kind-slice are both unchanged.
    ///
    /// `kinds` is keyed by absolute byte range, as the worker ships it. It
    /// may lag the source: that is the one staleness the design permits,
    /// and it costs refinement only — an identifier not yet known to name a
    /// knot still paints as an identifier.
    pub fn update(&mut self, source: &str, kinds: &Kinds) -> Vec<RawToken> {
        if self.native {
            self.last_recomputed = 1;
            self.last_segments = 1;
            return classify_native(source, kinds);
        }

        let segments = brink_syntax::segment_file(source);
        let index = LineIndex::new(source);
        let mut fresh: Vec<Entry> = Vec::with_capacity(segments.len());
        let mut out: Vec<RawToken> = Vec::new();
        let mut recomputed = 0;

        for segment in &segments {
            let start = u32::from(segment.lowered_range.start());
            let end = u32::from(segment.lowered_range.end());
            let text = &source[start as usize..end as usize];
            let (base_line, _) = index.line_col(segment.lowered_range.start());

            // The kind slice covering this segment, rebased so it is
            // comparable across a pure shift.
            let slice: Vec<((u32, u32), SymbolKind)> = kinds
                .range((start, start)..(end, u32::MAX))
                .filter(|((_, e), _)| *e <= end)
                .map(|((s, e), k)| ((s - start, e - start), *k))
                .collect();

            let tokens = match self.take_reusable(text, &slice) {
                Some(tokens) => tokens,
                None => {
                    recomputed += 1;
                    let rebased: Kinds = slice.iter().copied().collect();
                    classify_ink(text, &rebased)
                }
            };

            for token in &tokens {
                let mut token = token.clone();
                token.line += base_line;
                out.push(token);
            }
            fresh.push(Entry {
                text: text.to_owned(),
                kinds: slice,
                tokens,
            });
        }

        self.entries = fresh;
        self.last_recomputed = recomputed;
        self.last_segments = segments.len();
        out
    }

    /// Take a cached segment matching both halves of the key, removing it so
    /// two identical segments cannot both claim the same entry.
    fn take_reusable(
        &mut self,
        text: &str,
        kinds: &[((u32, u32), SymbolKind)],
    ) -> Option<Vec<RawToken>> {
        let at = self
            .entries
            .iter()
            .position(|e| e.text == text && e.kinds == kinds)?;
        Some(self.entries.swap_remove(at).tokens)
    }
}

fn classify_ink(source: &str, kinds: &Kinds) -> Vec<RawToken> {
    let parsed = brink_syntax::parse(source);
    brink_ir::semantic_tokens::tokens_with_kinds(source, &parsed.syntax(), kinds)
}

fn classify_native(source: &str, kinds: &Kinds) -> Vec<RawToken> {
    let parsed = brink_syntax_native::parse(source);
    brink_ir::semantic_tokens::tokens_with_kinds_native(source, &parsed.syntax(), kinds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn knots(n: usize, marker: &str) -> String {
        let mut s = String::new();
        for k in 0..n {
            s.push_str(&format!(
                "=== knot_{k} ===\nLine one of {marker}.\nLine two.\n-> DONE\n\n"
            ));
        }
        s
    }

    fn empty() -> Kinds {
        BTreeMap::new()
    }

    #[test]
    fn a_first_pass_reparses_every_segment() {
        let mut cache = TokenCache::new("a.ink");
        let source = knots(5, "x");
        let tokens = cache.update(&source, &empty());
        assert!(!tokens.is_empty());
        let (recomputed, segments) = cache.last_work();
        assert_eq!(segments, 6, "five knots plus the header segment");
        assert_eq!(recomputed, segments, "nothing is cached on the first pass");
    }

    #[test]
    fn an_unchanged_file_reparses_nothing() {
        let mut cache = TokenCache::new("a.ink");
        let source = knots(5, "x");
        let first = cache.update(&source, &empty());
        let second = cache.update(&source, &empty());
        assert_eq!(cache.last_work().0, 0, "every segment must be reused");
        assert_eq!(first, second, "reuse must be byte-identical to recompute");
    }

    #[test]
    fn editing_one_knot_reparses_only_that_knot() {
        let mut cache = TokenCache::new("a.ink");
        cache.update(&knots(20, "x"), &empty());

        let mut edited = knots(20, "x");
        edited = edited.replace(
            "=== knot_7 ===\nLine one of x.",
            "=== knot_7 ===\nLine one EDITED.",
        );
        let tokens = cache.update(&edited, &empty());

        assert_eq!(
            cache.last_work(),
            (1, 21),
            "exactly one of 21 segments should have been reparsed"
        );
        // And the answer must still equal a cold computation.
        let mut cold = TokenCache::new("a.ink");
        assert_eq!(tokens, cold.update(&edited, &empty()));
    }

    #[test]
    fn inserting_a_knot_at_the_top_shifts_the_rest_without_reparsing_them() {
        let mut cache = TokenCache::new("a.ink");
        cache.update(&knots(10, "x"), &empty());

        let shifted = format!("=== brand_new ===\nHello.\n-> DONE\n\n{}", knots(10, "x"));
        let tokens = cache.update(&shifted, &empty());

        assert_eq!(
            cache.last_work().0,
            1,
            "only the inserted knot is new — the ten that merely moved are \
             rebased, and so is the (empty, and so unchanged) header segment"
        );
        let mut cold = TokenCache::new("a.ink");
        assert_eq!(
            tokens,
            cold.update(&shifted, &empty()),
            "rebasing must be byte-identical to recomputing"
        );
    }

    #[test]
    fn a_kinds_change_invalidates_only_the_segments_it_touches() {
        let source = knots(10, "x");
        let mut cache = TokenCache::new("a.ink");
        cache.update(&source, &empty());

        // Refine one identifier inside a single knot.
        let at = source.find("knot_4").expect("the fixture names knot_4") as u32;
        let mut kinds = empty();
        kinds.insert((at, at + 6), SymbolKind::Knot);
        cache.update(&source, &kinds);

        assert_eq!(
            cache.last_work().0,
            1,
            "a refinement inside one knot must not repaint the file"
        );
    }

    #[test]
    fn duplicate_segments_do_not_share_one_cache_entry() {
        // Two byte-identical knots: each must claim its own entry, or the
        // second would silently reuse the first's and drop a reparse.
        let source = "=== a ===\nSame.\n-> DONE\n\n=== a ===\nSame.\n-> DONE\n";
        let mut cache = TokenCache::new("a.ink");
        cache.update(source, &empty());
        assert_eq!(cache.last_work(), (3, 3));
        cache.update(source, &empty());
        assert_eq!(cache.last_work().0, 0, "both duplicates must be reused");
    }

    #[test]
    fn a_native_document_reports_that_it_is_not_incremental() {
        let cache = TokenCache::new("a.brink");
        assert_eq!(cache.is_incremental(), Incrementality::WholeFileNative);
        let ink = TokenCache::new("a.ink");
        assert_eq!(ink.is_incremental(), Incrementality::PerSegment);
    }
}
