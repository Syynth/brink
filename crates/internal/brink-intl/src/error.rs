use brink_format::DecodeError;

/// Errors that can occur during internationalization operations.
#[derive(Debug, thiserror::Error)]
pub enum IntlError {
    #[error("failed to serialize: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("invalid scope id: {0}")]
    InvalidScopeId(String),
    #[error("scope not found in base: {0}")]
    ScopeNotInBase(String),
    #[error("line count mismatch for scope {scope_id}: expected {expected}, got {actual}")]
    LineCountMismatch {
        scope_id: String,
        expected: usize,
        actual: usize,
    },
    #[error("invalid locale tag: {0}")]
    InvalidLocaleTag(String),
    #[error("base format error: {0}")]
    BaseFormat(#[from] DecodeError),
    #[error("invalid select key: {0}")]
    InvalidSelectKey(String),
    #[error("untranslated line at index {line_index} in scope {scope_id}")]
    UntranslatedLine { scope_id: String, line_index: u16 },
    #[error("XLIFF error: {0}")]
    Xliff(#[from] xliff2::Xliff2Error),
    #[error("invalid XLIFF unit id `{0}`: expected format `scope_id:line_index`")]
    InvalidUnitId(String),
    #[error("missing brink:hash on unit `{0}`")]
    MissingHash(String),
    #[error("select data not found for dataRef `{0}`")]
    MissingSelectData(String),
    #[error("invalid select JSON in originalData: {0}")]
    InvalidSelectJson(String),
}
