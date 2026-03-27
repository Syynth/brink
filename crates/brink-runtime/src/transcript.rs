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

use std::rc::Rc;

use brink_format::{DefinitionId, LineFlags, Value};

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
const VAL_FRAGMENT_REF: u8 = 0x08;

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

/// Deserialize a transcript from the `.brkt` binary format.
///
/// Returns `(parts, source_checksum)`. The caller should validate the
/// source checksum against the program's checksum before using the parts.
/// Result of reading a transcript: (parts, `source_checksum`, fragments).
pub type TranscriptData = (Vec<OutputPart>, u32, Vec<crate::output::Fragment>);

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
                    slots.push(decode_value(bytes, &mut off)?);
                }
                OutputPart::LineRef {
                    container_idx,
                    line_idx,
                    slots,
                    flags,
                }
            }
            TAG_VALUE_REF => OutputPart::ValueRef(decode_value(bytes, &mut off)?),
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
                        slots.push(decode_value(bytes, &mut off)?);
                    }
                    OutputPart::LineRef {
                        container_idx,
                        line_idx,
                        slots,
                        flags,
                    }
                }
                TAG_VALUE_REF => OutputPart::ValueRef(decode_value(bytes, &mut off)?),
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

    Ok((parts, source_checksum, fragments))
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
            write_u8(buf, VAL_DIVERT_TARGET); // serialize same as divert target
            write_def_id(buf, *id);
        }
        Value::FragmentRef(idx) => {
            write_u8(buf, VAL_FRAGMENT_REF);
            write_u32(buf, *idx);
        }
        Value::TempPointer { .. } | Value::Null => {
            write_u8(buf, VAL_NULL);
        }
    }
}

fn decode_value(buf: &[u8], off: &mut usize) -> Result<Value, TranscriptError> {
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
            Ok(Value::String(Rc::from(s.as_str())))
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
            Ok(Value::List(Rc::new(brink_format::ListValue {
                items,
                origins,
            })))
        }
        VAL_DIVERT_TARGET => {
            let id = read_def_id(buf, off)?;
            Ok(Value::DivertTarget(id))
        }
        VAL_FRAGMENT_REF => Ok(Value::FragmentRef(read_u32(buf, off)?)),
        VAL_NULL => Ok(Value::Null),
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

    fn unwrap_transcript(bytes: &[u8]) -> (Vec<OutputPart>, u32, Vec<crate::output::Fragment>) {
        read_transcript(bytes).unwrap()
    }

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
        let (decoded, checksum, _fragments) = unwrap_transcript(&bytes);
        assert_eq!(checksum, 0xDEAD_BEEF);
        assert_eq!(decoded.len(), 5);
        assert!(matches!(&decoded[0], OutputPart::Text(s) if s == "Hello"));
        assert!(matches!(&decoded[1], OutputPart::Spring));
        assert!(matches!(&decoded[2], OutputPart::Newline));
        assert!(matches!(&decoded[3], OutputPart::Tag(s) if s == "tag1"));
        assert!(matches!(&decoded[4], OutputPart::Glue));
    }

    #[test]
    fn round_trip_line_ref_with_slots() {
        let parts = vec![OutputPart::LineRef {
            container_idx: 42,
            line_idx: 7,
            slots: vec![Value::Int(123), Value::String(Rc::from("hello"))],
            flags: LineFlags::STARTS_WITH_WS | LineFlags::ENDS_WITH_WS,
        }];
        let bytes = write_transcript(&parts, 1234, &[]);
        let (decoded, _, _fragments) = unwrap_transcript(&bytes);
        assert_eq!(decoded.len(), 1);
        match &decoded[0] {
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
        let (decoded, _, _fragments) = unwrap_transcript(&bytes);
        assert_eq!(decoded.len(), 2); // Checkpoint filtered
        assert!(matches!(&decoded[0], OutputPart::Text(_)));
        assert!(matches!(&decoded[1], OutputPart::Newline));
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
}
