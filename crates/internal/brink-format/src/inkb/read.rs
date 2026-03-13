//! Decoding (read) half of the `.inkb` binary format.

use crate::codec::{crc32, read_def_id, read_i32, read_str, read_u8, read_u16, read_u32, read_u64};
use crate::counting::CountingFlags;
use crate::definition::{
    AddressDef, ContainerDef, ExternalFnDef, GlobalVarDef, LineEntry, ListDef, ListItemDef,
    ScopeLineTable,
};
use crate::id::NameId;
use crate::line::{LineContent, LinePart, PluralCategory, SelectKey};
use crate::opcode::DecodeError;
use crate::story::StoryData;
use crate::value::{ListValue, Value, ValueType};

use super::{
    CAT_FEW, CAT_MANY, CAT_ONE, CAT_OTHER, CAT_TWO, CAT_ZERO, HEADER_PREAMBLE, InkbIndex,
    KEY_CARDINAL, KEY_EXACT, KEY_KEYWORD, KEY_ORDINAL, LINE_PLAIN, LINE_TEMPLATE, MAGIC,
    PART_LITERAL, PART_SELECT, PART_SLOT, SECTION_ENTRY_SIZE, SectionEntry, SectionKind, VAL_BOOL,
    VAL_DIVERT_TARGET, VAL_FLOAT, VAL_INT, VAL_LIST, VAL_NULL, VAL_STRING, VAL_VAR_POINTER,
    VERSION, safe_capacity,
};

// ── Tier 1: Full story read ─────────────────────────────────────────────────

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
    let line_tables = read_section_line_tables(buf, &index)?;
    let addresses = read_section_addresses(buf, &index)?;
    let list_literals = read_section_list_literals(buf, &index)?;

    Ok(StoryData {
        containers,
        line_tables,
        variables,
        list_defs,
        list_items,
        externals,
        addresses,
        name_table,
        list_literals,
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

// ── Tier 3: Section-level read ──────────────────────────────────────────────

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

/// Read the addresses from a complete `.inkb` file using its index.
pub fn read_section_addresses(
    buf: &[u8],
    index: &InkbIndex,
) -> Result<Vec<AddressDef>, DecodeError> {
    let Some(range) = index.section_range(SectionKind::Labels) else {
        // Addresses section is optional for backwards compatibility.
        return Ok(Vec::new());
    };
    let mut off = range.start;
    let count = read_u32(buf, &mut off)? as usize;
    // Each address entry: def_id(8) + container_id(8) + byte_offset(4) = 20 bytes
    let mut addresses = Vec::with_capacity(safe_capacity(count, buf.len(), off, 20));
    for _ in 0..count {
        let id = read_def_id(buf, &mut off)?;
        let container_id = read_def_id(buf, &mut off)?;
        let byte_offset = read_u32(buf, &mut off)?;
        addresses.push(AddressDef {
            id,
            container_id,
            byte_offset,
        });
    }
    Ok(addresses)
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
        VAL_VAR_POINTER => Ok(ValueType::VariablePointer),
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
        VAL_STRING => Ok(Value::String(read_str(buf, off)?.into())),
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
            Ok(Value::List(ListValue { items, origins }.into()))
        }
        VAL_DIVERT_TARGET => Ok(Value::DivertTarget(read_def_id(buf, off)?)),
        VAL_VAR_POINTER => Ok(Value::VariablePointer(read_def_id(buf, off)?)),
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
    let name = NameId(read_u16(buf, off)?);
    Ok(ListItemDef {
        id,
        origin,
        ordinal,
        name,
    })
}

/// Read the list literals from a complete `.inkb` file using its index.
pub fn read_section_list_literals(
    buf: &[u8],
    index: &InkbIndex,
) -> Result<Vec<ListValue>, DecodeError> {
    let Some(range) = index.section_range(SectionKind::ListLiterals) else {
        return Ok(Vec::new());
    };
    let mut off = range.start;
    let count = read_u32(buf, &mut off)? as usize;
    let mut literals = Vec::with_capacity(safe_capacity(count, buf.len(), off, 8));
    for _ in 0..count {
        let item_count = read_u32(buf, &mut off)? as usize;
        let mut items = Vec::with_capacity(safe_capacity(item_count, buf.len(), off, 8));
        for _ in 0..item_count {
            items.push(read_def_id(buf, &mut off)?);
        }
        let origin_count = read_u32(buf, &mut off)? as usize;
        let mut origins = Vec::with_capacity(safe_capacity(origin_count, buf.len(), off, 8));
        for _ in 0..origin_count {
            origins.push(read_def_id(buf, &mut off)?);
        }
        literals.push(ListValue { items, origins });
    }
    Ok(literals)
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
    let scope_id = read_def_id(buf, off)?;
    let content_hash = read_u64(buf, off)?;
    let counting_bits = read_u8(buf, off)?;
    let counting_flags = CountingFlags::from_bits(counting_bits).unwrap_or(CountingFlags::empty());
    let path_hash = read_i32(buf, off)?;

    let bytecode_len = read_u32(buf, off)? as usize;
    if *off + bytecode_len > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let bytecode = buf[*off..*off + bytecode_len].to_vec();
    *off += bytecode_len;

    Ok(ContainerDef {
        id,
        scope_id,
        bytecode,
        content_hash,
        counting_flags,
        path_hash,
    })
}

/// Read the line tables from a complete `.inkb` file using its index.
pub fn read_section_line_tables(
    buf: &[u8],
    index: &InkbIndex,
) -> Result<Vec<ScopeLineTable>, DecodeError> {
    let range =
        index
            .section_range(SectionKind::LineTables)
            .ok_or(DecodeError::MissingSectionKind(
                SectionKind::LineTables as u8,
            ))?;
    let mut off = range.start;
    let count = read_u32(buf, &mut off)? as usize;
    let mut tables = Vec::with_capacity(safe_capacity(count, buf.len(), off, 12));
    for _ in 0..count {
        tables.push(decode_scope_line_table(buf, &mut off)?);
    }
    Ok(tables)
}

fn decode_scope_line_table(buf: &[u8], off: &mut usize) -> Result<ScopeLineTable, DecodeError> {
    let scope_id = read_def_id(buf, off)?;
    let line_count = read_u32(buf, off)? as usize;
    let mut lines = Vec::with_capacity(safe_capacity(line_count, buf.len(), *off, 9));
    for _ in 0..line_count {
        lines.push(decode_line_entry(buf, off)?);
    }
    Ok(ScopeLineTable { scope_id, lines })
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
