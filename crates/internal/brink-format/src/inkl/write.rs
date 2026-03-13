//! Encoding (write) half of the `.inkl` locale overlay format.

use crate::codec::{write_def_id, write_str, write_u8, write_u16, write_u32};
use crate::definition::LocaleData;
use crate::inkb::write::encode_line_content;

use super::{INKL_MAGIC, INKL_VERSION};

/// Encode a [`LocaleData`] into the `.inkl` binary format.
#[expect(clippy::cast_possible_truncation)]
pub fn write_inkl(data: &LocaleData, buf: &mut Vec<u8>) {
    // Header
    buf.extend_from_slice(INKL_MAGIC);
    write_u8(buf, INKL_VERSION);
    write_u32(buf, data.base_checksum);
    write_u16(buf, data.locale_tag.len() as u16);
    buf.extend_from_slice(data.locale_tag.as_bytes());

    // Line tables payload
    write_u32(buf, data.line_tables.len() as u32);
    for scope in &data.line_tables {
        write_def_id(buf, scope.scope_id);
        write_u32(buf, scope.lines.len() as u32);
        for entry in &scope.lines {
            encode_line_content(&entry.content, buf);
            match &entry.audio_ref {
                Some(audio) => {
                    write_u8(buf, 1);
                    write_str(buf, audio);
                }
                None => {
                    write_u8(buf, 0);
                }
            }
        }
    }
}
