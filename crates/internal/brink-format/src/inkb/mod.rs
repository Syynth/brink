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
    read_inkb, read_inkb_index, read_section_address_paths, read_section_addresses,
    read_section_containers, read_section_externals, read_section_line_tables,
    read_section_list_defs, read_section_list_items, read_section_list_literals,
    read_section_literal_pool, read_section_name_table, read_section_struct_shapes,
    read_section_variables, read_section_visibility,
};
pub use write::{
    assemble_inkb, write_inkb, write_section_address_paths, write_section_addresses,
    write_section_containers, write_section_externals, write_section_line_tables,
    write_section_list_defs, write_section_list_items, write_section_list_literals,
    write_section_literal_pool, write_section_name_table, write_section_struct_shapes,
    write_section_variables, write_section_visibility,
};

use core::ops::Range;

use alloc::vec::Vec;

use crate::opcode::DecodeError;

// ── Constants ───────────────────────────────────────────────────────────────

pub(crate) const MAGIC: &[u8; 4] = b"INKB";
/// On-the-wire format version. Bumped on any byte-layout change; the reader
/// hard-rejects an unrecognized version (see `docs/format-spec.md` § Versioning).
/// v2 added `ContainerDef::param_count` to the Containers section.
/// v3 added the `local` scope bit to `GlobalVarDef` (Variables section) and
/// `ContainerDef` (Containers section) — see `docs/directive-annotations-spec.md`.
/// v4 added the collection value tags `VAL_ARRAY`/`VAL_MAP` (tree encoding) and
/// froze the reserved Tier-1 value-tag/section/opcode surface (the §9 one-bump
/// rule of `docs/value-model-spec.md`) — see `docs/format-v4-rfc.md`.
pub(crate) const VERSION: u16 = 4;
/// Fixed-size preamble: magic + version + section count + reserved + file size + checksum.
pub(crate) const HEADER_PREAMBLE: usize = 16;
/// Each offset table entry: kind(1) + reserved(3) + offset(4)
pub(crate) const SECTION_ENTRY_SIZE: usize = 8;
/// Number of sections in the current format.
pub(crate) const SECTION_COUNT: u8 = 12;

// Value type tags
pub(crate) const VAL_INT: u8 = 0x00;
pub(crate) const VAL_FLOAT: u8 = 0x01;
pub(crate) const VAL_BOOL: u8 = 0x02;
pub(crate) const VAL_STRING: u8 = 0x03;
pub(crate) const VAL_LIST: u8 = 0x04;
pub(crate) const VAL_DIVERT_TARGET: u8 = 0x05;
pub(crate) const VAL_NULL: u8 = 0x06;
pub(crate) const VAL_VAR_POINTER: u8 = 0x07;
pub(crate) const VAL_FRAGMENT_REF: u8 = 0x08;
// v4 collection tags (`docs/format-v4-rfc.md` §1). Tree encoding: sharing is
// not preserved on the wire — a snapshot serializes as a plain nested tree.
pub(crate) const VAL_ARRAY: u8 = 0x09;
pub(crate) const VAL_MAP: u8 = 0x0A;
// TM-4 (`docs/typed-mode-spec.md` §6 / `docs/value-model-spec.md` §11c):
// closed-shape records. `docs/format-v4-rfc.md` §1: `ShapeId (u32 into
// StructShapes), then field values in shape order`.
pub(crate) const VAL_RECORD: u8 = 0x0F;
// T1c (`docs/t1c-spec.md` §6, `docs/format-v4-rfc.md` §1): function values.
// `VAL_FN_REF` = the zero-bound case (a `DefinitionId`); `VAL_CLOSURE` =
// `DefinitionId`, u16 env count, then env entries `{NameId, kind u8 (0=val,
// 1=ref), value}`. Numeric assignments were frozen by the one-bump rule; this
// PR (T1c-2) materializes them.
pub(crate) const VAL_FN_REF: u8 = 0x0B;
pub(crate) const VAL_CLOSURE: u8 = 0x0C;
// T1d (`docs/t1d-spec.md` §2, `docs/format-v4-rfc.md` §1): opaque
// host-resource tokens. `kind NameId, u64 id` — no live pointer, no
// dedicated opcode; handles enter the script world only via bindings.
pub(crate) const VAL_HANDLE: u8 = 0x0D;

// Reserved v4 value tags — numeric assignments frozen by the one-bump rule,
// emitted by nothing in 4.0 (each is materialized when its milestone lands,
// still under VERSION 4). The strict reader rejects them until then because no
// `Value` variant exists to decode into. See `docs/format-v4-rfc.md` §1:
//   0x0E VAL_PROJECTION (T1e)

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
    AddressPaths = 0x0A,
    /// T1b `LiteralPool` (`docs/format-v4-rfc.md` §2): content-hash
    /// deduplicated constant values referenced by `PushLiteral(idx)`.
    /// Additive alongside `ListLiterals` — see the T1b-2 PR description for
    /// why the RFC's `ListLiterals` absorption is deferred, not done here.
    LiteralPool = 0x0B,
    /// TM-4 `StructShapes` (`docs/typed-mode-spec.md` §6): one entry per
    /// declared `STRUCT` — shape id, name, ordered field names. Reserved
    /// (count always 0) through 4.0; this PR lands the section's real
    /// encoding at the format layer only — nothing in the compiler emits a
    /// non-empty table yet (see the PR description's scope note).
    StructShapes = 0x0C,
    /// M-2b `Visibility` (`docs/modules-spec.md` §4, `docs/format-spec.md`):
    /// the `DefinitionId`s of every `#@private` definition, sorted ascending.
    /// **Omitted entirely when empty** (the common all-public case), so
    /// public-only stories stay byte-identical and no version bump is needed
    /// — the section is purely additive and self-framed in the offset table.
    /// `0x0D` is reserved for `EffectRows`, so this takes the next free tag.
    Visibility = 0x0E,
}

// Reserved v4 section kinds — numeric assignments frozen by the §9 one-bump
// rule (`docs/format-v4-rfc.md` §2 "Sections"), emitted by nothing in 4.0.
// Deliberately NOT a `SectionKind` variant: `from_u8` has no match arm for
// this, so the strict reader keeps rejecting it (`InvalidSectionKind`) until
// its milestone lands and a real variant is added, the same discipline
// `StructShapes` itself followed before this PR.
//   0x0D EffectRows    — reserved, count always 0 in 4.0, section-locally
//                        versioned so T2 can define the row encoding without
//                        another format bump

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
            0x0A => Ok(Self::AddressPaths),
            0x0B => Ok(Self::LiteralPool),
            0x0C => Ok(Self::StructShapes),
            0x0E => Ok(Self::Visibility),
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
    let max_possible = remaining.checked_div(min_element_size).unwrap_or(remaining);
    count.min(max_possible)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `EffectRows` (0x0D, `docs/format-v4-rfc.md` §2) is numbered but
    /// deliberately not a `SectionKind` variant — the strict reader must keep
    /// rejecting it until its milestone lands. `LiteralPool` (0x0B)
    /// graduated to a real section in T1b-2 (#570); `StructShapes` (0x0C)
    /// graduates here (TM-4, #620).
    #[test]
    fn from_u8_rejects_reserved_v4_sections() {
        let tag = 0x0Du8;
        let err = SectionKind::from_u8(tag).unwrap_err();
        assert_eq!(err, DecodeError::InvalidSectionKind(tag));
    }

    #[test]
    fn from_u8_accepts_all_current_sections() {
        for tag in 0x01u8..=0x0C {
            assert!(SectionKind::from_u8(tag).is_ok());
        }
    }
}
