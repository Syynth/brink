use brink_format::DecodeError;

/// Errors that can occur during internationalization operations.
#[derive(Debug)]
pub enum IntlError {
    /// JSON serialization failed.
    Serialize(serde_json::Error),
    /// The scope ID string could not be parsed.
    InvalidScopeId(String),
    /// A scope in the translated lines was not found in the base .inkb.
    ScopeNotInBase(String),
    /// The line count for a scope doesn't match the base.
    LineCountMismatch {
        scope_id: String,
        expected: usize,
        actual: usize,
    },
    /// The locale tag is empty or invalid.
    InvalidLocaleTag(String),
    /// Failed to decode the base .inkb file.
    BaseFormat(DecodeError),
    /// A select key string could not be parsed.
    InvalidSelectKey(String),
}

impl core::fmt::Display for IntlError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Serialize(e) => write!(f, "failed to serialize: {e}"),
            Self::InvalidScopeId(id) => write!(f, "invalid scope id: {id}"),
            Self::ScopeNotInBase(id) => write!(f, "scope not found in base: {id}"),
            Self::LineCountMismatch {
                scope_id,
                expected,
                actual,
            } => write!(
                f,
                "line count mismatch for scope {scope_id}: expected {expected}, got {actual}"
            ),
            Self::InvalidLocaleTag(tag) => write!(f, "invalid locale tag: {tag}"),
            Self::BaseFormat(e) => write!(f, "base format error: {e}"),
            Self::InvalidSelectKey(key) => write!(f, "invalid select key: {key}"),
        }
    }
}

impl std::error::Error for IntlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serialize(e) => Some(e),
            Self::BaseFormat(e) => Some(e),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for IntlError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialize(e)
    }
}

impl From<DecodeError> for IntlError {
    fn from(e: DecodeError) -> Self {
        Self::BaseFormat(e)
    }
}
