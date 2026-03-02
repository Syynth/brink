//! Binary (.inkb) writer and reader for [`StoryData`].
//!
//! The `.inkb` format is a compact, little-endian binary encoding designed for
//! fast loading by the runtime. See the plan/spec for the full format layout.

use crate::codec::{
    read_def_id, read_i32, read_str, read_u8, read_u16, read_u32, read_u64, write_def_id,
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

// ── Constants ───────────────────────────────────────────────────────────────

const MAGIC: &[u8; 4] = b"INKB";
const VERSION: u16 = 1;

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

// ── Writer ──────────────────────────────────────────────────────────────────

/// Encode a [`StoryData`] into the `.inkb` binary format.
#[expect(clippy::cast_possible_truncation)]
pub fn write_inkb(story: &StoryData, buf: &mut Vec<u8>) {
    // Header
    buf.extend_from_slice(MAGIC);
    write_u16(buf, VERSION);
    write_u16(buf, 0); // reserved

    // 1. Name table
    write_u32(buf, story.name_table.len() as u32);
    for name in &story.name_table {
        write_str(buf, name);
    }

    // 2. Variables
    write_u32(buf, story.variables.len() as u32);
    for var in &story.variables {
        encode_global_var(var, buf);
    }

    // 3. List defs
    write_u32(buf, story.list_defs.len() as u32);
    for ld in &story.list_defs {
        encode_list_def(ld, buf);
    }

    // 4. List items
    write_u32(buf, story.list_items.len() as u32);
    for li in &story.list_items {
        encode_list_item(li, buf);
    }

    // 5. Externals
    write_u32(buf, story.externals.len() as u32);
    for ext in &story.externals {
        encode_external(ext, buf);
    }

    // 6. Containers
    write_u32(buf, story.containers.len() as u32);
    for c in &story.containers {
        encode_container(c, buf);
    }
}

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

// ── Reader ──────────────────────────────────────────────────────────────────

/// Decode a [`StoryData`] from `.inkb` binary format.
pub fn read_inkb(buf: &[u8]) -> Result<StoryData, DecodeError> {
    // Header
    if buf.len() < 8 {
        return Err(DecodeError::UnexpectedEof);
    }
    let magic: [u8; 4] = [buf[0], buf[1], buf[2], buf[3]];
    let mut off = 4;
    if &magic != MAGIC {
        return Err(DecodeError::BadMagic(magic));
    }
    let version = read_u16(buf, &mut off)?;
    if version != VERSION {
        return Err(DecodeError::UnsupportedVersion(version));
    }
    let _reserved = read_u16(buf, &mut off)?;

    // 1. Name table
    let name_count = read_u32(buf, &mut off)? as usize;
    let mut name_table = Vec::with_capacity(name_count);
    for _ in 0..name_count {
        name_table.push(read_str(buf, &mut off)?);
    }

    // 2. Variables
    let var_count = read_u32(buf, &mut off)? as usize;
    let mut variables = Vec::with_capacity(var_count);
    for _ in 0..var_count {
        variables.push(decode_global_var(buf, &mut off)?);
    }

    // 3. List defs
    let list_def_count = read_u32(buf, &mut off)? as usize;
    let mut list_defs = Vec::with_capacity(list_def_count);
    for _ in 0..list_def_count {
        list_defs.push(decode_list_def(buf, &mut off)?);
    }

    // 4. List items
    let list_item_count = read_u32(buf, &mut off)? as usize;
    let mut list_items = Vec::with_capacity(list_item_count);
    for _ in 0..list_item_count {
        list_items.push(decode_list_item(buf, &mut off)?);
    }

    // 5. Externals
    let ext_count = read_u32(buf, &mut off)? as usize;
    let mut externals = Vec::with_capacity(ext_count);
    for _ in 0..ext_count {
        externals.push(decode_external(buf, &mut off)?);
    }

    // 6. Containers
    let container_count = read_u32(buf, &mut off)? as usize;
    let mut containers = Vec::with_capacity(container_count);
    for _ in 0..container_count {
        containers.push(decode_container(buf, &mut off)?);
    }

    Ok(StoryData {
        containers,
        variables,
        list_defs,
        list_items,
        externals,
        name_table,
    })
}

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
            let mut items = Vec::with_capacity(item_count);
            for _ in 0..item_count {
                items.push(read_def_id(buf, off)?);
            }
            let origin_count = read_u32(buf, off)? as usize;
            let mut origins = Vec::with_capacity(origin_count);
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
    let mut items = Vec::with_capacity(item_count);
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
    let mut line_table = Vec::with_capacity(line_count);
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
            let mut parts = Vec::with_capacity(part_count);
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
            let mut variants = Vec::with_capacity(variant_count);
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
