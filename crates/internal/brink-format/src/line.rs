/// The content of a single output line — either a plain string or a template
/// with interpolation slots and plural selects.
#[derive(Debug, Clone, PartialEq)]
pub enum LineContent {
    Plain(String),
    Template(LineTemplate),
}

bitflags::bitflags! {
    /// Whitespace characteristics of a line, precomputed at compile time.
    ///
    /// Used by the output buffer to make filtering decisions (suppress leading
    /// whitespace, collapse adjacent whitespace) without eagerly resolving
    /// deferred `LineRef` parts.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct LineFlags: u8 {
        /// The resolved content starts with whitespace.
        const STARTS_WITH_WS = 0b0001;
        /// The resolved content ends with whitespace.
        const ENDS_WITH_WS   = 0b0010;
        /// The resolved content is entirely whitespace (but not empty).
        const ALL_WS         = 0b0100;
        /// The resolved content is empty.
        const EMPTY          = 0b1000;
    }
}

impl LineFlags {
    /// Compute flags from a `LineContent`.
    ///
    /// For `Plain` content, flags are exact. For `Template` content, flags
    /// are conservative: `Slot`/`Select` parts are assumed to produce
    /// non-whitespace content.
    pub fn from_content(content: &LineContent) -> Self {
        match content {
            LineContent::Plain(s) => Self::from_plain(s),
            LineContent::Template(parts) => Self::from_template(parts),
        }
    }

    /// Compute flags from a plain string.
    pub fn from_plain(s: &str) -> Self {
        if s.is_empty() {
            return Self::EMPTY;
        }
        let mut flags = Self::empty();
        if s.starts_with(char::is_whitespace) {
            flags |= Self::STARTS_WITH_WS;
        }
        if s.ends_with(char::is_whitespace) {
            flags |= Self::ENDS_WITH_WS;
        }
        if s.trim().is_empty() {
            flags |= Self::ALL_WS;
        }
        flags
    }

    pub fn from_template(parts: &[LinePart]) -> Self {
        if parts.is_empty() {
            return Self::EMPTY;
        }
        let mut flags = Self::empty();

        // Check first part for leading whitespace.
        if let Some(LinePart::Literal(s)) = parts.first()
            && s.starts_with(char::is_whitespace)
        {
            flags |= Self::STARTS_WITH_WS;
        }

        // Check last part for trailing whitespace.
        match parts.last() {
            Some(LinePart::Literal(s)) if s.ends_with(char::is_whitespace) => {
                flags |= Self::ENDS_WITH_WS;
            }
            _ => {}
        }

        // ALL_WS: only true if every part is whitespace-only literals.
        // Any Slot/Select means we can't guarantee all-whitespace.
        let all_ws = parts.iter().all(|p| match p {
            LinePart::Literal(s) => s.trim().is_empty(),
            _ => false,
        });
        if all_ws {
            flags |= Self::ALL_WS;
        }

        flags
    }
}

/// A sequence of literal and dynamic parts that compose an output line.
pub type LineTemplate = Vec<LinePart>;

/// One segment of a [`LineTemplate`].
#[derive(Debug, Clone, PartialEq)]
pub enum LinePart {
    /// A literal string fragment.
    Literal(String),
    /// A value interpolation slot (index into the evaluation stack snapshot).
    Slot(u8),
    /// A plural/keyword select over a slot value.
    Select {
        slot: u8,
        variants: Vec<(SelectKey, String)>,
        default: String,
    },
}

/// The key for matching a branch in a [`LinePart::Select`].
#[derive(Debug, Clone, PartialEq)]
pub enum SelectKey {
    Cardinal(PluralCategory),
    Ordinal(PluralCategory),
    Exact(i32),
    Keyword(String),
}

/// CLDR plural category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluralCategory {
    Zero,
    One,
    Two,
    Few,
    Many,
    Other,
}

/// Trait for resolving plural categories at runtime.
///
/// Implementors provide locale-aware plural resolution. The `brink-intl` crate
/// ships a batteries-included implementation backed by ICU4X baked data.
pub trait PluralResolver {
    /// Resolve the cardinal plural category for the given integer.
    ///
    /// `locale_override` allows overriding the resolver's default locale.
    fn cardinal(&self, n: i64, locale_override: Option<&str>) -> PluralCategory;

    /// Resolve the ordinal plural category for the given integer.
    fn ordinal(&self, n: i64) -> PluralCategory;
}
