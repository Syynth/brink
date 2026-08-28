//! Shared low-level byte encoding/decoding helpers.
//!
//! Used by both the opcode codec and the `.inkb` binary format.

use alloc::string::String;
use alloc::vec::Vec;

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

/// Unsigned LEB128 varint — scoped to the `DebugInfo` section (`0x11`,
/// `docs/debugger-spec.md` §2.2) only. Every other `.inkb` section keeps the
/// format's established fixed-width house style (`write_u8`/`u16`/`u32`/
/// `u64` above); this is a deliberate, ruled departure for one section
/// whose row count scales with statement (later, expression) count, not a
/// format-wide encoding change.
pub(crate) fn write_varint(buf: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            buf.push(byte);
            break;
        }
        buf.push(byte | 0x80);
    }
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

/// Unsigned LEB128 varint reader, paired with [`write_varint`]. Bounded at
/// 10 bytes (`ceil(64/7)`) — a crafted `.inkb` cannot force an unbounded
/// read loop off a truncated/malformed varint (`CLAUDE.md` "Guard against
/// unbounded growth"); a longer continuation run is `DecodeError::
/// UnexpectedEof`, the same failure a truncated fixed-width field gives.
pub(crate) fn read_varint(buf: &[u8], offset: &mut usize) -> Result<u64, DecodeError> {
    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    for _ in 0..10 {
        let byte = read_u8(buf, offset)?;
        if shift < 64 {
            result |= u64::from(byte & 0x7F) << shift;
        }
        if byte & 0x80 == 0 {
            return Ok(result);
        }
        shift += 7;
    }
    Err(DecodeError::UnexpectedEof)
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

// ── CRC-32 ─────────────────────────────────────────────────────────────────

/// CRC-32 (ISO 3309 / ITU-T V.42) with the standard `0x04C1_1DB7` polynomial.
pub(crate) fn crc32(data: &[u8]) -> u32 {
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
    use super::{read_varint, write_varint};
    use alloc::vec::Vec;

    /// D6 (`docs/debugger-spec.md` §2.2): the `DebugInfo` section's varint
    /// codec round-trips across the interesting boundaries — the single-byte
    /// range, the first two-byte value (128, where the continuation bit
    /// first turns on), and both ends of the `u32`/`u64` domains the section
    /// actually stores values in.
    #[test]
    fn varint_round_trips_boundary_values() {
        for v in [
            0u64,
            1,
            0x7F,   // last single-byte value
            0x80,   // first two-byte value
            0x3FFF, // last two-byte value
            0x4000, // first three-byte value
            u32::MAX as u64,
            u64::MAX,
        ] {
            let mut buf = Vec::new();
            write_varint(&mut buf, v);
            let mut off = 0;
            let read = read_varint(&buf, &mut off).unwrap();
            assert_eq!(read, v, "round-trip mismatch for {v:#x}");
            assert_eq!(
                off,
                buf.len(),
                "read_varint must consume exactly what write_varint wrote"
            );
        }
    }

    /// Single-byte values encode in exactly one byte — this is the whole
    /// point of choosing varint for the entry table (`docs/debugger-spec.md`
    /// §2.2: "most spans are a few dozen bytes... exactly the varint-friendly
    /// case").
    #[test]
    fn varint_small_values_are_one_byte() {
        for v in [0u64, 1, 42, 0x7F] {
            let mut buf = Vec::new();
            write_varint(&mut buf, v);
            assert_eq!(buf.len(), 1, "value {v} should encode in one byte");
        }
    }

    /// A truncated continuation run (every byte has the high bit set, buffer
    /// runs out) is `UnexpectedEof`, not an infinite loop or a panic — the
    /// `CLAUDE.md` "Guard against unbounded growth" contract for a reader
    /// facing a crafted/corrupt `.inkb`.
    #[test]
    fn read_varint_rejects_truncated_continuation_run() {
        let buf = [0x80u8; 3]; // every byte says "more follows"; buffer ends
        let mut off = 0;
        let err = read_varint(&buf, &mut off).unwrap_err();
        assert_eq!(err, crate::opcode::DecodeError::UnexpectedEof);
    }

    /// A continuation run longer than 10 bytes (`ceil(64/7)`) is rejected
    /// rather than read forever — bounds the reader against a crafted input
    /// that never sets the terminating bit.
    #[test]
    fn read_varint_rejects_overlong_continuation_run() {
        let buf = [0x80u8; 11];
        let mut off = 0;
        let err = read_varint(&buf, &mut off).unwrap_err();
        assert_eq!(err, crate::opcode::DecodeError::UnexpectedEof);
    }
}
