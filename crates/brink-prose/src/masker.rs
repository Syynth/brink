//! The brink [`Masker`] — the piece that makes this a prose checker rather
//! than a text checker.
//!
//! Harper composes a `Masker` (which regions may be parsed) with a `Parser`
//! (how to tokenize them) via [`harper_core::parsers::Mask`], and pushes the
//! inner parser's spans back to absolute positions itself. So "check the
//! prose, never the machinery" is a masker, and the whole class of
//! offset-mapping bugs that a naive extract-and-remap implementation carries
//! simply cannot occur here.
//!
//! **The caller decides what prose is.** This crate is deliberately ignorant
//! of the HIR: it receives resolved ranges and does not know that they came
//! from `SpanKind::Content` with `Interpolation` children subtracted. If it
//! knew, it would have to depend on the compiler crates, and this wasm
//! artifact would duplicate the 2.6 MB that `brink-web` already ships — the
//! one thing the separate-module design exists to avoid.
//!
//! ## UTF-16 in and out, chars inside
//!
//! The boundary unit is the **UTF-16 code unit**, because both consumers
//! index that way: `CodeMirror` document positions are UTF-16 (`posOf` in
//! `hir-overlay.ts` computes `line.from + char`), and LSP positions are
//! UTF-16 by default. Bytes would force every caller to carry a conversion
//! table just to talk to this crate.
//!
//! Harper indexes by `char`. That conversion lives here, at the two edges, so
//! nothing else has to remember which space it is in. An offset landing
//! inside a surrogate pair is snapped outward rather than rejected — a
//! mis-specified range should narrow what gets checked, never panic, because
//! this runs on every keystroke and a panic poisons the wasm instance for the
//! rest of the session.

use harper_core::{Mask, Masker, Span};

/// A range of the source that holds real prose, in UTF-16 code units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utf16Range {
    pub start: usize,
    pub end: usize,
}

/// Allows exactly the ranges it was given, and nothing else.
pub struct SpanMasker {
    /// Char-indexed, sorted, non-overlapping, non-empty. Established once in
    /// [`SpanMasker::new`] — `Mask`'s `FromIterator` **asserts** all three,
    /// and this crate denies `panic`, so the invariant is enforced at
    /// construction rather than trusted.
    allowed: Vec<Span<char>>,
}

impl SpanMasker {
    /// Build a masker for `source` from UTF-16 ranges.
    ///
    /// Normalizes rather than validates: out-of-bounds ranges are clamped,
    /// inverted ones dropped, overlapping ones merged, and offsets inside a
    /// surrogate pair snapped to a character boundary.
    pub fn new(source: &str, ranges: &[Utf16Range]) -> Self {
        let to_char = Utf16ToChar::new(source);

        let mut spans: Vec<Span<char>> = ranges
            .iter()
            .filter_map(|r| {
                let start = to_char.floor(r.start);
                let end = to_char.ceil(r.end);
                (start < end).then(|| Span::new(start, end))
            })
            .collect();

        spans.sort_by_key(|s| (s.start, s.end));

        // Merge touching and overlapping runs. Overlap is not a caller error
        // worth refusing: the natural way to produce these ranges is to walk
        // a NESTED span tree, where two sibling walks can legitimately report
        // adjacent or duplicate cover.
        let mut allowed: Vec<Span<char>> = Vec::with_capacity(spans.len());
        for span in spans {
            match allowed.last_mut() {
                Some(last) if span.start <= last.end => {
                    last.end = last.end.max(span.end);
                }
                _ => allowed.push(span),
            }
        }

        Self { allowed }
    }

    /// Total characters this masker allows through — lets the caller skip the
    /// expensive dictionary build when there is no prose at all.
    pub fn allowed_chars(&self) -> usize {
        self.allowed.iter().map(|s| s.end - s.start).sum()
    }
}

impl Masker for SpanMasker {
    fn create_mask(&self, source: &[char]) -> Mask {
        // Clamp against the char length Harper actually sees. `new` clamped
        // against the `&str` it was given; the `Masker` contract makes this
        // slice authoritative, and an out-of-range span would index past it.
        let len = source.len();
        self.allowed
            .iter()
            .filter_map(|s| {
                let start = s.start.min(len);
                let end = s.end.min(len);
                (start < end).then(|| Span::new(start, end))
            })
            .collect()
    }
}

/// UTF-16 code-unit offset → char index.
///
/// A prefix table rather than a per-lookup scan: a document has many spans
/// and this runs per keystroke, so the quadratic version is a real cost.
struct Utf16ToChar {
    /// One entry per UTF-16 offset, plus the end. A unit inside a surrogate
    /// pair carries the index of the character it belongs to.
    char_at: Vec<usize>,
}

impl Utf16ToChar {
    fn new(source: &str) -> Self {
        let mut char_at = Vec::with_capacity(source.len() + 1);
        let mut count = 0usize;
        for (idx, ch) in source.chars().enumerate() {
            for _ in 0..ch.len_utf16() {
                char_at.push(idx);
            }
            count = idx + 1;
        }
        char_at.push(count);
        Self { char_at }
    }

    /// Char index at or before `offset` — a start inside a surrogate pair
    /// includes that whole character.
    fn floor(&self, offset: usize) -> usize {
        self.char_at
            .get(offset)
            .copied()
            .unwrap_or_else(|| self.char_at.last().copied().unwrap_or(0))
    }

    /// Char index at or after `offset` — an end inside a surrogate pair
    /// includes that whole character, so a range can never bisect one.
    fn ceil(&self, offset: usize) -> usize {
        let Some(&at) = self.char_at.get(offset) else {
            return self.char_at.last().copied().unwrap_or(0);
        };
        let inside_pair = offset > 0 && self.char_at.get(offset - 1).copied() == Some(at);
        if inside_pair { at + 1 } else { at }
    }
}

/// Char index → UTF-16 code-unit offset, for mapping Harper's lint spans out.
pub struct CharToUtf16 {
    offset_at: Vec<usize>,
}

impl CharToUtf16 {
    pub fn new(source: &str) -> Self {
        let mut offset_at = Vec::with_capacity(source.chars().count() + 1);
        let mut units = 0usize;
        for ch in source.chars() {
            offset_at.push(units);
            units += ch.len_utf16();
        }
        offset_at.push(units);
        Self { offset_at }
    }

    /// UTF-16 offset of char index `idx`, clamped to the end of the source.
    pub fn offset(&self, idx: usize) -> usize {
        self.offset_at
            .get(idx)
            .copied()
            .unwrap_or_else(|| self.offset_at.last().copied().unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::{CharToUtf16, SpanMasker, Utf16Range};
    use harper_core::Masker;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    /// UTF-16 offset of a byte position, so fixtures can be written with
    /// `str::find` rather than hand-counted units.
    fn utf16_of(source: &str, byte: usize) -> usize {
        source[..byte].chars().map(char::len_utf16).sum()
    }

    fn allowed_text(masker: &SpanMasker, src: &str) -> Vec<String> {
        let cs = chars(src);
        masker
            .create_mask(&cs)
            .iter_allowed(&cs)
            .map(|(_, c)| c.iter().collect())
            .collect()
    }

    #[test]
    fn allows_only_the_given_ranges() {
        let src = "-> knot\nThe square is empty.\n#tag";
        let start = utf16_of(src, src.find("The").expect("fixture contains 'The'"));
        let masker = SpanMasker::new(
            src,
            &[Utf16Range {
                start,
                end: start + "The square is empty.".len(),
            }],
        );
        assert_eq!(allowed_text(&masker, src), vec!["The square is empty."]);
    }

    #[test]
    fn merges_overlapping_and_adjacent_ranges() {
        // A nested-span walk can legitimately report both a parent's cover and
        // a child's; merging is what keeps `Mask`'s sortedness assertion — and
        // therefore its `assert!` — out of reach.
        let src = "abcdefghij";
        let masker = SpanMasker::new(
            src,
            &[
                Utf16Range { start: 4, end: 7 },
                Utf16Range { start: 0, end: 5 },
                Utf16Range { start: 7, end: 9 },
            ],
        );
        assert_eq!(masker.allowed_chars(), 9);
        assert_eq!(allowed_text(&masker, src), vec!["abcdefghi"]);
    }

    #[test]
    fn drops_inverted_and_out_of_bounds_ranges_instead_of_panicking() {
        let masker = SpanMasker::new(
            "hello",
            &[
                Utf16Range { start: 3, end: 1 },
                Utf16Range { start: 2, end: 2 },
                Utf16Range {
                    start: 99,
                    end: 120,
                },
            ],
        );
        assert_eq!(masker.allowed_chars(), 0);
    }

    #[test]
    fn an_offset_inside_a_surrogate_pair_snaps_outward() {
        // An emoji is ONE char and TWO UTF-16 units. A range starting at the
        // pair's second unit must widen to the whole character rather than
        // split it: Harper indexes by char, so a bisected range would shift
        // every span after it.
        let src = "🎭 the play";
        let masker = SpanMasker::new(src, &[Utf16Range { start: 1, end: 11 }]);
        assert_eq!(allowed_text(&masker, src), vec!["🎭 the play"]);
    }

    #[test]
    fn char_to_utf16_round_trips_through_astral_text() {
        let src = "a🎭b";
        let map = CharToUtf16::new(src);
        assert_eq!(map.offset(0), 0);
        assert_eq!(map.offset(1), 1);
        assert_eq!(map.offset(2), 3); // the emoji took two units
        assert_eq!(map.offset(3), 4);
        // Past the end clamps rather than panicking.
        assert_eq!(map.offset(99), 4);
    }
}
