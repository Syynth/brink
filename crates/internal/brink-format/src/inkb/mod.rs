//! Binary (.inkb) writer and reader for [`StoryData`].
//!
//! The `.inkb` format is a compact, little-endian binary encoding designed for
//! fast loading by the runtime.
//!
//! ## Header layout
//!
//! ```text
//! Offset  Size   Field
//! ------  -----  ------
//! 0       4      Magic: b"INKB"
//! 4       2      Version: u16 LE (= 1)
//! 6       1      Section count: u8 (N entries in offset table)
//! 7       1      Reserved: 0x00
//! 8       4      File size: u32 LE (total bytes)
//! 12      4      Content checksum: u32 LE (CRC-32 of all bytes after header)
//! 16      N*8    Offset table entries
//! ```
//!
//! Each offset table entry (8 bytes):
//! ```text
//! 0       1      SectionKind: u8 tag
//! 1       3      Reserved: 3 bytes of 0x00
//! 4       4      Offset: u32 LE (byte offset from start of file)
//! ```

pub(crate) mod read;
pub(crate) mod write;

pub use read::{
    read_inkb, read_inkb_index, read_section_addresses, read_section_containers,
    read_section_externals, read_section_line_tables, read_section_list_defs,
    read_section_list_items, read_section_list_literals, read_section_name_table,
    read_section_variables,
};
pub use write::{
    assemble_inkb, write_inkb, write_section_addresses, write_section_containers,
    write_section_externals, write_section_line_tables, write_section_list_defs,
    write_section_list_items, write_section_list_literals, write_section_name_table,
    write_section_variables,
};

use std::ops::Range;

use crate::opcode::DecodeError;

// ── Constants ───────────────────────────────────────────────────────────────

pub(crate) const MAGIC: &[u8; 4] = b"INKB";
pub(crate) const VERSION: u16 = 1;
/// Fixed-size preamble: magic + version + section count + reserved + file size + checksum.
pub(crate) const HEADER_PREAMBLE: usize = 16;
/// Each offset table entry: kind(1) + reserved(3) + offset(4)
pub(crate) const SECTION_ENTRY_SIZE: usize = 8;
/// Number of sections in the current format.
pub(crate) const SECTION_COUNT: u8 = 9;

// Value type tags
pub(crate) const VAL_INT: u8 = 0x00;
pub(crate) const VAL_FLOAT: u8 = 0x01;
pub(crate) const VAL_BOOL: u8 = 0x02;
pub(crate) const VAL_STRING: u8 = 0x03;
pub(crate) const VAL_LIST: u8 = 0x04;
pub(crate) const VAL_DIVERT_TARGET: u8 = 0x05;
pub(crate) const VAL_NULL: u8 = 0x06;
pub(crate) const VAL_VAR_POINTER: u8 = 0x07;

// LineContent tags
pub(crate) const LINE_PLAIN: u8 = 0x00;
pub(crate) const LINE_TEMPLATE: u8 = 0x01;

// LinePart tags
pub(crate) const PART_LITERAL: u8 = 0x00;
pub(crate) const PART_SLOT: u8 = 0x01;
pub(crate) const PART_SELECT: u8 = 0x02;

// SelectKey tags
pub(crate) const KEY_CARDINAL: u8 = 0x00;
pub(crate) const KEY_ORDINAL: u8 = 0x01;
pub(crate) const KEY_EXACT: u8 = 0x02;
pub(crate) const KEY_KEYWORD: u8 = 0x03;

// PluralCategory tags
pub(crate) const CAT_ZERO: u8 = 0x00;
pub(crate) const CAT_ONE: u8 = 0x01;
pub(crate) const CAT_TWO: u8 = 0x02;
pub(crate) const CAT_FEW: u8 = 0x03;
pub(crate) const CAT_MANY: u8 = 0x04;
pub(crate) const CAT_OTHER: u8 = 0x05;

// ── Section types ───────────────────────────────────────────────────────────

/// Identifies a section within an `.inkb` file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SectionKind {
    NameTable = 0x01,
    Variables = 0x02,
    ListDefs = 0x03,
    ListItems = 0x04,
    Externals = 0x05,
    Containers = 0x06,
    LineTables = 0x07,
    Labels = 0x08,
    ListLiterals = 0x09,
}

impl SectionKind {
    pub(crate) fn from_u8(tag: u8) -> Result<Self, DecodeError> {
        match tag {
            0x01 => Ok(Self::NameTable),
            0x02 => Ok(Self::Variables),
            0x03 => Ok(Self::ListDefs),
            0x04 => Ok(Self::ListItems),
            0x05 => Ok(Self::Externals),
            0x06 => Ok(Self::Containers),
            0x07 => Ok(Self::LineTables),
            0x08 => Ok(Self::Labels),
            0x09 => Ok(Self::ListLiterals),
            _ => Err(DecodeError::InvalidSectionKind(tag)),
        }
    }
}

/// An entry in the `.inkb` offset table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionEntry {
    pub kind: SectionKind,
    pub offset: u32,
}

/// Parsed header + offset table from an `.inkb` file.
///
/// Allows selective reads without parsing section data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InkbIndex {
    pub version: u16,
    pub file_size: u32,
    pub checksum: u32,
    pub sections: Vec<SectionEntry>,
}

impl InkbIndex {
    /// Total header size in bytes (preamble + offset table).
    pub fn header_size(&self) -> usize {
        HEADER_PREAMBLE + self.sections.len() * SECTION_ENTRY_SIZE
    }

    /// Returns `(offset, length)` for a section, computing length from the
    /// next section's offset (or `file_size` for the last section).
    ///
    /// Subtraction is safe because `read_inkb_index` validates that offsets
    /// are monotonically increasing and within `[header_size, file_size]`.
    pub fn section_range(&self, kind: SectionKind) -> Option<Range<usize>> {
        let idx = self.sections.iter().position(|e| e.kind == kind)?;
        let start = self.sections[idx].offset as usize;
        let end = self
            .sections
            .get(idx + 1)
            .map_or(self.file_size, |e| e.offset) as usize;
        Some(start..end)
    }
}

/// Cap `Vec::with_capacity` allocations against remaining buffer bytes to avoid
/// OOM on crafted inputs with huge count fields. Each element occupies at least
/// `min_element_size` bytes, so the count can't exceed `remaining / min`.
pub(crate) fn safe_capacity(
    count: usize,
    buf_len: usize,
    offset: usize,
    min_element_size: usize,
) -> usize {
    let remaining = buf_len.saturating_sub(offset);
    let max_possible = if min_element_size > 0 {
        remaining / min_element_size
    } else {
        remaining
    };
    count.min(max_possible)
}
