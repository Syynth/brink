/// Errors that can occur during internationalization export.
#[derive(Debug)]
pub enum IntlError {
    /// JSON serialization failed.
    Serialize(serde_json::Error),
}

impl core::fmt::Display for IntlError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Serialize(e) => write!(f, "failed to serialize: {e}"),
        }
    }
}

impl std::error::Error for IntlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serialize(e) => Some(e),
        }
    }
}

impl From<serde_json::Error> for IntlError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialize(e)
    }
}
