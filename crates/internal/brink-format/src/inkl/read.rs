//! Decoding (read) half of the `.inkl` locale overlay format.

use crate::codec::{read_def_id, read_str, read_u8, read_u16, read_u32};
use crate::definition::{LocaleData, LocaleLineEntry, LocaleScopeTable};
use crate::inkb::read::decode_line_content;
use crate::opcode::DecodeError;

use super::{INKL_MAGIC, INKL_VERSION};

/// Decode a [`LocaleData`] from `.inkl` binary format.
pub fn read_inkl(buf: &[u8]) -> Result<LocaleData, DecodeError> {
    if buf.len() < 4 {
        return Err(DecodeError::UnexpectedEof);
    }
    let magic: [u8; 4] = [buf[0], buf[1], buf[2], buf[3]];
    if &magic != INKL_MAGIC {
        return Err(DecodeError::BadInklMagic(magic));
    }

    let mut off = 4;
    let version = read_u8(buf, &mut off)?;
    if version != INKL_VERSION {
        return Err(DecodeError::UnsupportedInklVersion(version));
    }

    let base_checksum = read_u32(buf, &mut off)?;
    let locale_len = read_u16(buf, &mut off)? as usize;
    if off + locale_len > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let locale_tag = String::from_utf8(buf[off..off + locale_len].to_vec())
        .map_err(|_| DecodeError::InvalidUtf8)?;
    off += locale_len;

    let scope_count = read_u32(buf, &mut off)? as usize;
    let mut line_tables = Vec::with_capacity(scope_count);
    for _ in 0..scope_count {
        let scope_id = read_def_id(buf, &mut off)?;
        let line_count = read_u32(buf, &mut off)? as usize;
        let mut lines = Vec::with_capacity(line_count);
        for _ in 0..line_count {
            let content = decode_line_content(buf, &mut off)?;
            let has_audio = read_u8(buf, &mut off)? != 0;
            let audio_ref = if has_audio {
                Some(read_str(buf, &mut off)?)
            } else {
                None
            };
            lines.push(LocaleLineEntry { content, audio_ref });
        }
        line_tables.push(LocaleScopeTable { scope_id, lines });
    }

    Ok(LocaleData {
        locale_tag,
        base_checksum,
        line_tables,
    })
}
