use rowan::TextSize;

/// Maps byte offsets to (line, col) positions in a source file.
///
/// All positions are 0-based. Columns are measured in UTF-16 code units
/// to match the LSP specification.
#[derive(Debug)]
pub struct LineIndex {
    line_starts: Vec<u32>,
    source: String,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0u32];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(u32::try_from(i).unwrap_or(u32::MAX) + 1);
            }
        }
        Self {
            line_starts,
            source: source.to_owned(),
        }
    }

    /// Convert a byte offset to a 0-based `(line, utf16_col)` pair.
    pub fn line_col(&self, offset: TextSize) -> (u32, u32) {
        let offset = u32::from(offset);
        let line = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let line_start = self.line_starts[line] as usize;
        let offset_usize = offset as usize;

        // Count UTF-16 code units from line start to offset.
        let col_utf16 = self.source[line_start..offset_usize]
            .chars()
            .map(|c| u32::try_from(c.len_utf16()).unwrap_or(1))
            .sum();

        (u32::try_from(line).unwrap_or(u32::MAX), col_utf16)
    }

    /// Convert a 0-based `(line, utf16_col)` to a byte offset.
    pub fn offset(&self, line: u32, col: u32) -> TextSize {
        let line_idx = line as usize;
        let line_start = if line_idx < self.line_starts.len() {
            self.line_starts[line_idx] as usize
        } else {
            return TextSize::from(u32::try_from(self.source.len()).unwrap_or(u32::MAX));
        };

        let rest = &self.source[line_start..];
        let mut utf16_count = 0u32;
        let mut byte_offset = 0usize;
        for c in rest.chars() {
            if utf16_count >= col {
                break;
            }
            utf16_count += u32::try_from(c.len_utf16()).unwrap_or(1);
            byte_offset += c.len_utf8();
        }

        TextSize::from(u32::try_from(line_start + byte_offset).unwrap_or(u32::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source() {
        let idx = LineIndex::new("");
        assert_eq!(idx.line_col(TextSize::from(0)), (0, 0));
        assert_eq!(idx.offset(0, 0), TextSize::from(0));
    }

    #[test]
    fn single_line() {
        let idx = LineIndex::new("hello");
        assert_eq!(idx.line_col(TextSize::from(0)), (0, 0));
        assert_eq!(idx.line_col(TextSize::from(3)), (0, 3));
        assert_eq!(idx.line_col(TextSize::from(5)), (0, 5));
        assert_eq!(idx.offset(0, 3), TextSize::from(3));
    }

    #[test]
    fn multi_line() {
        let src = "abc\ndef\nghi";
        let idx = LineIndex::new(src);
        // 'a' at 0
        assert_eq!(idx.line_col(TextSize::from(0)), (0, 0));
        // 'd' at offset 4 => line 1, col 0
        assert_eq!(idx.line_col(TextSize::from(4)), (1, 0));
        // 'g' at offset 8 => line 2, col 0
        assert_eq!(idx.line_col(TextSize::from(8)), (2, 0));
        // 'i' at offset 10 => line 2, col 2
        assert_eq!(idx.line_col(TextSize::from(10)), (2, 2));

        assert_eq!(idx.offset(1, 0), TextSize::from(4));
        assert_eq!(idx.offset(2, 2), TextSize::from(10));
    }

    #[test]
    fn trailing_newline() {
        let src = "abc\n";
        let idx = LineIndex::new(src);
        // After newline => line 1, col 0
        assert_eq!(idx.line_col(TextSize::from(4)), (1, 0));
        assert_eq!(idx.offset(1, 0), TextSize::from(4));
    }

    #[test]
    fn multibyte_utf8() {
        // '€' is 3 bytes in UTF-8, 1 code unit in UTF-16
        // '𝄞' is 4 bytes in UTF-8, 2 code units in UTF-16
        let src = "a€𝄞b";
        let idx = LineIndex::new(src);

        // 'a' at byte 0 => col 0
        assert_eq!(idx.line_col(TextSize::from(0)), (0, 0));
        // '€' at byte 1 => col 1
        assert_eq!(idx.line_col(TextSize::from(1)), (0, 1));
        // '𝄞' at byte 4 => col 2 (after 'a'=1 + '€'=1)
        assert_eq!(idx.line_col(TextSize::from(4)), (0, 2));
        // 'b' at byte 8 => col 4 (after 'a'=1 + '€'=1 + '𝄞'=2)
        assert_eq!(idx.line_col(TextSize::from(8)), (0, 4));

        // Round-trip
        assert_eq!(idx.offset(0, 0), TextSize::from(0));
        assert_eq!(idx.offset(0, 1), TextSize::from(1));
        assert_eq!(idx.offset(0, 2), TextSize::from(4));
        assert_eq!(idx.offset(0, 4), TextSize::from(8));
    }

    #[test]
    fn offset_to_line_col_roundtrip() {
        let src = "abc\ndef\nghi";
        let idx = LineIndex::new(src);
        // offset 4 → (1,0) → offset 4
        let (line, col) = idx.line_col(TextSize::from(4));
        assert_eq!(idx.offset(line, col), TextSize::from(4));
        // offset 10 → (2,2) → offset 10
        let (line, col) = idx.line_col(TextSize::from(10));
        assert_eq!(idx.offset(line, col), TextSize::from(10));
    }
}
