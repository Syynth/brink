//! Binary (.inkl) writer and reader for [`LocaleData`].
//!
//! The `.inkl` format is a locale overlay that replaces line table content
//! in a linked program at runtime.
//!
//! ## Header layout
//!
//! ```text
//! Offset  Size   Field
//! ------  -----  ------
//! 0       4      Magic: b"INKL"
//! 4       1      Version: u8 (= 2)
//! 5       4      Base checksum: u32 LE (must match .inkb source_hash)
//! 9       2      Locale tag length: u16 LE
//! 11      N      Locale tag: UTF-8 bytes (BCP 47)
//! ```

mod read;
mod write;

pub use read::read_inkl;
pub use write::write_inkl;

pub(crate) const INKL_MAGIC: &[u8; 4] = b"INKL";
/// `.inkl` shares `encode_line_content`/`decode_line_content` with `.inkb`
/// (`inkb::write::encode_line_content`), so a new `LinePart` tag is exactly
/// as unrecognizable to an old `.inkl` reader as it is to an old `.inkb`
/// reader — `docs/prose-dialect-spec.md` §4.4's "Format cost, acknowledged"
/// bullet rules this explicitly: "a new `LinePart` tag is a `.inkb` version
/// bump (v6) + `.inkl` bump". v2 accompanies `.inkb` v6's `PART_SPAN`
/// addition (#1716, `docs/format-spec.md` § Versioning).
pub(crate) const INKL_VERSION: u8 = 2;
