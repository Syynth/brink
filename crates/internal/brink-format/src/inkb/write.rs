//! Encoding (write) half of the `.inkb` binary format.

use crate::codec::{
    crc32, write_def_id, write_i32, write_str, write_u8, write_u16, write_u32, write_u64,
};
use crate::definition::{
    AddressDef, ContainerDef, ExternalFnDef, GlobalVarDef, LineEntry, ListDef, ListItemDef,
    ScopeLineTable,
};
use crate::line::{LineContent, LinePart, PluralCategory, SelectKey};
use crate::story::StoryData;
use crate::value::{ListValue, Value, ValueType};

use super::{
    CAT_FEW, CAT_MANY, CAT_ONE, CAT_OTHER, CAT_TWO, CAT_ZERO, HEADER_PREAMBLE, KEY_CARDINAL,
    KEY_EXACT, KEY_KEYWORD, KEY_ORDINAL, LINE_PLAIN, LINE_TEMPLATE, MAGIC, PART_LITERAL,
    PART_SELECT, PART_SLOT, SECTION_COUNT, SECTION_ENTRY_SIZE, SectionKind, VAL_BOOL,
    VAL_DIVERT_TARGET, VAL_FLOAT, VAL_INT, VAL_LIST, VAL_NULL, VAL_STRING, VAL_VAR_POINTER,
    VERSION,
};

// ── Tier 1: Full story write ────────────────────────────────────────────────

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
        SectionKind::LineTables,
        SectionKind::Labels,
        SectionKind::ListLiterals,
    ];
    let mut section_offsets = [0u32; 9];

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

    // 7. LineTables
    section_offsets[6] = (buf.len() - base) as u32;
    write_section_line_tables(&story.line_tables, buf);

    // 8. Addresses (Labels section)
    section_offsets[7] = (buf.len() - base) as u32;
    write_section_addresses(&story.addresses, buf);

    // 9. ListLiterals
    section_offsets[8] = (buf.len() - base) as u32;
    write_section_list_literals(&story.list_literals, buf);

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

/// Write the addresses section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_addresses(addresses: &[AddressDef], buf: &mut Vec<u8>) {
    write_u32(buf, addresses.len() as u32);
    for addr in addresses {
        write_def_id(buf, addr.id);
        write_def_id(buf, addr.container_id);
        write_u32(buf, addr.byte_offset);
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
        ValueType::VariablePointer => VAL_VAR_POINTER,
        // TempPointer is runtime-only and should never appear in .inkb files.
        ValueType::TempPointer | ValueType::Null => VAL_NULL,
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
        Value::VariablePointer(id) => {
            write_u8(buf, VAL_VAR_POINTER);
            write_def_id(buf, *id);
        }
        // TempPointer is runtime-only and should never appear in .inkb files.
        Value::TempPointer { .. } | Value::Null => {
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
    write_u16(buf, li.name.0);
}

/// Write the list literals section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_list_literals(list_literals: &[ListValue], buf: &mut Vec<u8>) {
    write_u32(buf, list_literals.len() as u32);
    for lv in list_literals {
        write_u32(buf, lv.items.len() as u32);
        for item in &lv.items {
            write_def_id(buf, *item);
        }
        write_u32(buf, lv.origins.len() as u32);
        for origin in &lv.origins {
            write_def_id(buf, *origin);
        }
    }
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
    write_def_id(buf, c.scope_id);
    write_u64(buf, c.content_hash);
    write_u8(buf, c.counting_flags.bits());
    write_i32(buf, c.path_hash);
    write_u32(buf, c.bytecode.len() as u32);
    buf.extend_from_slice(&c.bytecode);
}

/// Write the line tables section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_line_tables(line_tables: &[ScopeLineTable], buf: &mut Vec<u8>) {
    write_u32(buf, line_tables.len() as u32);
    for lt in line_tables {
        encode_scope_line_table(lt, buf);
    }
}

#[expect(clippy::cast_possible_truncation)]
fn encode_scope_line_table(lt: &ScopeLineTable, buf: &mut Vec<u8>) {
    write_def_id(buf, lt.scope_id);
    write_u32(buf, lt.lines.len() as u32);
    for entry in &lt.lines {
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
