//! Textual (.inkt) format: writer and reader.
//!
//! The writer (`write_inkt`) is dependency-free (`inkt-write` feature). The
//! reader (`read_inkt`) depends on `pest` and needs the full `inkt` feature.

#[cfg(feature = "inkt")]
mod read;
pub(crate) mod write;

#[cfg(feature = "inkt")]
pub use read::{InktParseError, read_inkt};
pub use write::write_inkt;
