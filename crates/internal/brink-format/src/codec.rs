//! Shared low-level byte encoding/decoding helpers.
//!
//! Used by both the opcode codec and the `.inkb` binary format.

use crate::id::DefinitionId;
use crate::opcode::DecodeError;

// ── Encoding helpers ────────────────────────────────────────────────────────

pub(crate) fn write_u8(buf: &mut Vec<u8>, v: u8) {
    buf.push(v);
}

pub(crate) fn write_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn write_i32(buf: &mut Vec<u8>, v: i32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn write_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn write_f32(buf: &mut Vec<u8>, v: f32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn write_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn write_def_id(buf: &mut Vec<u8>, id: DefinitionId) {
    write_u64(buf, id.to_raw());
}

#[expect(clippy::cast_possible_truncation)]
pub(crate) fn write_str(buf: &mut Vec<u8>, s: &str) {
    write_u32(buf, s.len() as u32);
    buf.extend_from_slice(s.as_bytes());
}

// ── Decoding helpers ────────────────────────────────────────────────────────

pub(crate) fn read_u8(buf: &[u8], offset: &mut usize) -> Result<u8, DecodeError> {
    if *offset >= buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let v = buf[*offset];
    *offset += 1;
    Ok(v)
}

pub(crate) fn read_u16(buf: &[u8], offset: &mut usize) -> Result<u16, DecodeError> {
    if *offset + 2 > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let v = u16::from_le_bytes([buf[*offset], buf[*offset + 1]]);
    *offset += 2;
    Ok(v)
}

pub(crate) fn read_i32(buf: &[u8], offset: &mut usize) -> Result<i32, DecodeError> {
    if *offset + 4 > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let v = i32::from_le_bytes([
        buf[*offset],
        buf[*offset + 1],
        buf[*offset + 2],
        buf[*offset + 3],
    ]);
    *offset += 4;
    Ok(v)
}

pub(crate) fn read_u32(buf: &[u8], offset: &mut usize) -> Result<u32, DecodeError> {
    if *offset + 4 > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let v = u32::from_le_bytes([
        buf[*offset],
        buf[*offset + 1],
        buf[*offset + 2],
        buf[*offset + 3],
    ]);
    *offset += 4;
    Ok(v)
}

pub(crate) fn read_f32(buf: &[u8], offset: &mut usize) -> Result<f32, DecodeError> {
    if *offset + 4 > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let v = f32::from_le_bytes([
        buf[*offset],
        buf[*offset + 1],
        buf[*offset + 2],
        buf[*offset + 3],
    ]);
    *offset += 4;
    Ok(v)
}

pub(crate) fn read_u64(buf: &[u8], offset: &mut usize) -> Result<u64, DecodeError> {
    if *offset + 8 > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let v = u64::from_le_bytes([
        buf[*offset],
        buf[*offset + 1],
        buf[*offset + 2],
        buf[*offset + 3],
        buf[*offset + 4],
        buf[*offset + 5],
        buf[*offset + 6],
        buf[*offset + 7],
    ]);
    *offset += 8;
    Ok(v)
}

pub(crate) fn read_def_id(buf: &[u8], offset: &mut usize) -> Result<DefinitionId, DecodeError> {
    let raw = read_u64(buf, offset)?;
    DefinitionId::from_raw(raw).ok_or(DecodeError::InvalidDefinitionId(raw))
}

pub(crate) fn read_str(buf: &[u8], offset: &mut usize) -> Result<String, DecodeError> {
    let len = read_u32(buf, offset)? as usize;
    if *offset + len > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let bytes = &buf[*offset..*offset + len];
    *offset += len;
    String::from_utf8(bytes.to_vec()).map_err(|_| DecodeError::InvalidUtf8)
}
