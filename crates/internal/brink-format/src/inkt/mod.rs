//! Textual (.inkt) format: writer and reader.
//!
//! Gated behind the `inkt` cargo feature since the reader depends on `pest`.

mod read;
pub(crate) mod write;

pub use read::{InktParseError, read_inkt};
pub use write::write_inkt;
