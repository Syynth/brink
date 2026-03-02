/// The content of a single output line — either a plain string or a template
/// with interpolation slots and plural selects.
#[derive(Debug, Clone, PartialEq)]
pub enum LineContent {
    Plain(String),
    Template(LineTemplate),
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
