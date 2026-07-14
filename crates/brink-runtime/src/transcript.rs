//! Transcript binary serialization (`.brkt` format).
//!
//! A transcript is a serialized `Vec<OutputPart>` — the append-only log of
//! all output parts produced during story execution. Combined with an `.inkb`
//! program and optional `.inkl` locale data, a transcript can be re-rendered
//! in any language without re-executing the story.
//!
//! ## Binary format
//!
//! ```text
//! Header (16 bytes):
//!   b"BRKT"           magic (4)
//!   u16 LE            version = 1 (2)
//!   u16 LE            reserved (2)
//!   u32 LE            source_checksum (4)
//!   u32 LE            content CRC-32 (4)
//!
//! Body:
//!   u32 LE            part count
//!   [Part]*           encoded parts
//! ```

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use brink_format::{DefinitionId, LineFlags, MAX_DECODE_DEPTH, MapKey, NameId, OrderedMap, Value};

use crate::output::{OutputPart, resolve_lines};
use crate::program::Program;

// ── Format constants ──────────────────────────────────────────────────────

const MAGIC: &[u8; 4] = b"BRKT";
const VERSION: u16 = 1;
const HEADER_SIZE: usize = 16;

// Part tags
const TAG_TEXT: u8 = 0x01;
const TAG_LINE_REF: u8 = 0x02;
const TAG_VALUE_REF: u8 = 0x03;
const TAG_NEWLINE: u8 = 0x04;
const TAG_SPRING: u8 = 0x05;
const TAG_GLUE: u8 = 0x06;
const TAG_TAG: u8 = 0x07;

// Value tags (matching inkb encoding)
const VAL_INT: u8 = 0x00;
const VAL_FLOAT: u8 = 0x01;
const VAL_BOOL: u8 = 0x02;
const VAL_STRING: u8 = 0x03;
const VAL_LIST: u8 = 0x04;
const VAL_DIVERT_TARGET: u8 = 0x05;
const VAL_NULL: u8 = 0x06;
// Shared numeric surface with the `.inkb` format (`inkb::VAL_VAR_POINTER`). A
// `VariablePointer` is preserved distinctly (T1c, #700) so a `ref`-bound
// closure env entry round-trips losslessly — the old code collapsed it to
// `VAL_DIVERT_TARGET`, which is fine for a bare pointer (display-identical) but
// would corrupt a captured `ref` cell inside a persisted function value.
const VAL_VAR_POINTER: u8 = 0x07;
const VAL_FRAGMENT_REF: u8 = 0x08;
// v4 collection tags — shared numeric surface with the `.inkb` format
// (`docs/format-v4-rfc.md` §1). Reachable today via `Opcode::EmitValue`, which
// pushes any popped stack value (including a binding/external return that since
// #525 can be a collection) into `OutputPart::ValueRef`.
const VAL_ARRAY: u8 = 0x09;
const VAL_MAP: u8 = 0x0A;
// T1c function-value tags — shared numeric surface with the `.inkb` format
// (`docs/format-v4-rfc.md` §1). A function value can ride the append-only
// transcript / journal / speculation snapshots as an ordinary value (spec §6:
// "function values save like every other value"), e.g. via `Opcode::EmitValue`
// or a saved binding.
const VAL_FN_REF: u8 = 0x0B;
const VAL_CLOSURE: u8 = 0x0C;
// T1d handle tag — shared numeric surface with the `.inkb` format
// (`docs/format-v4-rfc.md` §1: `kind NameId, u64 id`). A handle received
// from a binding rides the append-only transcript / journal / speculation
// snapshots as an ordinary value (`docs/t1d-spec.md` §2: "handles appear in
// saves, journals, and speculation snapshots as ordinary values"), e.g. via
// `Opcode::EmitValue` or a saved binding.
const VAL_HANDLE: u8 = 0x0D;
// TM-4 record tag — shared numeric surface with the `.inkb` format
// (`docs/format-v4-rfc.md` §1: `ShapeId`, then field values in shape order).
const VAL_RECORD: u8 = 0x0F;

// ── Error type ────────────────────────────────────────────────────────────

/// Errors from transcript serialization/deserialization.
#[derive(Debug, thiserror::Error)]
pub enum TranscriptError {
    #[error("invalid magic: expected BRKT")]
    InvalidMagic,
    #[error("unsupported version: {0}")]
    UnsupportedVersion(u16),
    #[error("checksum mismatch: transcript {transcript:#010x} != program {program:#010x}")]
    ChecksumMismatch { transcript: u32, program: u32 },
    #[error("integrity check failed: content CRC-32 mismatch")]
    IntegrityCheckFailed,
    #[error("unexpected end of data")]
    UnexpectedEof,
    #[error("invalid part tag: {0:#04x}")]
    InvalidPartTag(u8),
    #[error("invalid value tag: {0:#04x}")]
    InvalidValueTag(u8),
    #[error("invalid UTF-8")]
    InvalidUtf8,
    #[error("invalid definition ID")]
    InvalidDefinitionId,
    #[error("value nesting exceeded max decode depth ({0})")]
    MaxDepthExceeded(usize),
}

// ── Write ─────────────────────────────────────────────────────────────────

/// Serialize a transcript to the `.brkt` binary format.
///
/// Checkpoint parts are filtered out (they are transient capture markers
/// that should never appear in a persisted transcript).
#[expect(clippy::cast_possible_truncation)]
pub fn write_transcript(
    parts: &[OutputPart],
    source_checksum: u32,
    fragments: &[crate::output::Fragment],
) -> Vec<u8> {
    let mut body = Vec::new();

    // Count non-Checkpoint parts
    let count = parts
        .iter()
        .filter(|p| !matches!(p, OutputPart::Checkpoint))
        .count() as u32;
    write_u32(&mut body, count);

    for part in parts {
        match part {
            OutputPart::Text(s) => {
                write_u8(&mut body, TAG_TEXT);
                write_str(&mut body, s);
            }
            OutputPart::LineRef {
                container_idx,
                line_idx,
                slots,
                flags,
            } => {
                write_u8(&mut body, TAG_LINE_REF);
                write_u32(&mut body, *container_idx);
                write_u16(&mut body, *line_idx);
                write_u8(&mut body, flags.bits());
                write_u16(&mut body, slots.len() as u16);
                for val in slots {
                    encode_value(val, &mut body);
                }
            }
            OutputPart::ValueRef(val) => {
                write_u8(&mut body, TAG_VALUE_REF);
                encode_value(val, &mut body);
            }
            OutputPart::Newline => write_u8(&mut body, TAG_NEWLINE),
            OutputPart::Spring => write_u8(&mut body, TAG_SPRING),
            OutputPart::Glue => write_u8(&mut body, TAG_GLUE),
            OutputPart::Tag(s) => {
                write_u8(&mut body, TAG_TAG);
                write_str(&mut body, s);
            }
            OutputPart::Checkpoint => {} // filtered out
        }
    }

    // Serialize fragments
    write_u32(&mut body, fragments.len() as u32);
    for fragment in fragments {
        let filtered_count = fragment
            .parts
            .iter()
            .filter(|p| !matches!(p, OutputPart::Checkpoint))
            .count() as u32;
        write_u32(&mut body, filtered_count);
        for part in &fragment.parts {
            match part {
                OutputPart::Text(s) => {
                    write_u8(&mut body, TAG_TEXT);
                    write_str(&mut body, s);
                }
                OutputPart::LineRef {
                    container_idx,
                    line_idx,
                    slots,
                    flags,
                } => {
                    write_u8(&mut body, TAG_LINE_REF);
                    write_u32(&mut body, *container_idx);
                    write_u16(&mut body, *line_idx);
                    write_u8(&mut body, flags.bits());
                    write_u16(&mut body, slots.len() as u16);
                    for val in slots {
                        encode_value(val, &mut body);
                    }
                }
                OutputPart::ValueRef(val) => {
                    write_u8(&mut body, TAG_VALUE_REF);
                    encode_value(val, &mut body);
                }
                OutputPart::Newline => write_u8(&mut body, TAG_NEWLINE),
                OutputPart::Spring => write_u8(&mut body, TAG_SPRING),
                OutputPart::Glue => write_u8(&mut body, TAG_GLUE),
                OutputPart::Tag(s) => {
                    write_u8(&mut body, TAG_TAG);
                    write_str(&mut body, s);
                }
                OutputPart::Checkpoint => {}
            }
        }
    }

    // Build header
    let content_crc = crc32(&body);
    let mut buf = Vec::with_capacity(HEADER_SIZE + body.len());
    buf.extend_from_slice(MAGIC);
    write_u16(&mut buf, VERSION);
    write_u16(&mut buf, 0); // reserved
    write_u32(&mut buf, source_checksum);
    write_u32(&mut buf, content_crc);
    buf.extend(body);
    buf
}

// ── Read ──────────────────────────────────────────────────────────────────

/// A decoded transcript: the output parts, the source program's checksum
/// (to verify compatibility before rendering), and the captured fragments
/// (for re-rendering choice display text and computed substrings).
///
/// The caller should validate `source_checksum` against the program's
/// checksum (via [`Program::source_checksum`](crate::Program::source_checksum))
/// before passing `parts` to [`render_transcript`].
#[derive(Debug, Clone)]
pub struct TranscriptData {
    pub parts: Vec<OutputPart>,
    pub source_checksum: u32,
    pub fragments: Vec<crate::output::Fragment>,
}

/// Deserialize a transcript from the `.brkt` binary format.
pub fn read_transcript(bytes: &[u8]) -> Result<TranscriptData, TranscriptError> {
    if bytes.len() < HEADER_SIZE {
        return Err(TranscriptError::UnexpectedEof);
    }

    // Validate header
    if &bytes[0..4] != MAGIC {
        return Err(TranscriptError::InvalidMagic);
    }
    let mut off = 4;
    let version = read_u16(bytes, &mut off)?;
    if version != VERSION {
        return Err(TranscriptError::UnsupportedVersion(version));
    }
    let _reserved = read_u16(bytes, &mut off)?;
    let source_checksum = read_u32(bytes, &mut off)?;
    let expected_crc = read_u32(bytes, &mut off)?;

    // Validate body integrity
    let body = &bytes[HEADER_SIZE..];
    if crc32(body) != expected_crc {
        return Err(TranscriptError::IntegrityCheckFailed);
    }

    // Decode parts
    let mut off = HEADER_SIZE;
    let count = read_u32(bytes, &mut off)? as usize;
    let mut parts = Vec::with_capacity(count);

    for _ in 0..count {
        let tag = read_u8(bytes, &mut off)?;
        let part = match tag {
            TAG_TEXT => OutputPart::Text(read_str(bytes, &mut off)?),
            TAG_LINE_REF => {
                let container_idx = read_u32(bytes, &mut off)?;
                let line_idx = read_u16(bytes, &mut off)?;
                let flags_bits = read_u8(bytes, &mut off)?;
                let flags = LineFlags::from_bits_truncate(flags_bits);
                let slot_count = read_u16(bytes, &mut off)? as usize;
                let mut slots = Vec::with_capacity(slot_count);
                for _ in 0..slot_count {
                    slots.push(decode_value(bytes, &mut off, 0)?);
                }
                OutputPart::LineRef {
                    container_idx,
                    line_idx,
                    slots,
                    flags,
                }
            }
            TAG_VALUE_REF => OutputPart::ValueRef(decode_value(bytes, &mut off, 0)?),
            TAG_NEWLINE => OutputPart::Newline,
            TAG_SPRING => OutputPart::Spring,
            TAG_GLUE => OutputPart::Glue,
            TAG_TAG => OutputPart::Tag(read_str(bytes, &mut off)?),
            _ => return Err(TranscriptError::InvalidPartTag(tag)),
        };
        parts.push(part);
    }

    // Deserialize fragments
    let fragment_count = if off < bytes.len() {
        read_u32(bytes, &mut off)? as usize
    } else {
        0 // backward compat: old transcripts without fragments
    };
    let mut fragments = Vec::with_capacity(fragment_count);
    for _ in 0..fragment_count {
        let frag_part_count = read_u32(bytes, &mut off)? as usize;
        let mut frag_parts = Vec::with_capacity(frag_part_count);
        for _ in 0..frag_part_count {
            let tag = read_u8(bytes, &mut off)?;
            let part = match tag {
                TAG_TEXT => OutputPart::Text(read_str(bytes, &mut off)?),
                TAG_LINE_REF => {
                    let container_idx = read_u32(bytes, &mut off)?;
                    let line_idx = read_u16(bytes, &mut off)?;
                    let flags_bits = read_u8(bytes, &mut off)?;
                    let flags = LineFlags::from_bits_truncate(flags_bits);
                    let slot_count = read_u16(bytes, &mut off)? as usize;
                    let mut slots = Vec::with_capacity(slot_count);
                    for _ in 0..slot_count {
                        slots.push(decode_value(bytes, &mut off, 0)?);
                    }
                    OutputPart::LineRef {
                        container_idx,
                        line_idx,
                        slots,
                        flags,
                    }
                }
                TAG_VALUE_REF => OutputPart::ValueRef(decode_value(bytes, &mut off, 0)?),
                TAG_NEWLINE => OutputPart::Newline,
                TAG_SPRING => OutputPart::Spring,
                TAG_GLUE => OutputPart::Glue,
                TAG_TAG => OutputPart::Tag(read_str(bytes, &mut off)?),
                _ => return Err(TranscriptError::InvalidPartTag(tag)),
            };
            frag_parts.push(part);
        }
        fragments.push(crate::output::Fragment {
            parts: frag_parts,
            tags: Vec::new(),
        });
    }

    Ok(TranscriptData {
        parts,
        source_checksum,
        fragments,
    })
}

// ── Render ────────────────────────────────────────────────────────────────

/// Re-render a transcript against the given line tables.
///
/// Applies glue resolution, Spring spacing, and line trimming — the same
/// pipeline as `flush_lines` — producing `(text, tags)` tuples per line.
pub fn render_transcript(
    parts: &[OutputPart],
    program: &Program,
    line_tables: &[Vec<brink_format::LineEntry>],
    resolver: Option<&dyn brink_format::PluralResolver>,
    fragments: &[crate::output::Fragment],
) -> Vec<(String, Vec<String>)> {
    resolve_lines(parts, program, line_tables, resolver, fragments)
}

// ── Codec helpers (self-contained, no dependency on brink-format internals) ──

fn write_u8(buf: &mut Vec<u8>, v: u8) {
    buf.push(v);
}

fn write_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn write_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn write_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn write_i32(buf: &mut Vec<u8>, v: i32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

#[expect(clippy::cast_possible_truncation)]
fn write_str(buf: &mut Vec<u8>, s: &str) {
    write_u32(buf, s.len() as u32);
    buf.extend_from_slice(s.as_bytes());
}

fn write_def_id(buf: &mut Vec<u8>, id: DefinitionId) {
    write_u64(buf, id.to_raw());
}

fn read_u8(buf: &[u8], off: &mut usize) -> Result<u8, TranscriptError> {
    if *off >= buf.len() {
        return Err(TranscriptError::UnexpectedEof);
    }
    let v = buf[*off];
    *off += 1;
    Ok(v)
}

fn read_u16(buf: &[u8], off: &mut usize) -> Result<u16, TranscriptError> {
    if *off + 2 > buf.len() {
        return Err(TranscriptError::UnexpectedEof);
    }
    let v = u16::from_le_bytes([buf[*off], buf[*off + 1]]);
    *off += 2;
    Ok(v)
}

fn read_u32(buf: &[u8], off: &mut usize) -> Result<u32, TranscriptError> {
    if *off + 4 > buf.len() {
        return Err(TranscriptError::UnexpectedEof);
    }
    let v = u32::from_le_bytes([buf[*off], buf[*off + 1], buf[*off + 2], buf[*off + 3]]);
    *off += 4;
    Ok(v)
}

fn read_i32(buf: &[u8], off: &mut usize) -> Result<i32, TranscriptError> {
    if *off + 4 > buf.len() {
        return Err(TranscriptError::UnexpectedEof);
    }
    let v = i32::from_le_bytes([buf[*off], buf[*off + 1], buf[*off + 2], buf[*off + 3]]);
    *off += 4;
    Ok(v)
}

fn read_f32(buf: &[u8], off: &mut usize) -> Result<f32, TranscriptError> {
    if *off + 4 > buf.len() {
        return Err(TranscriptError::UnexpectedEof);
    }
    let v = f32::from_le_bytes([buf[*off], buf[*off + 1], buf[*off + 2], buf[*off + 3]]);
    *off += 4;
    Ok(v)
}

fn read_u64(buf: &[u8], off: &mut usize) -> Result<u64, TranscriptError> {
    if *off + 8 > buf.len() {
        return Err(TranscriptError::UnexpectedEof);
    }
    let v = u64::from_le_bytes([
        buf[*off],
        buf[*off + 1],
        buf[*off + 2],
        buf[*off + 3],
        buf[*off + 4],
        buf[*off + 5],
        buf[*off + 6],
        buf[*off + 7],
    ]);
    *off += 8;
    Ok(v)
}

fn read_str(buf: &[u8], off: &mut usize) -> Result<String, TranscriptError> {
    let len = read_u32(buf, off)? as usize;
    if *off + len > buf.len() {
        return Err(TranscriptError::UnexpectedEof);
    }
    let bytes = &buf[*off..*off + len];
    *off += len;
    String::from_utf8(bytes.to_vec()).map_err(|_| TranscriptError::InvalidUtf8)
}

fn read_def_id(buf: &[u8], off: &mut usize) -> Result<DefinitionId, TranscriptError> {
    let raw = read_u64(buf, off)?;
    DefinitionId::from_raw(raw).ok_or(TranscriptError::InvalidDefinitionId)
}

// ── Value encoding ────────────────────────────────────────────────────────

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
        Value::FragmentRef(idx) => {
            write_u8(buf, VAL_FRAGMENT_REF);
            write_u32(buf, *idx);
        }
        // TempPointer is runtime-only.
        Value::TempPointer { .. } | Value::Null => {
            write_u8(buf, VAL_NULL);
        }
        // Collections encode as trees (v4, `docs/format-v4-rfc.md` §1): a length
        // prefix then the recursively-encoded elements / key-value pairs. Arc
        // sharing is not preserved on the wire (value-model-spec §5).
        Value::Array(items) => {
            write_u8(buf, VAL_ARRAY);
            write_u32(buf, items.len() as u32);
            for item in items.iter() {
                encode_value(item, buf);
            }
        }
        Value::Map(map) => {
            write_u8(buf, VAL_MAP);
            write_u32(buf, map.len() as u32);
            for (key, val) in map.iter() {
                encode_map_key(key, buf);
                encode_value(val, buf);
            }
        }
        Value::Record { shape, fields } => {
            write_u8(buf, VAL_RECORD);
            write_u32(buf, shape.0);
            write_u32(buf, fields.len() as u32);
            for field in fields.iter() {
                encode_value(field, buf);
            }
        }
        // Function values (T1c, spec §6): save like every other value. `FnRef`
        // is the fn token; `Closure` adds a u32-counted env of `{NameId, kind
        // u8, value}` entries — the named/moded env the rehydration check reads.
        Value::FnRef(target) => {
            write_u8(buf, VAL_FN_REF);
            write_def_id(buf, *target);
        }
        Value::Closure(c) => {
            write_u8(buf, VAL_CLOSURE);
            write_def_id(buf, c.target);
            write_u32(buf, c.env.len() as u32);
            for entry in &c.env {
                write_u16(buf, entry.name.0);
                write_u8(buf, u8::from(entry.is_ref));
                encode_value(&entry.payload, buf);
            }
        }
        // Handle values (T1d, spec §5: "the journal records returned tokens";
        // §2: "handles appear in saves, journals, and speculation snapshots
        // as ordinary values"). Token equality holds at this level; rebinding
        // to a live resource happens at the host boundary, not here.
        Value::Handle { kind, id } => {
            write_u8(buf, VAL_HANDLE);
            write_u16(buf, kind.0);
            write_u64(buf, *id);
        }
    }
}

/// Encode a [`MapKey`] using the scalar `VAL_*` tag surface (`int`/`string`/
/// `bool` — the v1 key domain). Self-describing so the reader rejects a
/// non-scalar key tag.
fn encode_map_key(key: &MapKey, buf: &mut Vec<u8>) {
    match key {
        MapKey::Int(n) => {
            write_u8(buf, VAL_INT);
            write_i32(buf, *n);
        }
        MapKey::Str(s) => {
            write_u8(buf, VAL_STRING);
            write_str(buf, s);
        }
        MapKey::Bool(b) => {
            write_u8(buf, VAL_BOOL);
            write_u8(buf, u8::from(*b));
        }
    }
}

fn decode_value(buf: &[u8], off: &mut usize, depth: usize) -> Result<Value, TranscriptError> {
    if depth > MAX_DECODE_DEPTH {
        return Err(TranscriptError::MaxDepthExceeded(MAX_DECODE_DEPTH));
    }
    let tag = read_u8(buf, off)?;
    match tag {
        VAL_INT => Ok(Value::Int(read_i32(buf, off)?)),
        VAL_FLOAT => Ok(Value::Float(read_f32(buf, off)?)),
        VAL_BOOL => {
            let b = read_u8(buf, off)?;
            Ok(Value::Bool(b != 0))
        }
        VAL_STRING => {
            let s = read_str(buf, off)?;
            Ok(Value::String(Arc::from(s.as_str())))
        }
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
            Ok(Value::List(Arc::new(brink_format::ListValue {
                items,
                origins,
            })))
        }
        VAL_DIVERT_TARGET => {
            let id = read_def_id(buf, off)?;
            Ok(Value::DivertTarget(id))
        }
        VAL_VAR_POINTER => {
            let id = read_def_id(buf, off)?;
            Ok(Value::VariablePointer(id))
        }
        VAL_FRAGMENT_REF => Ok(Value::FragmentRef(read_u32(buf, off)?)),
        VAL_NULL => Ok(Value::Null),
        VAL_ARRAY => {
            let len = read_u32(buf, off)? as usize;
            let mut items = Vec::with_capacity(len.min(buf.len().saturating_sub(*off)));
            for _ in 0..len {
                items.push(decode_value(buf, off, depth + 1)?);
            }
            Ok(Value::array(items))
        }
        VAL_MAP => {
            let len = read_u32(buf, off)? as usize;
            let mut map = OrderedMap::with_capacity(len.min(buf.len().saturating_sub(*off)));
            for _ in 0..len {
                let key = decode_map_key(buf, off)?;
                let val = decode_value(buf, off, depth + 1)?;
                map.insert(key, val);
            }
            Ok(Value::map(map))
        }
        VAL_RECORD => {
            let shape = brink_format::ShapeId(read_u32(buf, off)?);
            let len = read_u32(buf, off)? as usize;
            let mut fields = Vec::with_capacity(len.min(buf.len().saturating_sub(*off)));
            for _ in 0..len {
                fields.push(decode_value(buf, off, depth + 1)?);
            }
            Ok(Value::record(shape, fields))
        }
        VAL_FN_REF => Ok(Value::FnRef(read_def_id(buf, off)?)),
        VAL_CLOSURE => {
            let target = read_def_id(buf, off)?;
            let count = read_u32(buf, off)? as usize;
            let mut env = Vec::with_capacity(count.min(buf.len().saturating_sub(*off)));
            for _ in 0..count {
                let name = brink_format::NameId(read_u16(buf, off)?);
                let is_ref = read_u8(buf, off)? != 0;
                let payload = decode_value(buf, off, depth + 1)?;
                env.push(brink_format::ClosureEnvEntry {
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
        _ => Err(TranscriptError::InvalidValueTag(tag)),
    }
}

/// Decode a [`MapKey`] written by `encode_map_key`: a scalar `VAL_*` tag then
/// its payload. Any other tag is rejected — only `int`/`string`/`bool` keys are
/// permitted (`docs/value-model-spec.md` §4).
fn decode_map_key(buf: &[u8], off: &mut usize) -> Result<MapKey, TranscriptError> {
    let tag = read_u8(buf, off)?;
    match tag {
        VAL_INT => Ok(MapKey::Int(read_i32(buf, off)?)),
        VAL_STRING => Ok(MapKey::Str(Arc::from(read_str(buf, off)?.as_str()))),
        VAL_BOOL => Ok(MapKey::Bool(read_u8(buf, off)? != 0)),
        _ => Err(TranscriptError::InvalidValueTag(tag)),
    }
}

// ── CRC-32 ────────────────────────────────────────────────────────────────

fn crc32(data: &[u8]) -> u32 {
    static TABLE: [u32; 256] = {
        let mut table = [0u32; 256];
        let mut i = 0u32;
        while i < 256 {
            let mut crc = i;
            let mut j = 0;
            while j < 8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xEDB8_8320;
                } else {
                    crc >>= 1;
                }
                j += 1;
            }
            table[i as usize] = crc;
            i += 1;
        }
        table
    };

    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        let idx = ((crc ^ u32::from(byte)) & 0xFF) as usize;
        crc = (crc >> 8) ^ TABLE[idx];
    }
    crc ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_format::LineFlags;

    #[test]
    fn round_trip_simple_parts() {
        let parts = vec![
            OutputPart::Text("Hello".to_string()),
            OutputPart::Spring,
            OutputPart::Newline,
            OutputPart::Tag("tag1".to_string()),
            OutputPart::Glue,
        ];
        let bytes = write_transcript(&parts, 0xDEAD_BEEF, &[]);
        let data = read_transcript(&bytes).unwrap();
        assert_eq!(data.source_checksum, 0xDEAD_BEEF);
        assert_eq!(data.parts.len(), 5);
        assert!(matches!(&data.parts[0], OutputPart::Text(s) if s == "Hello"));
        assert!(matches!(&data.parts[1], OutputPart::Spring));
        assert!(matches!(&data.parts[2], OutputPart::Newline));
        assert!(matches!(&data.parts[3], OutputPart::Tag(s) if s == "tag1"));
        assert!(matches!(&data.parts[4], OutputPart::Glue));
    }

    // A collection reaches the transcript through `Opcode::EmitValue`, which
    // pops any stack value — including a binding/external return that since #525
    // can be an `Array`/`Map` — into `OutputPart::ValueRef`. This locks the v4
    // tree encoding of that part: structural equality, insertion order, scalar
    // key types, and nesting all survive the `.brkt` round-trip (#526).
    #[test]
    fn round_trip_value_ref_collections() {
        use brink_format::{MapKey, OrderedMap};

        let map: OrderedMap = [
            (MapKey::from("name"), Value::String(Arc::from("goblin"))),
            (
                MapKey::from(1),
                Value::array(vec![Value::Int(10), Value::Int(20)]),
            ),
            (MapKey::from(true), Value::Bool(false)),
        ]
        .into_iter()
        .collect();
        let array = Value::array(vec![
            Value::Int(1),
            Value::String(Arc::from("two")),
            Value::map(map.clone()),
            Value::Null,
        ]);

        let parts = vec![
            OutputPart::ValueRef(array.clone()),
            OutputPart::ValueRef(Value::map(map.clone())),
        ];
        let bytes = write_transcript(&parts, 42, &[]);
        let data = read_transcript(&bytes).unwrap();

        assert_eq!(data.parts.len(), 2);
        match &data.parts[0] {
            OutputPart::ValueRef(v) => assert_eq!(*v, array),
            other => unreachable!("expected ValueRef(array), got {other:?}"),
        }
        match &data.parts[1] {
            OutputPart::ValueRef(v) => assert_eq!(*v, Value::map(map)),
            other => unreachable!("expected ValueRef(map), got {other:?}"),
        }
    }

    // Function values (T1c, #700) persist through the transcript/journal as
    // ordinary values (spec §6). This locks the VAL_FN_REF / VAL_CLOSURE
    // encoding — the fn token, the bound-env names/modes, and both payload
    // shapes (`val` snapshot, `ref` VariablePointer) — across the round-trip.
    #[test]
    fn round_trip_value_ref_function_values() {
        use brink_format::{ClosureEnvEntry, DefinitionId, DefinitionTag, NameId};

        let target = DefinitionId::new(DefinitionTag::Address, 7);
        let cell = DefinitionId::new(DefinitionTag::Address, 3);
        let fn_ref = Value::FnRef(target);
        let closure = Value::closure(
            target,
            vec![
                ClosureEnvEntry {
                    name: NameId(2),
                    is_ref: true,
                    payload: Value::VariablePointer(cell),
                },
                ClosureEnvEntry {
                    name: NameId(5),
                    is_ref: false,
                    payload: Value::Int(41),
                },
            ],
        );

        let parts = vec![
            OutputPart::ValueRef(fn_ref.clone()),
            OutputPart::ValueRef(closure.clone()),
        ];
        let bytes = write_transcript(&parts, 7, &[]);
        let data = read_transcript(&bytes).unwrap();

        assert_eq!(data.parts.len(), 2);
        match &data.parts[0] {
            OutputPart::ValueRef(v) => assert_eq!(*v, fn_ref),
            other => unreachable!("expected ValueRef(fn_ref), got {other:?}"),
        }
        match &data.parts[1] {
            OutputPart::ValueRef(v) => assert_eq!(*v, closure),
            other => unreachable!("expected ValueRef(closure), got {other:?}"),
        }
    }

    #[test]
    fn round_trip_line_ref_with_slots() {
        let parts = vec![OutputPart::LineRef {
            container_idx: 42,
            line_idx: 7,
            slots: vec![Value::Int(123), Value::String(Arc::from("hello"))],
            flags: LineFlags::STARTS_WITH_WS | LineFlags::ENDS_WITH_WS,
        }];
        let bytes = write_transcript(&parts, 1234, &[]);
        let data = read_transcript(&bytes).unwrap();
        assert_eq!(data.parts.len(), 1);
        match &data.parts[0] {
            OutputPart::LineRef {
                container_idx,
                line_idx,
                slots,
                flags,
            } => {
                assert_eq!(*container_idx, 42);
                assert_eq!(*line_idx, 7);
                assert_eq!(slots.len(), 2);
                assert!(matches!(&slots[0], Value::Int(123)));
                assert!(flags.contains(LineFlags::STARTS_WITH_WS));
                assert!(flags.contains(LineFlags::ENDS_WITH_WS));
            }
            other => unreachable!("expected LineRef, got {other:?}"),
        }
    }

    #[test]
    fn checkpoint_filtered_on_write() {
        let parts = vec![
            OutputPart::Text("hello".to_string()),
            OutputPart::Checkpoint,
            OutputPart::Newline,
        ];
        let bytes = write_transcript(&parts, 0, &[]);
        let data = read_transcript(&bytes).unwrap();
        assert_eq!(data.parts.len(), 2); // Checkpoint filtered
        assert!(matches!(&data.parts[0], OutputPart::Text(_)));
        assert!(matches!(&data.parts[1], OutputPart::Newline));
    }

    #[test]
    fn invalid_magic_errors() {
        let mut bytes = write_transcript(&[], 0, &[]);
        bytes[0] = b'X';
        assert!(matches!(
            read_transcript(&bytes),
            Err(TranscriptError::InvalidMagic)
        ));
    }

    #[test]
    fn integrity_check_errors() {
        let mut bytes = write_transcript(&[OutputPart::Newline], 0, &[]);
        // Corrupt a body byte
        if let Some(last) = bytes.last_mut() {
            *last ^= 0xFF;
        }
        assert!(matches!(
            read_transcript(&bytes),
            Err(TranscriptError::IntegrityCheckFailed)
        ));
    }

    // ── Recursion-depth cap on VAL_ARRAY/VAL_MAP decode (#553, #561, #562) ──
    //
    // `decode_value` recurses into itself for VAL_ARRAY/VAL_MAP children with
    // no depth limit. A crafted transcript of nested single-element arrays
    // (~5 bytes/level) can stack-overflow the reader. These tests hand-build
    // a `Value` nested exactly at, and one past,
    // `brink_format::MAX_DECODE_DEPTH` (the single canonical definition
    // shared by every `decode_value` implementation, #561) and prove the
    // reader accepts the former and rejects the latter with a proper decode
    // error instead of overflowing the stack. Both the `VAL_ARRAY` recursion
    // branch and the parallel `VAL_MAP` branch are exercised at the boundary
    // (#562).

    /// A `Value` wrapped in `depth` single-element arrays around a scalar
    /// leaf, matching the issue's "nested single-element arrays" shape.
    fn nested_array(depth: usize) -> Value {
        let mut v = Value::Int(42);
        for _ in 0..depth {
            v = Value::array(vec![v]);
        }
        v
    }

    /// A `Value` wrapped in `depth` single-entry maps around a scalar leaf —
    /// the `VAL_MAP` analogue of [`nested_array`], exercising the parallel
    /// map recursion branch in `decode_value` (#562).
    fn nested_map(depth: usize) -> Value {
        use brink_format::{MapKey, OrderedMap};

        let mut v = Value::Int(42);
        for _ in 0..depth {
            let mut map = OrderedMap::with_capacity(1);
            map.insert(MapKey::Int(0), v);
            v = Value::map(map);
        }
        v
    }

    #[test]
    fn decode_value_accepts_max_depth_nesting() {
        // Exactly MAX_DECODE_DEPTH levels of nesting must still decode
        // cleanly — the cap must not clip legitimate (if unusual) data.
        let value = nested_array(MAX_DECODE_DEPTH);
        let parts = vec![OutputPart::ValueRef(value.clone())];
        let bytes = write_transcript(&parts, 0, &[]);

        let data = read_transcript(&bytes).expect("depth exactly at cap must decode");
        match &data.parts[0] {
            OutputPart::ValueRef(v) => assert_eq!(*v, value),
            other => unreachable!("expected ValueRef, got {other:?}"),
        }
    }

    #[test]
    fn decode_value_rejects_beyond_max_depth() {
        // One level past the cap must be rejected with a proper decode
        // error, not a stack overflow.
        let value = nested_array(MAX_DECODE_DEPTH + 1);
        let parts = vec![OutputPart::ValueRef(value)];
        let bytes = write_transcript(&parts, 0, &[]);

        assert!(matches!(
            read_transcript(&bytes),
            Err(TranscriptError::MaxDepthExceeded(MAX_DECODE_DEPTH))
        ));
    }

    #[test]
    fn decode_value_rejects_deeply_crafted_nesting() {
        // The actual attack scenario the issue describes: a much deeper
        // chain than any legitimate story would produce (well beyond the
        // cap, but shallow enough that constructing/encoding the fixture
        // itself — which has no depth cap by design; only the
        // untrusted-input decode path is guarded — doesn't hit unrelated
        // recursion limits). The reader must reject it promptly rather than
        // recursing hundreds of frames deep.
        let value = nested_array(8 * MAX_DECODE_DEPTH);
        let parts = vec![OutputPart::ValueRef(value)];
        let bytes = write_transcript(&parts, 0, &[]);

        assert!(matches!(
            read_transcript(&bytes),
            Err(TranscriptError::MaxDepthExceeded(MAX_DECODE_DEPTH))
        ));
    }

    // ── #562: parallel VAL_MAP recursion branch at the boundary ────────────

    #[test]
    fn decode_value_accepts_max_depth_map_nesting() {
        // Exactly MAX_DECODE_DEPTH levels of map nesting must still decode
        // cleanly — the cap must not clip legitimate (if unusual) data.
        let value = nested_map(MAX_DECODE_DEPTH);
        let parts = vec![OutputPart::ValueRef(value.clone())];
        let bytes = write_transcript(&parts, 0, &[]);

        let data = read_transcript(&bytes).expect("map depth exactly at cap must decode");
        match &data.parts[0] {
            OutputPart::ValueRef(v) => assert_eq!(*v, value),
            other => unreachable!("expected ValueRef, got {other:?}"),
        }
    }

    #[test]
    fn decode_value_rejects_beyond_max_depth_map_nesting() {
        // One level past the cap must be rejected with a proper decode
        // error, not a stack overflow.
        let value = nested_map(MAX_DECODE_DEPTH + 1);
        let parts = vec![OutputPart::ValueRef(value)];
        let bytes = write_transcript(&parts, 0, &[]);

        assert!(matches!(
            read_transcript(&bytes),
            Err(TranscriptError::MaxDepthExceeded(MAX_DECODE_DEPTH))
        ));
    }
}
