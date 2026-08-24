//! Whole-file byte→UTF-16 offset index (#3065, measure-first ruling
//! 2026-08-24).
//!
//! [`super::byte_to_utf16`] is a linear scan from offset 0 per call, and the
//! per-compile pulls call it per symbol/node/edge/diagnostic — 17,744 calls
//! per compile cycle on the perf fixture (`docs/desktop-perf-baseline.md`),
//! making `project_outline`/`story_graph` O(symbols × file size). This index
//! is built once per file per pull (one pass) and answers each conversion in
//! O(log lines + line length).
//!
//! Semantics are pinned to the naive function BIT-FOR-BIT (see the
//! equivalence test): the result counts the UTF-16 length of every char
//! whose START byte index is `< byte` — a mid-char `byte` includes its
//! containing char, and any `byte >= source.len()` yields the full UTF-16
//! length. Distinct from `brink_ide::LineIndex`, which answers (line, col)
//! pairs; the editor wire format wants flat whole-file UTF-16 offsets.

pub(crate) struct Utf16Index<'a> {
    source: &'a str,
    /// Byte offset of each line start (line 0 starts at 0).
    line_starts: Vec<u32>,
    /// Cumulative UTF-16 code units strictly before each line start.
    utf16_at_line: Vec<u32>,
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "ink files are always < 4GB (same bound as super::byte_to_utf16)"
)]
impl<'a> Utf16Index<'a> {
    pub(crate) fn new(source: &'a str) -> Self {
        let mut line_starts = vec![0u32];
        let mut utf16_at_line = vec![0u32];
        let mut utf16 = 0u32;
        for (i, c) in source.char_indices() {
            utf16 += c.len_utf16() as u32;
            if c == '\n' {
                line_starts.push(i as u32 + 1);
                utf16_at_line.push(utf16);
            }
        }
        Self {
            source,
            line_starts,
            utf16_at_line,
        }
    }

    /// See the module doc for the exact (naive-pinned) semantics.
    pub(crate) fn byte_to_utf16(&self, byte: u32) -> u32 {
        let line = self
            .line_starts
            .partition_point(|&start| start <= byte)
            .saturating_sub(1);
        let line_start = self.line_starts[line] as usize;
        let mut units = self.utf16_at_line[line];
        let byte = byte as usize;
        for (i, c) in self.source[line_start..].char_indices() {
            if line_start + i >= byte {
                return units;
            }
            units += c.len_utf16() as u32;
        }
        units
    }
}

#[cfg(test)]
mod tests {
    use super::Utf16Index;
    use crate::editor::byte_to_utf16;

    /// The one contract that matters: the index agrees with the naive scan
    /// at EVERY byte offset (boundaries, mid-char, past-the-end included)
    /// on content covering ASCII, 2/3/4-byte UTF-8 (incl. surrogate-pair
    /// emoji), CRLF, empty lines, and no-trailing-newline tails.
    #[test]
    fn equivalent_to_naive_at_every_offset() {
        let cases = [
            "",
            "\n",
            "plain ascii, one line",
            "line one\nline two\nline three\n",
            "café naïve\nsmall é here\n",
            "emoji 😀 line\nanother 🎭🎬 line\n",
            "crlf line\r\nnext\r\n",
            "\n\n\nempty lines\n\n",
            "no trailing newline: 😀 tail",
            "mixed é😀e\né😀\n😀",
        ];
        for source in cases {
            let index = Utf16Index::new(source);
            #[expect(clippy::cast_possible_truncation, reason = "test sources are tiny")]
            let len = source.len() as u32;
            for byte in 0..=len + 3 {
                assert_eq!(
                    index.byte_to_utf16(byte),
                    byte_to_utf16(source, byte),
                    "divergence at byte {byte} in {source:?}"
                );
            }
        }
    }
}
