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
//! 4       1      Version: u8 (= 1)
//! 5       4      Base checksum: u32 LE (must match .inkb source_hash)
//! 9       2      Locale tag length: u16 LE
//! 11      N      Locale tag: UTF-8 bytes (BCP 47)
//! ```

mod read;
mod write;

pub use read::read_inkl;
pub use write::write_inkl;

pub(crate) const INKL_MAGIC: &[u8; 4] = b"INKL";
pub(crate) const INKL_VERSION: u8 = 1;
