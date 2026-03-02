use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConvertError {
    #[error("unresolved path: {0}")]
    UnresolvedPath(String),

    #[error("unknown variable: {0}")]
    UnknownVariable(String),

    #[error("integer overflow converting value")]
    IntegerOverflow,

    #[error("name table overflow (>65535 entries)")]
    NameTableOverflow,

    #[error("line table overflow (>65535 entries)")]
    LineTableOverflow,

    #[error("temp variable overflow (>65535 slots)")]
    TempOverflow,
}
