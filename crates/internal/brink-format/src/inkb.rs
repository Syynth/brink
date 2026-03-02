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

use std::ops::Range;

use crate::codec::{
    crc32, read_def_id, read_i32, read_str, read_u8, read_u16, read_u32, read_u64, write_def_id,
    write_i32, write_str, write_u8, write_u16, write_u32, write_u64,
};
use crate::counting::CountingFlags;
use crate::definition::{
    ContainerDef, ExternalFnDef, GlobalVarDef, LineEntry, ListDef, ListItemDef,
};
use crate::id::NameId;
use crate::line::{LineContent, LinePart, PluralCategory, SelectKey};
use crate::opcode::DecodeError;
use crate::story::StoryData;
use crate::value::{ListValue, Value, ValueType};

/// Cap `Vec::with_capacity` allocations against remaining buffer bytes to avoid
/// OOM on crafted inputs with huge count fields. Each element occupies at least
/// `min_element_size` bytes, so the count can't exceed `remaining / min`.
fn safe_capacity(count: usize, buf_len: usize, offset: usize, min_element_size: usize) -> usize {
    let remaining = buf_len.saturating_sub(offset);
    let max_possible = if min_element_size > 0 {
        remaining / min_element_size
    } else {
        remaining
    };
    count.min(max_possible)
}

// ── Constants ───────────────────────────────────────────────────────────────

const MAGIC: &[u8; 4] = b"INKB";
const VERSION: u16 = 1;
/// Fixed-size preamble: magic + version + section count + reserved + file size + checksum.
const HEADER_PREAMBLE: usize = 16;
/// Each offset table entry: kind(1) + reserved(3) + offset(4)
const SECTION_ENTRY_SIZE: usize = 8;
/// Number of sections in the current format.
const SECTION_COUNT: u8 = 6;

// Value type tags
const VAL_INT: u8 = 0x00;
const VAL_FLOAT: u8 = 0x01;
const VAL_BOOL: u8 = 0x02;
const VAL_STRING: u8 = 0x03;
const VAL_LIST: u8 = 0x04;
const VAL_DIVERT_TARGET: u8 = 0x05;
const VAL_NULL: u8 = 0x06;

// LineContent tags
const LINE_PLAIN: u8 = 0x00;
const LINE_TEMPLATE: u8 = 0x01;

// LinePart tags
const PART_LITERAL: u8 = 0x00;
const PART_SLOT: u8 = 0x01;
const PART_SELECT: u8 = 0x02;

// SelectKey tags
const KEY_CARDINAL: u8 = 0x00;
const KEY_ORDINAL: u8 = 0x01;
const KEY_EXACT: u8 = 0x02;
const KEY_KEYWORD: u8 = 0x03;

// PluralCategory tags
const CAT_ZERO: u8 = 0x00;
const CAT_ONE: u8 = 0x01;
const CAT_TWO: u8 = 0x02;
const CAT_FEW: u8 = 0x03;
const CAT_MANY: u8 = 0x04;
const CAT_OTHER: u8 = 0x05;

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
}

impl SectionKind {
    fn from_u8(tag: u8) -> Result<Self, DecodeError> {
        match tag {
            0x01 => Ok(Self::NameTable),
            0x02 => Ok(Self::Variables),
            0x03 => Ok(Self::ListDefs),
            0x04 => Ok(Self::ListItems),
            0x05 => Ok(Self::Externals),
            0x06 => Ok(Self::Containers),
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

// ── Tier 1: Full story write/read ───────────────────────────────────────────

/// Encode a [`StoryData`] into the `.inkb` binary format with sectioned header.
#[expect(clippy::cast_possible_truncation)]
pub fn write_inkb(story: &StoryData, buf: &mut Vec<u8>) {
    let base = buf.len();
    let header_size = HEADER_PREAMBLE + SECTION_COUNT as usize * SECTION_ENTRY_SIZE;

    // Write placeholder header (zeros) — we'll patch it after writing sections.
    buf.resize(base + header_size, 0);

    // Track section offsets as we write each section.
    let section_kinds = [
        SectionKind::NameTable,
        SectionKind::Variables,
        SectionKind::ListDefs,
        SectionKind::ListItems,
        SectionKind::Externals,
        SectionKind::Containers,
    ];
    let mut section_offsets = [0u32; 6];

    // 1. NameTable
    section_offsets[0] = (buf.len() - base) as u32;
    write_section_name_table(&story.name_table, buf);

    // 2. Variables
    section_offsets[1] = (buf.len() - base) as u32;
    write_section_variables(&story.variables, buf);

    // 3. ListDefs
    section_offsets[2] = (buf.len() - base) as u32;
    write_section_list_defs(&story.list_defs, buf);

    // 4. ListItems
    section_offsets[3] = (buf.len() - base) as u32;
    write_section_list_items(&story.list_items, buf);

    // 5. Externals
    section_offsets[4] = (buf.len() - base) as u32;
    write_section_externals(&story.externals, buf);

    // 6. Containers
    section_offsets[5] = (buf.len() - base) as u32;
    write_section_containers(&story.containers, buf);

    let file_size = (buf.len() - base) as u32;
    let checksum = crc32(&buf[base + header_size..]);

    // Patch header in-place.
    let h = &mut buf[base..];
    h[0..4].copy_from_slice(MAGIC);
    h[4..6].copy_from_slice(&VERSION.to_le_bytes());
    h[6] = SECTION_COUNT;
    h[7] = 0; // reserved
    h[8..12].copy_from_slice(&file_size.to_le_bytes());
    h[12..16].copy_from_slice(&checksum.to_le_bytes());

    for (i, kind) in section_kinds.iter().enumerate() {
        let entry_base = HEADER_PREAMBLE + i * SECTION_ENTRY_SIZE;
        h[entry_base] = *kind as u8;
        h[entry_base + 1] = 0; // reserved
        h[entry_base + 2] = 0;
        h[entry_base + 3] = 0;
        h[entry_base + 4..entry_base + 8].copy_from_slice(&section_offsets[i].to_le_bytes());
    }
}

/// Decode a [`StoryData`] from `.inkb` binary format.
pub fn read_inkb(buf: &[u8]) -> Result<StoryData, DecodeError> {
    let index = read_inkb_index(buf)?;

    // Validate checksum.
    let header_size = index.header_size();
    let computed = crc32(&buf[header_size..]);
    if computed != index.checksum {
        return Err(DecodeError::ChecksumMismatch {
            expected: index.checksum,
            actual: computed,
        });
    }

    let name_table = read_section_name_table(buf, &index)?;
    let variables = read_section_variables(buf, &index)?;
    let list_defs = read_section_list_defs(buf, &index)?;
    let list_items = read_section_list_items(buf, &index)?;
    let externals = read_section_externals(buf, &index)?;
    let containers = read_section_containers(buf, &index)?;

    Ok(StoryData {
        containers,
        variables,
        list_defs,
        list_items,
        externals,
        name_table,
    })
}

// ── Tier 2: Index-only parse ────────────────────────────────────────────────

/// Parse the `.inkb` header and offset table without touching section data.
pub fn read_inkb_index(buf: &[u8]) -> Result<InkbIndex, DecodeError> {
    if buf.len() < HEADER_PREAMBLE {
        return Err(DecodeError::UnexpectedEof);
    }

    let magic: [u8; 4] = [buf[0], buf[1], buf[2], buf[3]];
    if &magic != MAGIC {
        return Err(DecodeError::BadMagic(magic));
    }

    let mut off = 4;
    let version = read_u16(buf, &mut off)?;
    if version != VERSION {
        return Err(DecodeError::UnsupportedVersion(version));
    }

    let section_count = read_u8(buf, &mut off)?;
    let _reserved = read_u8(buf, &mut off)?;
    let file_size = read_u32(buf, &mut off)?;
    let checksum = read_u32(buf, &mut off)?;

    // Validate file size.
    if file_size as usize != buf.len() {
        return Err(DecodeError::FileSizeMismatch {
            expected: file_size,
            actual: buf.len(),
        });
    }

    let total_header = HEADER_PREAMBLE + section_count as usize * SECTION_ENTRY_SIZE;
    if buf.len() < total_header {
        return Err(DecodeError::UnexpectedEof);
    }

    let mut sections = Vec::with_capacity(section_count as usize);
    for _ in 0..section_count {
        let kind_tag = read_u8(buf, &mut off)?;
        let kind = SectionKind::from_u8(kind_tag)?;
        let _reserved0 = read_u8(buf, &mut off)?;
        let _reserved1 = read_u8(buf, &mut off)?;
        let _reserved2 = read_u8(buf, &mut off)?;
        let offset = read_u32(buf, &mut off)?;
        sections.push(SectionEntry { kind, offset });
    }

    // Validate structural invariants so downstream code can trust the index:
    //   1. Every offset >= header size (sections live after the header)
    //   2. Offsets are strictly monotonically increasing
    //   3. Every offset <= file_size (sections live within the file)
    // Max value: 16 + 255*8 = 2056, always fits in u32.
    #[expect(clippy::cast_possible_truncation)]
    let header_size = total_header as u32;
    let mut prev_offset = header_size;
    for entry in &sections {
        if entry.offset < header_size || entry.offset > file_size || entry.offset < prev_offset {
            return Err(DecodeError::InvalidSectionOffset {
                kind: entry.kind as u8,
                offset: entry.offset,
            });
        }
        prev_offset = entry.offset;
    }

    Ok(InkbIndex {
        version,
        file_size,
        checksum,
        sections,
    })
}

// ── Tier 3: Section-level read/write ────────────────────────────────────────

// ── Section writers ─────────────────────────────────────────────────────────

/// Write the name table section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_name_table(names: &[String], buf: &mut Vec<u8>) {
    write_u32(buf, names.len() as u32);
    for name in names {
        write_str(buf, name);
    }
}

/// Write the variables section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_variables(variables: &[GlobalVarDef], buf: &mut Vec<u8>) {
    write_u32(buf, variables.len() as u32);
    for var in variables {
        encode_global_var(var, buf);
    }
}

/// Write the list definitions section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_list_defs(list_defs: &[ListDef], buf: &mut Vec<u8>) {
    write_u32(buf, list_defs.len() as u32);
    for ld in list_defs {
        encode_list_def(ld, buf);
    }
}

/// Write the list items section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_list_items(list_items: &[ListItemDef], buf: &mut Vec<u8>) {
    write_u32(buf, list_items.len() as u32);
    for li in list_items {
        encode_list_item(li, buf);
    }
}

/// Write the externals section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_externals(externals: &[ExternalFnDef], buf: &mut Vec<u8>) {
    write_u32(buf, externals.len() as u32);
    for ext in externals {
        encode_external(ext, buf);
    }
}

/// Write the containers section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_containers(containers: &[ContainerDef], buf: &mut Vec<u8>) {
    write_u32(buf, containers.len() as u32);
    for c in containers {
        encode_container(c, buf);
    }
}

// ── Section readers ─────────────────────────────────────────────────────────

/// Read the name table from a complete `.inkb` file using its index.
pub fn read_section_name_table(buf: &[u8], index: &InkbIndex) -> Result<Vec<String>, DecodeError> {
    let range =
        index
            .section_range(SectionKind::NameTable)
            .ok_or(DecodeError::MissingSectionKind(
                SectionKind::NameTable as u8,
            ))?;
    let mut off = range.start;
    let count = read_u32(buf, &mut off)? as usize;
    let mut names = Vec::with_capacity(safe_capacity(count, buf.len(), off, 4));
    for _ in 0..count {
        names.push(read_str(buf, &mut off)?);
    }
    Ok(names)
}

/// Read the variables from a complete `.inkb` file using its index.
pub fn read_section_variables(
    buf: &[u8],
    index: &InkbIndex,
) -> Result<Vec<GlobalVarDef>, DecodeError> {
    let range =
        index
            .section_range(SectionKind::Variables)
            .ok_or(DecodeError::MissingSectionKind(
                SectionKind::Variables as u8,
            ))?;
    let mut off = range.start;
    let count = read_u32(buf, &mut off)? as usize;
    let mut vars = Vec::with_capacity(safe_capacity(count, buf.len(), off, 12));
    for _ in 0..count {
        vars.push(decode_global_var(buf, &mut off)?);
    }
    Ok(vars)
}

/// Read the list definitions from a complete `.inkb` file using its index.
pub fn read_section_list_defs(buf: &[u8], index: &InkbIndex) -> Result<Vec<ListDef>, DecodeError> {
    let range = index
        .section_range(SectionKind::ListDefs)
        .ok_or(DecodeError::MissingSectionKind(SectionKind::ListDefs as u8))?;
    let mut off = range.start;
    let count = read_u32(buf, &mut off)? as usize;
    let mut defs = Vec::with_capacity(safe_capacity(count, buf.len(), off, 14));
    for _ in 0..count {
        defs.push(decode_list_def(buf, &mut off)?);
    }
    Ok(defs)
}

/// Read the list items from a complete `.inkb` file using its index.
pub fn read_section_list_items(
    buf: &[u8],
    index: &InkbIndex,
) -> Result<Vec<ListItemDef>, DecodeError> {
    let range =
        index
            .section_range(SectionKind::ListItems)
            .ok_or(DecodeError::MissingSectionKind(
                SectionKind::ListItems as u8,
            ))?;
    let mut off = range.start;
    let count = read_u32(buf, &mut off)? as usize;
    let mut items = Vec::with_capacity(safe_capacity(count, buf.len(), off, 20));
    for _ in 0..count {
        items.push(decode_list_item(buf, &mut off)?);
    }
    Ok(items)
}

/// Read the externals from a complete `.inkb` file using its index.
pub fn read_section_externals(
    buf: &[u8],
    index: &InkbIndex,
) -> Result<Vec<ExternalFnDef>, DecodeError> {
    let range =
        index
            .section_range(SectionKind::Externals)
            .ok_or(DecodeError::MissingSectionKind(
                SectionKind::Externals as u8,
            ))?;
    let mut off = range.start;
    let count = read_u32(buf, &mut off)? as usize;
    let mut exts = Vec::with_capacity(safe_capacity(count, buf.len(), off, 12));
    for _ in 0..count {
        exts.push(decode_external(buf, &mut off)?);
    }
    Ok(exts)
}

/// Read the containers from a complete `.inkb` file using its index.
pub fn read_section_containers(
    buf: &[u8],
    index: &InkbIndex,
) -> Result<Vec<ContainerDef>, DecodeError> {
    let range =
        index
            .section_range(SectionKind::Containers)
            .ok_or(DecodeError::MissingSectionKind(
                SectionKind::Containers as u8,
            ))?;
    let mut off = range.start;
    let count = read_u32(buf, &mut off)? as usize;
    let mut containers = Vec::with_capacity(safe_capacity(count, buf.len(), off, 21));
    for _ in 0..count {
        containers.push(decode_container(buf, &mut off)?);
    }
    Ok(containers)
}

// ── Assembly ────────────────────────────────────────────────────────────────

/// Assemble a complete `.inkb` file from pre-encoded section buffers.
///
/// Sections should be provided in the canonical order matching [`SectionKind`]
/// tags. The header (with offsets and checksum) is computed automatically.
#[expect(clippy::cast_possible_truncation)]
pub fn assemble_inkb(sections: &[(SectionKind, &[u8])], out: &mut Vec<u8>) {
    let base = out.len();
    let section_count = sections.len() as u8;
    let header_size = HEADER_PREAMBLE + sections.len() * SECTION_ENTRY_SIZE;

    // Placeholder header.
    out.resize(base + header_size, 0);

    // Append section data and record offsets.
    let mut entries: Vec<(SectionKind, u32)> = Vec::with_capacity(sections.len());
    for (kind, data) in sections {
        let offset = (out.len() - base) as u32;
        entries.push((*kind, offset));
        out.extend_from_slice(data);
    }

    let file_size = (out.len() - base) as u32;
    let checksum = crc32(&out[base + header_size..]);

    // Patch header.
    let h = &mut out[base..];
    h[0..4].copy_from_slice(MAGIC);
    h[4..6].copy_from_slice(&VERSION.to_le_bytes());
    h[6] = section_count;
    h[7] = 0;
    h[8..12].copy_from_slice(&file_size.to_le_bytes());
    h[12..16].copy_from_slice(&checksum.to_le_bytes());

    for (i, (kind, offset)) in entries.iter().enumerate() {
        let entry_base = HEADER_PREAMBLE + i * SECTION_ENTRY_SIZE;
        h[entry_base] = *kind as u8;
        h[entry_base + 1] = 0;
        h[entry_base + 2] = 0;
        h[entry_base + 3] = 0;
        h[entry_base + 4..entry_base + 8].copy_from_slice(&offset.to_le_bytes());
    }
}

// ── Encode helpers (private) ────────────────────────────────────────────────

fn encode_global_var(v: &GlobalVarDef, buf: &mut Vec<u8>) {
    write_def_id(buf, v.id);
    write_u16(buf, v.name.0);
    encode_value_type(v.value_type, buf);
    encode_value(&v.default_value, buf);
    write_u8(buf, u8::from(v.mutable));
}

fn encode_value_type(vt: ValueType, buf: &mut Vec<u8>) {
    let tag = match vt {
        ValueType::Int => VAL_INT,
        ValueType::Float => VAL_FLOAT,
        ValueType::Bool => VAL_BOOL,
        ValueType::String => VAL_STRING,
        ValueType::List => VAL_LIST,
        ValueType::DivertTarget => VAL_DIVERT_TARGET,
        ValueType::Null => VAL_NULL,
    };
    write_u8(buf, tag);
}

#[expect(clippy::cast_possible_truncation)]
fn encode_value(v: &Value, buf: &mut Vec<u8>) {
    match v {
        Value::Int(n) => {
            write_u8(buf, VAL_INT);
            write_i32(buf, *n);
        }
        Value::Float(n) => {
            write_u8(buf, VAL_FLOAT);
            buf.extend_from_slice(&n.to_le_bytes());
        }
        Value::Bool(b) => {
            write_u8(buf, VAL_BOOL);
            write_u8(buf, u8::from(*b));
        }
        Value::String(s) => {
            write_u8(buf, VAL_STRING);
            write_str(buf, s);
        }
        Value::List(lv) => {
            write_u8(buf, VAL_LIST);
            write_u32(buf, lv.items.len() as u32);
            for item in &lv.items {
                write_def_id(buf, *item);
            }
            write_u32(buf, lv.origins.len() as u32);
            for origin in &lv.origins {
                write_def_id(buf, *origin);
            }
        }
        Value::DivertTarget(id) => {
            write_u8(buf, VAL_DIVERT_TARGET);
            write_def_id(buf, *id);
        }
        Value::Null => {
            write_u8(buf, VAL_NULL);
        }
    }
}

#[expect(clippy::cast_possible_truncation)]
fn encode_list_def(ld: &ListDef, buf: &mut Vec<u8>) {
    write_def_id(buf, ld.id);
    write_u16(buf, ld.name.0);
    write_u32(buf, ld.items.len() as u32);
    for (name_id, ordinal) in &ld.items {
        write_u16(buf, name_id.0);
        write_i32(buf, *ordinal);
    }
}

fn encode_list_item(li: &ListItemDef, buf: &mut Vec<u8>) {
    write_def_id(buf, li.id);
    write_def_id(buf, li.origin);
    write_i32(buf, li.ordinal);
}

fn encode_external(ext: &ExternalFnDef, buf: &mut Vec<u8>) {
    write_def_id(buf, ext.id);
    write_u16(buf, ext.name.0);
    write_u8(buf, ext.arg_count);
    match ext.fallback {
        Some(fb) => {
            write_u8(buf, 1);
            write_def_id(buf, fb);
        }
        None => {
            write_u8(buf, 0);
        }
    }
}

#[expect(clippy::cast_possible_truncation)]
fn encode_container(c: &ContainerDef, buf: &mut Vec<u8>) {
    write_def_id(buf, c.id);
    write_u64(buf, c.content_hash);
    write_u8(buf, c.counting_flags.bits());
    write_u32(buf, c.bytecode.len() as u32);
    buf.extend_from_slice(&c.bytecode);
    write_u32(buf, c.line_table.len() as u32);
    for entry in &c.line_table {
        encode_line_entry(entry, buf);
    }
}

fn encode_line_entry(entry: &LineEntry, buf: &mut Vec<u8>) {
    encode_line_content(&entry.content, buf);
    write_u64(buf, entry.source_hash);
}

#[expect(clippy::cast_possible_truncation)]
fn encode_line_content(content: &LineContent, buf: &mut Vec<u8>) {
    match content {
        LineContent::Plain(s) => {
            write_u8(buf, LINE_PLAIN);
            write_str(buf, s);
        }
        LineContent::Template(parts) => {
            write_u8(buf, LINE_TEMPLATE);
            write_u32(buf, parts.len() as u32);
            for part in parts {
                encode_line_part(part, buf);
            }
        }
    }
}

#[expect(clippy::cast_possible_truncation)]
fn encode_line_part(part: &LinePart, buf: &mut Vec<u8>) {
    match part {
        LinePart::Literal(s) => {
            write_u8(buf, PART_LITERAL);
            write_str(buf, s);
        }
        LinePart::Slot(idx) => {
            write_u8(buf, PART_SLOT);
            write_u8(buf, *idx);
        }
        LinePart::Select {
            slot,
            variants,
            default,
        } => {
            write_u8(buf, PART_SELECT);
            write_u8(buf, *slot);
            write_u32(buf, variants.len() as u32);
            for (key, text) in variants {
                encode_select_key(key, buf);
                write_str(buf, text);
            }
            write_str(buf, default);
        }
    }
}

fn encode_select_key(key: &SelectKey, buf: &mut Vec<u8>) {
    match key {
        SelectKey::Cardinal(cat) => {
            write_u8(buf, KEY_CARDINAL);
            encode_plural_category(*cat, buf);
        }
        SelectKey::Ordinal(cat) => {
            write_u8(buf, KEY_ORDINAL);
            encode_plural_category(*cat, buf);
        }
        SelectKey::Exact(n) => {
            write_u8(buf, KEY_EXACT);
            write_i32(buf, *n);
        }
        SelectKey::Keyword(k) => {
            write_u8(buf, KEY_KEYWORD);
            write_str(buf, k);
        }
    }
}

fn encode_plural_category(cat: PluralCategory, buf: &mut Vec<u8>) {
    let tag = match cat {
        PluralCategory::Zero => CAT_ZERO,
        PluralCategory::One => CAT_ONE,
        PluralCategory::Two => CAT_TWO,
        PluralCategory::Few => CAT_FEW,
        PluralCategory::Many => CAT_MANY,
        PluralCategory::Other => CAT_OTHER,
    };
    write_u8(buf, tag);
}

// ── Decode helpers (private) ────────────────────────────────────────────────

fn decode_global_var(buf: &[u8], off: &mut usize) -> Result<GlobalVarDef, DecodeError> {
    let id = read_def_id(buf, off)?;
    let name = NameId(read_u16(buf, off)?);
    let value_type = decode_value_type(buf, off)?;
    let default_value = decode_value(buf, off)?;
    let mutable = read_u8(buf, off)? != 0;
    Ok(GlobalVarDef {
        id,
        name,
        value_type,
        default_value,
        mutable,
    })
}

fn decode_value_type(buf: &[u8], off: &mut usize) -> Result<ValueType, DecodeError> {
    let tag = read_u8(buf, off)?;
    match tag {
        VAL_INT => Ok(ValueType::Int),
        VAL_FLOAT => Ok(ValueType::Float),
        VAL_BOOL => Ok(ValueType::Bool),
        VAL_STRING => Ok(ValueType::String),
        VAL_LIST => Ok(ValueType::List),
        VAL_DIVERT_TARGET => Ok(ValueType::DivertTarget),
        VAL_NULL => Ok(ValueType::Null),
        _ => Err(DecodeError::InvalidValueType(tag)),
    }
}

fn decode_value(buf: &[u8], off: &mut usize) -> Result<Value, DecodeError> {
    let tag = read_u8(buf, off)?;
    match tag {
        VAL_INT => Ok(Value::Int(read_i32(buf, off)?)),
        VAL_FLOAT => {
            if *off + 4 > buf.len() {
                return Err(DecodeError::UnexpectedEof);
            }
            let v = f32::from_le_bytes([buf[*off], buf[*off + 1], buf[*off + 2], buf[*off + 3]]);
            *off += 4;
            Ok(Value::Float(v))
        }
        VAL_BOOL => Ok(Value::Bool(read_u8(buf, off)? != 0)),
        VAL_STRING => Ok(Value::String(read_str(buf, off)?)),
        VAL_LIST => {
            let item_count = read_u32(buf, off)? as usize;
            let mut items = Vec::with_capacity(safe_capacity(item_count, buf.len(), *off, 8));
            for _ in 0..item_count {
                items.push(read_def_id(buf, off)?);
            }
            let origin_count = read_u32(buf, off)? as usize;
            let mut origins = Vec::with_capacity(safe_capacity(origin_count, buf.len(), *off, 8));
            for _ in 0..origin_count {
                origins.push(read_def_id(buf, off)?);
            }
            Ok(Value::List(ListValue { items, origins }))
        }
        VAL_DIVERT_TARGET => Ok(Value::DivertTarget(read_def_id(buf, off)?)),
        VAL_NULL => Ok(Value::Null),
        _ => Err(DecodeError::InvalidValueType(tag)),
    }
}

fn decode_list_def(buf: &[u8], off: &mut usize) -> Result<ListDef, DecodeError> {
    let id = read_def_id(buf, off)?;
    let name = NameId(read_u16(buf, off)?);
    let item_count = read_u32(buf, off)? as usize;
    let mut items = Vec::with_capacity(safe_capacity(item_count, buf.len(), *off, 6));
    for _ in 0..item_count {
        let name_id = NameId(read_u16(buf, off)?);
        let ordinal = read_i32(buf, off)?;
        items.push((name_id, ordinal));
    }
    Ok(ListDef { id, name, items })
}

fn decode_list_item(buf: &[u8], off: &mut usize) -> Result<ListItemDef, DecodeError> {
    let id = read_def_id(buf, off)?;
    let origin = read_def_id(buf, off)?;
    let ordinal = read_i32(buf, off)?;
    Ok(ListItemDef {
        id,
        origin,
        ordinal,
    })
}

fn decode_external(buf: &[u8], off: &mut usize) -> Result<ExternalFnDef, DecodeError> {
    let id = read_def_id(buf, off)?;
    let name = NameId(read_u16(buf, off)?);
    let arg_count = read_u8(buf, off)?;
    let has_fallback = read_u8(buf, off)? != 0;
    let fallback = if has_fallback {
        Some(read_def_id(buf, off)?)
    } else {
        None
    };
    Ok(ExternalFnDef {
        id,
        name,
        arg_count,
        fallback,
    })
}

fn decode_container(buf: &[u8], off: &mut usize) -> Result<ContainerDef, DecodeError> {
    let id = read_def_id(buf, off)?;
    let content_hash = read_u64(buf, off)?;
    let counting_bits = read_u8(buf, off)?;
    let counting_flags = CountingFlags::from_bits(counting_bits).unwrap_or(CountingFlags::empty());

    let bytecode_len = read_u32(buf, off)? as usize;
    if *off + bytecode_len > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let bytecode = buf[*off..*off + bytecode_len].to_vec();
    *off += bytecode_len;

    let line_count = read_u32(buf, off)? as usize;
    let mut line_table = Vec::with_capacity(safe_capacity(line_count, buf.len(), *off, 9));
    for _ in 0..line_count {
        line_table.push(decode_line_entry(buf, off)?);
    }

    Ok(ContainerDef {
        id,
        bytecode,
        content_hash,
        counting_flags,
        line_table,
    })
}

fn decode_line_entry(buf: &[u8], off: &mut usize) -> Result<LineEntry, DecodeError> {
    let content = decode_line_content(buf, off)?;
    let source_hash = read_u64(buf, off)?;
    Ok(LineEntry {
        content,
        source_hash,
    })
}

fn decode_line_content(buf: &[u8], off: &mut usize) -> Result<LineContent, DecodeError> {
    let tag = read_u8(buf, off)?;
    match tag {
        LINE_PLAIN => Ok(LineContent::Plain(read_str(buf, off)?)),
        LINE_TEMPLATE => {
            let part_count = read_u32(buf, off)? as usize;
            let mut parts = Vec::with_capacity(safe_capacity(part_count, buf.len(), *off, 2));
            for _ in 0..part_count {
                parts.push(decode_line_part(buf, off)?);
            }
            Ok(LineContent::Template(parts))
        }
        _ => Err(DecodeError::InvalidLineContent(tag)),
    }
}

fn decode_line_part(buf: &[u8], off: &mut usize) -> Result<LinePart, DecodeError> {
    let tag = read_u8(buf, off)?;
    match tag {
        PART_LITERAL => Ok(LinePart::Literal(read_str(buf, off)?)),
        PART_SLOT => Ok(LinePart::Slot(read_u8(buf, off)?)),
        PART_SELECT => {
            let slot = read_u8(buf, off)?;
            let variant_count = read_u32(buf, off)? as usize;
            let mut variants = Vec::with_capacity(safe_capacity(variant_count, buf.len(), *off, 6));
            for _ in 0..variant_count {
                let key = decode_select_key(buf, off)?;
                let text = read_str(buf, off)?;
                variants.push((key, text));
            }
            let default = read_str(buf, off)?;
            Ok(LinePart::Select {
                slot,
                variants,
                default,
            })
        }
        _ => Err(DecodeError::InvalidLinePart(tag)),
    }
}

fn decode_select_key(buf: &[u8], off: &mut usize) -> Result<SelectKey, DecodeError> {
    let tag = read_u8(buf, off)?;
    match tag {
        KEY_CARDINAL => Ok(SelectKey::Cardinal(decode_plural_category(buf, off)?)),
        KEY_ORDINAL => Ok(SelectKey::Ordinal(decode_plural_category(buf, off)?)),
        KEY_EXACT => Ok(SelectKey::Exact(read_i32(buf, off)?)),
        KEY_KEYWORD => Ok(SelectKey::Keyword(read_str(buf, off)?)),
        _ => Err(DecodeError::InvalidSelectKey(tag)),
    }
}

fn decode_plural_category(buf: &[u8], off: &mut usize) -> Result<PluralCategory, DecodeError> {
    let tag = read_u8(buf, off)?;
    match tag {
        CAT_ZERO => Ok(PluralCategory::Zero),
        CAT_ONE => Ok(PluralCategory::One),
        CAT_TWO => Ok(PluralCategory::Two),
        CAT_FEW => Ok(PluralCategory::Few),
        CAT_MANY => Ok(PluralCategory::Many),
        CAT_OTHER => Ok(PluralCategory::Other),
        _ => Err(DecodeError::InvalidPluralCategory(tag)),
    }
}
