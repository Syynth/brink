//! Decoding (read) half of the `.inkb` binary format.

use alloc::string::String;
use alloc::vec::Vec;

use crate::codec::{crc32, read_def_id, read_i32, read_str, read_u8, read_u16, read_u32, read_u64};
use crate::counting::CountingFlags;
use crate::definition::{
    AddressDef, AddressPath, AliasEntry, CallAtom, CapabilityParam, ContainerDef, DirectEffects,
    DispatchEntry, EffectRowEntry, ExternalFnDef, FrameShapeDef, GlobalVarDef, LineEntry, ListDef,
    ListItemDef, ParamMeta, ScopeLineTable, SlotInfo, SourceLocation, StructShapeDef,
};
use crate::id::{DefinitionId, NameId};
use crate::line::{LineContent, LinePart, PluralCategory, SelectKey};
use crate::opcode::DecodeError;
use crate::story::StoryData;
use crate::value::{
    ClosureEnvEntry, ListValue, MAX_DECODE_DEPTH, MapKey, OrderedMap, ShapeId, Value, ValueType,
};

use super::write::{
    ALIAS_TABLE_SECTION_VERSION, EFFECT_ROWS_SECTION_VERSION, FRAME_SHAPES_SECTION_VERSION,
};
use super::{
    CAP_PARAM_ANY, CAT_FEW, CAT_MANY, CAT_ONE, CAT_OTHER, CAT_TWO, CAT_ZERO, HANDLE_PARAM_NONE,
    HEADER_PREAMBLE, InkbIndex, KEY_CARDINAL, KEY_EXACT, KEY_KEYWORD, KEY_ORDINAL, LINE_PLAIN,
    LINE_TEMPLATE, MAGIC, PART_LITERAL, PART_SELECT, PART_SLOT, PART_SPAN, PROJ_SEG_INDEX,
    PROJ_SEG_KEY,
    SECTION_ENTRY_SIZE, SectionEntry, SectionKind, VAL_ARRAY, VAL_BOOL, VAL_CLOSURE,
    VAL_DIVERT_TARGET, VAL_FLOAT, VAL_FN_REF, VAL_FRAGMENT_REF, VAL_HANDLE, VAL_INT, VAL_LIST,
    VAL_MAP, VAL_MAT2, VAL_MAT3, VAL_MAT4, VAL_NULL, VAL_OPTION, VAL_PROJECTION, VAL_QUAT,
    VAL_RANGE, VAL_RECORD, VAL_STRING, VAL_VAR_POINTER, VAL_VEC2, VAL_VEC3, VAL_VEC4, VAL_WEIGHTED,
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
    let address_paths = read_section_address_paths(buf, &index)?;
    let literal_pool = read_section_literal_pool(buf, &index)?;
    let struct_shapes = read_section_struct_shapes(buf, &index)?;
    let private_defs = read_section_visibility(buf, &index)?;
    let alias_table = read_section_alias_table(buf, &index)?;
    let effect_rows = read_section_effect_rows(buf, &index)?;
    let frame_shapes = read_section_frame_shapes(buf, &index)?;

    Ok(StoryData {
        containers,
        line_tables,
        variables,
        list_defs,
        list_items,
        externals,
        addresses,
        address_paths,
        name_table,
        list_literals,
        literal_pool,
        struct_shapes,
        private_defs,
        alias_table,
        effect_rows,
        frame_shapes,
        source_checksum: index.checksum,
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

/// Read the address-paths section using a pre-parsed index.
pub fn read_section_address_paths(
    buf: &[u8],
    index: &InkbIndex,
) -> Result<Vec<AddressPath>, DecodeError> {
    let Some(range) = index.section_range(SectionKind::AddressPaths) else {
        // AddressPaths section is optional for backwards compatibility
        // (legacy `.inkb` and converter output omit it).
        return Ok(Vec::new());
    };
    let mut off = range.start;
    let count = read_u32(buf, &mut off)? as usize;
    // Each entry: path NameId(2) + target def_id(8) = 10 bytes
    let mut paths = Vec::with_capacity(safe_capacity(count, buf.len(), off, 10));
    for _ in 0..count {
        let path = NameId(read_u16(buf, &mut off)?);
        let target = read_def_id(buf, &mut off)?;
        paths.push(AddressPath { path, target });
    }
    Ok(paths)
}

// ── Decode helpers (private) ────────────────────────────────────────────────

fn decode_global_var(buf: &[u8], off: &mut usize) -> Result<GlobalVarDef, DecodeError> {
    let id = read_def_id(buf, off)?;
    let name = NameId(read_u16(buf, off)?);
    let value_type = decode_value_type(buf, off)?;
    let default_value = decode_value(buf, off, 0)?;
    let mutable = read_u8(buf, off)? != 0;
    let local = read_u8(buf, off)? != 0;
    Ok(GlobalVarDef {
        id,
        name,
        value_type,
        default_value,
        mutable,
        local,
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
        VAL_FRAGMENT_REF => Ok(ValueType::FragmentRef),
        VAL_NULL => Ok(ValueType::Null),
        VAL_ARRAY => Ok(ValueType::Array),
        VAL_MAP => Ok(ValueType::Map),
        VAL_RECORD => Ok(ValueType::Record),
        VAL_FN_REF => Ok(ValueType::FnRef),
        VAL_CLOSURE => Ok(ValueType::Closure),
        VAL_HANDLE => Ok(ValueType::Handle),
        VAL_PROJECTION => Ok(ValueType::Projection),
        VAL_OPTION => Ok(ValueType::Option),
        VAL_RANGE => Ok(ValueType::Range),
        VAL_VEC2 => Ok(ValueType::Vec2),
        VAL_VEC3 => Ok(ValueType::Vec3),
        VAL_VEC4 => Ok(ValueType::Vec4),
        VAL_QUAT => Ok(ValueType::Quat),
        VAL_MAT2 => Ok(ValueType::Mat2),
        VAL_MAT3 => Ok(ValueType::Mat3),
        VAL_MAT4 => Ok(ValueType::Mat4),
        VAL_WEIGHTED => Ok(ValueType::Weighted),
        _ => Err(DecodeError::InvalidValueType(tag)),
    }
}

/// NS-A8 (`docs/tower-mini-spec.md` T5): read `N` explicit little-endian
/// f32 lanes — the hand-serialized tower wire form `write_f32_lanes`
/// produced. The lanes are handed back as a plain array; the caller builds
/// the glam value through its explicit `from_array`/`from_cols_array`
/// constructor (never a memory-layout cast).
fn read_f32_lanes<const N: usize>(buf: &[u8], off: &mut usize) -> Result<[f32; N], DecodeError> {
    if *off + 4 * N > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let mut lanes = [0.0f32; N];
    for lane in &mut lanes {
        *lane = f32::from_le_bytes([buf[*off], buf[*off + 1], buf[*off + 2], buf[*off + 3]]);
        *off += 4;
    }
    Ok(lanes)
}

#[expect(
    clippy::too_many_lines,
    reason = "one match arm per value tag — the NS-A1 VAL_OPTION arm pushed this past 100"
)]
fn decode_value(buf: &[u8], off: &mut usize, depth: usize) -> Result<Value, DecodeError> {
    if depth > MAX_DECODE_DEPTH {
        return Err(DecodeError::MaxDepthExceeded(MAX_DECODE_DEPTH));
    }
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
        VAL_FRAGMENT_REF => Ok(Value::FragmentRef(read_u32(buf, off)?)),
        VAL_NULL => Ok(Value::Null),
        VAL_ARRAY => {
            let len = read_u32(buf, off)? as usize;
            // Each element is at least one tag byte, so `len` can't exceed the
            // remaining bytes — cap the pre-allocation against crafted inputs.
            let mut items = Vec::with_capacity(safe_capacity(len, buf.len(), *off, 1));
            for _ in 0..len {
                items.push(decode_value(buf, off, depth + 1)?);
            }
            Ok(Value::array(items))
        }
        VAL_MAP => {
            let len = read_u32(buf, off)? as usize;
            let mut map = OrderedMap::with_capacity(safe_capacity(len, buf.len(), *off, 2));
            for _ in 0..len {
                let key = decode_map_key(buf, off)?;
                let val = decode_value(buf, off, depth + 1)?;
                // A repeated key would violate the content-based `OrderedMap`
                // `Eq` (#909); reject rather than silently keeping the last
                // occurrence (#985).
                if map.contains_key(&key) {
                    return Err(DecodeError::DuplicateMapKey);
                }
                map.insert(key, val);
            }
            Ok(Value::map(map))
        }
        VAL_RECORD => {
            let shape = ShapeId(read_u32(buf, off)?);
            let len = read_u32(buf, off)? as usize;
            let mut fields = Vec::with_capacity(safe_capacity(len, buf.len(), *off, 1));
            for _ in 0..len {
                fields.push(decode_value(buf, off, depth + 1)?);
            }
            Ok(Value::record(shape, fields))
        }
        // Function values (T1c, `docs/format-v4-rfc.md` §1).
        VAL_FN_REF => Ok(Value::FnRef(read_def_id(buf, off)?)),
        VAL_CLOSURE => {
            let target = read_def_id(buf, off)?;
            let count = read_u16(buf, off)? as usize;
            let mut env = Vec::with_capacity(safe_capacity(count, buf.len(), *off, 4));
            for _ in 0..count {
                let name = NameId(read_u16(buf, off)?);
                let is_ref = read_u8(buf, off)? != 0;
                let payload = decode_value(buf, off, depth + 1)?;
                env.push(ClosureEnvEntry {
                    name,
                    is_ref,
                    payload,
                });
            }
            Ok(Value::closure(target, env))
        }
        // Handle values (T1d, `docs/format-v4-rfc.md` §1).
        VAL_HANDLE => {
            let kind = NameId(read_u16(buf, off)?);
            let id = read_u64(buf, off)?;
            Ok(Value::handle(kind, id))
        }
        // Projection values (T1e, `docs/format-v4-rfc.md` §1). Segment kind
        // `2=range` is RESERVED — `decode_proj_segment` rejects it since no
        // `ProjSegment` variant exists to decode into (`docs/t1e-spec.md` §3).
        VAL_PROJECTION => {
            let cell = read_def_id(buf, off)?;
            let count = read_u8(buf, off)? as usize;
            let mut segments = Vec::with_capacity(safe_capacity(count, buf.len(), *off, 1));
            for _ in 0..count {
                segments.push(decode_proj_segment(buf, off, depth + 1)?);
            }
            Ok(Value::projection(cell, segments))
        }
        // Option values (NS-A1, `docs/stdlib-spec.md` §1.4): flag byte
        // (0 = none, 1 = some) then the inner value when some. Any other
        // flag byte is corrupt input. Depth-counted like the collection
        // tags — a crafted chain of nested `some`s is the same recursion
        // shape as nested single-element arrays.
        VAL_OPTION => match read_u8(buf, off)? {
            0 => Ok(Value::none()),
            1 => Ok(Value::some(decode_value(buf, off, depth + 1)?)),
            other => Err(DecodeError::InvalidValueType(other)),
        },
        // Range values (NS-A5, F7): start i32, end i32, inclusive flag.
        // Flat — a range holds two ints, never another value, so there is
        // no recursion and no depth accounting. Any flag byte other than
        // 0/1 is corrupt input.
        VAL_RANGE => {
            let start = read_i32(buf, off)?;
            let end = read_i32(buf, off)?;
            let inclusive = match read_u8(buf, off)? {
                0 => false,
                1 => true,
                other => return Err(DecodeError::InvalidValueType(other)),
            };
            Ok(Value::range(start, end, inclusive))
        }
        // Tower values (NS-A8, `docs/tower-mini-spec.md` T5): explicit
        // little-endian f32 lanes in the pinned order (vec/quat `x, y(, z,
        // w)`; matrices column-major), rebuilt through glam's explicit
        // array constructors. Fixed sizes — no counts, no recursion, no
        // depth concerns (tower values are leaves).
        VAL_VEC2 => Ok(Value::Vec2(glam::Vec2::from_array(read_f32_lanes::<2>(
            buf, off,
        )?))),
        VAL_VEC3 => Ok(Value::Vec3(glam::Vec3::from_array(read_f32_lanes::<3>(
            buf, off,
        )?))),
        VAL_VEC4 => Ok(Value::Vec4(glam::Vec4::from_array(read_f32_lanes::<4>(
            buf, off,
        )?))),
        VAL_QUAT => Ok(Value::Quat(glam::Quat::from_array(read_f32_lanes::<4>(
            buf, off,
        )?))),
        VAL_MAT2 => Ok(Value::Mat2(glam::Mat2::from_cols_array(&read_f32_lanes::<
            4,
        >(
            buf, off
        )?))),
        VAL_MAT3 => Ok(Value::Mat3(glam::Mat3::from_cols_array(&read_f32_lanes::<
            9,
        >(
            buf, off
        )?))),
        VAL_MAT4 => Ok(Value::Mat4(glam::Mat4::from_cols_array(&read_f32_lanes::<
            16,
        >(
            buf, off
        )?))),
        // Weighted tables (NS-A7, `docs/stdlib-spec.md` §8): u32 entry
        // count, then per entry an i32 weight and a recursively-decoded
        // value. Depth-counted like the collection tags. The §8
        // evidence-by-construction invariant is enforced HERE too: an
        // empty table or a non-positive weight is corrupt input (a
        // `Weighted` never enters the runtime invalid, even from a
        // crafted file).
        VAL_WEIGHTED => {
            let count = read_u32(buf, off)?;
            if count == 0 {
                return Err(DecodeError::InvalidValueType(VAL_WEIGHTED));
            }
            let mut entries = Vec::with_capacity(safe_capacity(count as usize, buf.len(), *off, 5));
            for _ in 0..count {
                let weight = read_i32(buf, off)?;
                if weight < 1 {
                    return Err(DecodeError::InvalidValueType(VAL_WEIGHTED));
                }
                let value = decode_value(buf, off, depth + 1)?;
                entries.push((weight, value));
            }
            Ok(Value::weighted(entries))
        }
        _ => Err(DecodeError::InvalidValueType(tag)),
    }
}

/// Decode a single [`crate::ProjSegment`] written by `encode_proj_segment`.
fn decode_proj_segment(
    buf: &[u8],
    off: &mut usize,
    depth: usize,
) -> Result<crate::ProjSegment, DecodeError> {
    let kind = read_u8(buf, off)?;
    match kind {
        PROJ_SEG_INDEX => Ok(crate::ProjSegment::Index(read_i32(buf, off)?)),
        PROJ_SEG_KEY => Ok(crate::ProjSegment::Key(decode_value(buf, off, depth)?)),
        other => Err(DecodeError::InvalidProjSegmentKind(other)),
    }
}

/// Decode a [`MapKey`] written by `encode_map_key`: a scalar `VAL_*` tag
/// (`int`/`string`/`bool`) then its payload. The strict reader rejects any
/// other tag — only the v1 key domain is permitted (`docs/value-model-spec.md` §4).
fn decode_map_key(buf: &[u8], off: &mut usize) -> Result<MapKey, DecodeError> {
    let tag = read_u8(buf, off)?;
    match tag {
        VAL_INT => Ok(MapKey::Int(read_i32(buf, off)?)),
        VAL_STRING => Ok(MapKey::Str(read_str(buf, off)?.into())),
        VAL_BOOL => Ok(MapKey::Bool(read_u8(buf, off)? != 0)),
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

/// Read the T1b literal pool from a complete `.inkb` file using its index.
/// Absent section (older-shaped buffer within the same version) decodes as
/// empty, mirroring [`read_section_list_literals`].
pub fn read_section_literal_pool(buf: &[u8], index: &InkbIndex) -> Result<Vec<Value>, DecodeError> {
    let Some(range) = index.section_range(SectionKind::LiteralPool) else {
        return Ok(Vec::new());
    };
    let mut off = range.start;
    let count = read_u32(buf, &mut off)? as usize;
    let mut pool = Vec::with_capacity(safe_capacity(count, buf.len(), off, 1));
    for _ in 0..count {
        pool.push(decode_value(buf, &mut off, 0)?);
    }
    Ok(pool)
}

/// Read the TM-4 `StructShapes` section from a complete `.inkb` file using
/// its index. Absent section decodes as empty, mirroring
/// [`read_section_literal_pool`].
pub fn read_section_struct_shapes(
    buf: &[u8],
    index: &InkbIndex,
) -> Result<Vec<StructShapeDef>, DecodeError> {
    let Some(range) = index.section_range(SectionKind::StructShapes) else {
        return Ok(Vec::new());
    };
    let mut off = range.start;
    let count = read_u32(buf, &mut off)? as usize;
    let mut shapes = Vec::with_capacity(safe_capacity(count, buf.len(), off, 8));
    for _ in 0..count {
        let id = ShapeId(read_u32(buf, &mut off)?);
        let name = NameId(read_u16(buf, &mut off)?);
        let field_count = read_u16(buf, &mut off)? as usize;
        let mut fields = Vec::with_capacity(safe_capacity(field_count, buf.len(), off, 2));
        for _ in 0..field_count {
            fields.push(NameId(read_u16(buf, &mut off)?));
        }
        shapes.push(StructShapeDef { id, name, fields });
    }
    Ok(shapes)
}

/// Read the M-2b `Visibility` section (tag `0x0E`) from a complete `.inkb`
/// file using its index — the `DefinitionId`s of every `#@private`
/// definition. Absent section decodes as empty (the all-public common case),
/// mirroring [`read_section_struct_shapes`].
pub fn read_section_visibility(
    buf: &[u8],
    index: &InkbIndex,
) -> Result<Vec<DefinitionId>, DecodeError> {
    let Some(range) = index.section_range(SectionKind::Visibility) else {
        return Ok(Vec::new());
    };
    let mut off = range.start;
    let count = read_u32(buf, &mut off)? as usize;
    let mut ids = Vec::with_capacity(safe_capacity(count, buf.len(), off, 4));
    for _ in 0..count {
        ids.push(read_def_id(buf, &mut off)?);
    }
    Ok(ids)
}

/// Read the M-3 `AliasTable` section (`docs/modules-spec.md` §5) from a
/// complete `.inkb` file using its index. Absent section (a pre-M-3 file, or
/// a story with no `#@was` directives) decodes as empty, mirroring
/// [`read_section_literal_pool`]. The section-local version byte is checked
/// independently of the whole-file `VERSION` — see [`ALIAS_TABLE_SECTION_VERSION`].
pub fn read_section_alias_table(
    buf: &[u8],
    index: &InkbIndex,
) -> Result<Vec<AliasEntry>, DecodeError> {
    let Some(range) = index.section_range(SectionKind::AliasTable) else {
        return Ok(Vec::new());
    };
    let mut off = range.start;
    let section_version = read_u8(buf, &mut off)?;
    if section_version != ALIAS_TABLE_SECTION_VERSION {
        return Err(DecodeError::UnsupportedSectionVersion {
            section: SectionKind::AliasTable as u8,
            version: section_version,
        });
    }
    let count = read_u32(buf, &mut off)? as usize;
    let mut entries = Vec::with_capacity(safe_capacity(count, buf.len(), off, 16));
    for _ in 0..count {
        let old = read_def_id(buf, &mut off)?;
        let new = read_def_id(buf, &mut off)?;
        entries.push(AliasEntry { old, new });
    }
    Ok(entries)
}

/// Read the T2-3 `EffectRows` section (`docs/effects-spec.md` §11) from a
/// complete `.inkb` file using its index. Absent section (converter output, or
/// a story compiled before this slice) decodes as empty, mirroring
/// [`read_section_alias_table`]. The section-local version byte is checked
/// independently of the whole-file `VERSION` — see [`EFFECT_ROWS_SECTION_VERSION`].
pub fn read_section_effect_rows(
    buf: &[u8],
    index: &InkbIndex,
) -> Result<Vec<EffectRowEntry>, DecodeError> {
    let Some(range) = index.section_range(SectionKind::EffectRows) else {
        return Ok(Vec::new());
    };
    let mut off = range.start;
    let section_version = read_u8(buf, &mut off)?;
    if section_version != EFFECT_ROWS_SECTION_VERSION {
        return Err(DecodeError::UnsupportedSectionVersion {
            section: SectionKind::EffectRows as u8,
            version: section_version,
        });
    }
    let count = read_u32(buf, &mut off)? as usize;
    // Minimum per-entry footprint: def_id(8) + is_entry(1) +
    // direct(3×u32 counts + opaque + dims) + dispatch count(4) =
    // 8 + 1 + 13 + 1 + 4 = 27 bytes.
    let mut rows = Vec::with_capacity(safe_capacity(count, buf.len(), off, 27));
    for _ in 0..count {
        let def = read_def_id(buf, &mut off)?;
        // #882 freeze bit — see `EffectRowEntry::is_entry`'s doc.
        let is_entry = read_u8(buf, &mut off)? != 0;
        let direct = decode_direct_effects(buf, &mut off)?;
        let dispatch_count = read_u32(buf, &mut off)? as usize;
        let mut dispatches = Vec::with_capacity(safe_capacity(dispatch_count, buf.len(), off, 13));
        for _ in 0..dispatch_count {
            let cell = read_def_id(buf, &mut off)?;
            let narrowable = read_u8(buf, &mut off)? != 0;
            let fallback = decode_direct_effects(buf, &mut off)?;
            dispatches.push(DispatchEntry {
                cell,
                narrowable,
                fallback,
            });
        }
        rows.push(EffectRowEntry {
            def,
            is_entry,
            direct,
            dispatches,
        });
    }
    Ok(rows)
}

/// Read the FS-3 `FrameShapes` section (`docs/flow-suspension-spec.md`
/// §4/§11) from a complete `.inkb` file using its index. Absent section (every
/// story compiled behind the E052 fence, and all converter output) decodes as
/// empty, mirroring [`read_section_visibility`]. The section-local version byte
/// is checked independently of the whole-file `VERSION` — see
/// [`FRAME_SHAPES_SECTION_VERSION`].
pub fn read_section_frame_shapes(
    buf: &[u8],
    index: &InkbIndex,
) -> Result<Vec<FrameShapeDef>, DecodeError> {
    let Some(range) = index.section_range(SectionKind::FrameShapes) else {
        return Ok(Vec::new());
    };
    let mut off = range.start;
    let section_version = read_u8(buf, &mut off)?;
    if section_version != FRAME_SHAPES_SECTION_VERSION {
        return Err(DecodeError::UnsupportedSectionVersion {
            section: SectionKind::FrameShapes as u8,
            version: section_version,
        });
    }
    let count = read_u32(buf, &mut off)? as usize;
    // Minimum per-entry footprint: site def_id(8) + slot count(4) = 12 bytes.
    let mut shapes = Vec::with_capacity(safe_capacity(count, buf.len(), off, 12));
    for _ in 0..count {
        let site = read_def_id(buf, &mut off)?;
        let slot_count = read_u32(buf, &mut off)? as usize;
        let mut slots = Vec::with_capacity(safe_capacity(slot_count, buf.len(), off, 2));
        for _ in 0..slot_count {
            slots.push(NameId(read_u16(buf, &mut off)?));
        }
        shapes.push(FrameShapeDef { site, slots });
    }
    Ok(shapes)
}

/// Decode a [`DirectEffects`] block written by `encode_direct_effects`.
fn decode_direct_effects(buf: &[u8], off: &mut usize) -> Result<DirectEffects, DecodeError> {
    let read_count = read_u32(buf, off)? as usize;
    let mut reads = Vec::with_capacity(safe_capacity(read_count, buf.len(), *off, 8));
    for _ in 0..read_count {
        reads.push(read_def_id(buf, off)?);
    }
    let write_count = read_u32(buf, off)? as usize;
    let mut writes = Vec::with_capacity(safe_capacity(write_count, buf.len(), *off, 8));
    for _ in 0..write_count {
        writes.push(read_def_id(buf, off)?);
    }
    let call_count = read_u32(buf, off)? as usize;
    let mut calls = Vec::with_capacity(safe_capacity(call_count, buf.len(), *off, 4));
    for _ in 0..call_count {
        calls.push(decode_call_atom(buf, off)?);
    }
    let opaque = read_u8(buf, off)? != 0;
    // NS-A2 extension-flags byte (section version 3): the strict reader
    // rejects reserved bits (3–7) until a section version graduates them —
    // the same reservation discipline the capability/handle slots follow.
    let dims = read_u8(buf, off)?;
    if dims & !super::EFFECT_DIM_KNOWN_MASK != 0 {
        return Err(DecodeError::InvalidEffectDimensions(dims));
    }
    Ok(DirectEffects {
        reads,
        writes,
        calls,
        opaque,
        emits: dims & super::EFFECT_DIM_EMITS != 0,
        tags: dims & super::EFFECT_DIM_TAGS != 0,
        faults: dims & super::EFFECT_DIM_FAULTS != 0,
    })
}

/// Decode a single [`CallAtom`] written by `encode_call_atom`. The strict
/// reader rejects a non-`Any` capability tag (path-granular is reserved, #826)
/// and a non-`None` handle-parameter slot (reserved, `docs/t1d-spec.md` §7) —
/// the same reservation discipline the projection range segment follows.
fn decode_call_atom(buf: &[u8], off: &mut usize) -> Result<CallAtom, DecodeError> {
    let name = NameId(read_u16(buf, off)?);
    let cap_tag = read_u8(buf, off)?;
    let capability = match cap_tag {
        CAP_PARAM_ANY => CapabilityParam::Any,
        other => return Err(DecodeError::InvalidEffectCapParam(other)),
    };
    let handle_tag = read_u8(buf, off)?;
    if handle_tag != HANDLE_PARAM_NONE {
        return Err(DecodeError::InvalidEffectHandleParam(handle_tag));
    }
    Ok(CallAtom {
        name,
        capability,
        handle_param: None,
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
    let scope_id = read_def_id(buf, off)?;
    let has_name = read_u8(buf, off)? != 0;
    let name = if has_name {
        Some(NameId(read_u16(buf, off)?))
    } else {
        None
    };
    let counting_bits = read_u8(buf, off)?;
    let counting_flags = CountingFlags::from_bits(counting_bits).unwrap_or(CountingFlags::empty());
    let path_hash = read_i32(buf, off)?;
    let param_count = read_u8(buf, off)?;
    let local = read_u8(buf, off)? != 0;
    // Per-param name/mode metadata (T1c, `docs/t1c-spec.md` §6).
    let param_meta_count = read_u16(buf, off)? as usize;
    let mut params = Vec::with_capacity(safe_capacity(param_meta_count, buf.len(), *off, 3));
    for _ in 0..param_meta_count {
        let name = NameId(read_u16(buf, off)?);
        let is_ref = read_u8(buf, off)? != 0;
        params.push(ParamMeta { name, is_ref });
    }
    // `ContainerDef::params`'s doc invariant: `params.len()` always equals
    // `param_count` whenever per-param metadata is present at all (empty
    // `params` is the separate, legitimate "count only, no metadata" case).
    // A mutated `.inkb` asserting otherwise is malformed input, not
    // silently-acceptable data — mirrors the `.inkt` reader's guard (#745,
    // #954), rejecting with a decode error rather than constructing an
    // inconsistent `ContainerDef`.
    if !params.is_empty() && params.len() != usize::from(param_count) {
        return Err(DecodeError::ParamCountMismatch {
            declared: param_count,
            actual: params.len(),
        });
    }

    let bytecode_len = read_u32(buf, off)? as usize;
    if *off + bytecode_len > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let bytecode = buf[*off..*off + bytecode_len].to_vec();
    *off += bytecode_len;

    Ok(ContainerDef {
        id,
        scope_id,
        name,
        bytecode,
        counting_flags,
        path_hash,
        param_count,
        params,
        local,
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
    let has_audio = read_u8(buf, off)? != 0;
    let audio_ref = if has_audio {
        Some(read_str(buf, off)?)
    } else {
        None
    };
    // Slot info
    let slot_count = read_u8(buf, off)? as usize;
    let mut slot_info = Vec::with_capacity(slot_count);
    for _ in 0..slot_count {
        let index = read_u8(buf, off)?;
        let name = read_str(buf, off)?;
        slot_info.push(SlotInfo { index, name });
    }

    // Source location
    let has_source_loc = read_u8(buf, off)? != 0;
    let source_location = if has_source_loc {
        let file = read_str(buf, off)?;
        let range_start = read_u32(buf, off)?;
        let range_end = read_u32(buf, off)?;
        Some(SourceLocation {
            file,
            range_start,
            range_end,
        })
    } else {
        None
    };

    let flags = crate::LineFlags::from_content(&content);
    Ok(LineEntry {
        content,
        flags,
        source_hash,
        audio_ref,
        slot_info,
        source_location,
    })
}

pub(crate) fn decode_line_content(buf: &[u8], off: &mut usize) -> Result<LineContent, DecodeError> {
    let tag = read_u8(buf, off)?;
    match tag {
        LINE_PLAIN => Ok(LineContent::Plain(read_str(buf, off)?)),
        LINE_TEMPLATE => {
            let part_count = read_u32(buf, off)? as usize;
            let mut parts = Vec::with_capacity(safe_capacity(part_count, buf.len(), *off, 2));
            for _ in 0..part_count {
                parts.push(decode_line_part(buf, off, 0)?);
            }
            Ok(LineContent::Template(parts))
        }
        _ => Err(DecodeError::InvalidLineContent(tag)),
    }
}

/// `depth` guards against a crafted file of deeply nested `LinePart::Span`s
/// (#1716) blowing the stack — the same `MAX_DECODE_DEPTH` cap
/// `decode_value` enforces for `VAL_ARRAY`/`VAL_MAP`/etc., since `Span` is
/// now the one `LinePart` shape that recurses.
fn decode_line_part(buf: &[u8], off: &mut usize, depth: usize) -> Result<LinePart, DecodeError> {
    if depth > MAX_DECODE_DEPTH {
        return Err(DecodeError::MaxDepthExceeded(MAX_DECODE_DEPTH));
    }
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
        PART_SPAN => {
            let name = read_str(buf, off)?;
            let attr_count = read_u32(buf, off)? as usize;
            // Each attr is two `write_str`-encoded strings, minimum 4 bytes
            // (an empty string's length prefix) apiece.
            let attrs_cap = safe_capacity(attr_count, buf.len(), *off, 8);
            let mut attrs = Vec::with_capacity(attrs_cap);
            for _ in 0..attr_count {
                let k = read_str(buf, off)?;
                let v = read_str(buf, off)?;
                attrs.push((k, v));
            }
            let child_count = read_u32(buf, off)? as usize;
            let mut children = Vec::with_capacity(safe_capacity(child_count, buf.len(), *off, 2));
            for _ in 0..child_count {
                children.push(decode_line_part(buf, off, depth + 1)?);
            }
            Ok(LinePart::Span {
                name,
                attrs,
                children,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{write_u8, write_u32};

    /// Hand-build a `VAL_MAP` payload carrying the same `int` key twice — no
    /// legitimate encoder emits this (`OrderedMap::insert` de-duplicates on
    /// the write side), so this is the "crafted payload" scenario issue #985
    /// guards against: the reader must reject it with a decode error, never
    /// construct an `OrderedMap` that violates the content-based `Eq`
    /// invariant (#909) by silently keeping the last occurrence.
    fn duplicate_int_key_map_bytes() -> Vec<u8> {
        let mut buf = Vec::new();
        write_u8(&mut buf, VAL_MAP);
        write_u32(&mut buf, 2); // two entries
        // entry 0: key = int(0), value = int(1)
        write_u8(&mut buf, VAL_INT);
        buf.extend_from_slice(&0i32.to_le_bytes());
        write_u8(&mut buf, VAL_INT);
        buf.extend_from_slice(&1i32.to_le_bytes());
        // entry 1: key = int(0) again, value = int(2)
        write_u8(&mut buf, VAL_INT);
        buf.extend_from_slice(&0i32.to_le_bytes());
        write_u8(&mut buf, VAL_INT);
        buf.extend_from_slice(&2i32.to_le_bytes());
        buf
    }

    #[test]
    fn decode_value_rejects_duplicate_map_key() {
        let buf = duplicate_int_key_map_bytes();
        let mut off = 0;
        assert_eq!(
            decode_value(&buf, &mut off, 0),
            Err(DecodeError::DuplicateMapKey)
        );
    }

    #[test]
    fn decode_value_accepts_distinct_map_keys() {
        let mut buf = Vec::new();
        write_u8(&mut buf, VAL_MAP);
        write_u32(&mut buf, 2);
        write_u8(&mut buf, VAL_INT);
        buf.extend_from_slice(&0i32.to_le_bytes());
        write_u8(&mut buf, VAL_INT);
        buf.extend_from_slice(&1i32.to_le_bytes());
        write_u8(&mut buf, VAL_INT);
        buf.extend_from_slice(&5i32.to_le_bytes());
        write_u8(&mut buf, VAL_INT);
        buf.extend_from_slice(&2i32.to_le_bytes());

        let mut off = 0;
        let value = decode_value(&buf, &mut off, 0).expect("distinct keys decode cleanly");
        let Value::Map(map) = value else {
            unreachable!("expected a map value");
        };
        assert_eq!(map.len(), 2);
    }
}
