use alloc::string::String;
use alloc::vec::Vec;

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

    fn from_template(parts: &[LinePart]) -> Self {
        if parts.is_empty() {
            return Self::EMPTY;
        }
        let mut flags = Self::empty();

        // Leading/trailing empty-string literals contribute no characters, so
        // they must not be mistaken for the part that determines leading/
        // trailing whitespace — walk past them to the first part that could
        // actually carry a character.
        //
        // Only empty-string `Literal`s are skipped here; every other part
        // kind (a non-empty `Literal`, `Slot`, `Select`) stops the walk. In
        // particular `Slot`/`Select` parts that remain after skipping are
        // left conservative on purpose: their resolved content isn't known
        // at compile time, so we can't claim they do or don't start/end with
        // whitespace.
        if let Some(LinePart::Literal(s)) = parts.iter().find(|p| !p.contributes_no_text())
            && s.starts_with(char::is_whitespace)
        {
            flags |= Self::STARTS_WITH_WS;
        }

        if let Some(LinePart::Literal(s)) = parts.iter().rev().find(|p| !p.contributes_no_text())
            && s.ends_with(char::is_whitespace)
        {
            flags |= Self::ENDS_WITH_WS;
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

impl LinePart {
    /// Whether this part is guaranteed to contribute no characters to the
    /// resolved content, and so can never carry leading/trailing whitespace
    /// itself.
    ///
    /// Only an empty-string `Literal` qualifies. This is an exhaustive match
    /// with no `_` arm on purpose: adding a new `LinePart` variant (e.g. a
    /// nested `Span`) forces a decision here about whether it can be
    /// zero-width, rather than silently defaulting to "carries content" or
    /// "contributes no text".
    fn contributes_no_text(&self) -> bool {
        match self {
            Self::Literal(s) => s.is_empty(),
            Self::Slot(_) | Self::Select { .. } => false,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn select_part() -> LinePart {
        LinePart::Select {
            slot: 0,
            variants: alloc::vec![(SelectKey::Exact(1), "one".to_string())],
            default: "many".to_string(),
        }
    }

    #[test]
    fn leading_slot_is_conservative_but_trailing_literal_still_checked() {
        // `LinePart::Slot` at the front is content we can't inspect at
        // compile time, so STARTS_WITH_WS must stay unset — but the
        // trailing literal is still directly inspectable.
        let parts = alloc::vec![
            LinePart::Slot(0),
            LinePart::Literal("trailing ".to_string())
        ];
        let flags = LineFlags::from_template(&parts);
        assert!(!flags.contains(LineFlags::STARTS_WITH_WS));
        assert!(flags.contains(LineFlags::ENDS_WITH_WS));
    }

    #[test]
    fn trailing_slot_is_conservative_but_leading_literal_still_checked() {
        let parts = alloc::vec![LinePart::Literal(" leading".to_string()), LinePart::Slot(0)];
        let flags = LineFlags::from_template(&parts);
        assert!(flags.contains(LineFlags::STARTS_WITH_WS));
        assert!(!flags.contains(LineFlags::ENDS_WITH_WS));
    }

    #[test]
    fn leading_select_is_conservative() {
        let parts = alloc::vec![select_part(), LinePart::Literal("trailing ".to_string())];
        let flags = LineFlags::from_template(&parts);
        assert!(!flags.contains(LineFlags::STARTS_WITH_WS));
        assert!(flags.contains(LineFlags::ENDS_WITH_WS));
    }

    #[test]
    fn trailing_select_is_conservative() {
        let parts = alloc::vec![LinePart::Literal(" leading".to_string()), select_part()];
        let flags = LineFlags::from_template(&parts);
        assert!(flags.contains(LineFlags::STARTS_WITH_WS));
        assert!(!flags.contains(LineFlags::ENDS_WITH_WS));
    }

    #[test]
    fn empty_leading_literal_does_not_defeat_starts_with_ws() {
        // Regression for #1444: an empty leading literal contributes no
        // characters and must not be mistaken for "the first part has no
        // leading whitespace" — the check must walk past it to the literal
        // that actually carries content.
        let parts = alloc::vec![
            LinePart::Literal(String::new()),
            LinePart::Literal(" indented".to_string()),
        ];
        let flags = LineFlags::from_template(&parts);
        assert!(flags.contains(LineFlags::STARTS_WITH_WS));
    }

    #[test]
    fn empty_trailing_literal_does_not_defeat_ends_with_ws() {
        let parts = alloc::vec![
            LinePart::Literal("trailing ".to_string()),
            LinePart::Literal(String::new()),
        ];
        let flags = LineFlags::from_template(&parts);
        assert!(flags.contains(LineFlags::ENDS_WITH_WS));
    }

    #[test]
    fn multiple_empty_leading_literals_are_all_skipped() {
        let parts = alloc::vec![
            LinePart::Literal(String::new()),
            LinePart::Literal(String::new()),
            LinePart::Literal(" indented".to_string()),
        ];
        let flags = LineFlags::from_template(&parts);
        assert!(flags.contains(LineFlags::STARTS_WITH_WS));
    }

    #[test]
    fn empty_leading_literal_then_slot_stays_conservative() {
        // The empty literal is skipped, landing on the Slot — still
        // unknowable, so STARTS_WITH_WS correctly stays unset.
        let parts = alloc::vec![LinePart::Literal(String::new()), LinePart::Slot(0)];
        let flags = LineFlags::from_template(&parts);
        assert!(!flags.contains(LineFlags::STARTS_WITH_WS));
    }

    #[test]
    fn no_leading_or_trailing_whitespace() {
        let parts = alloc::vec![
            LinePart::Literal("Hello ".to_string()),
            LinePart::Slot(0),
            LinePart::Literal(" world".to_string()),
        ];
        let flags = LineFlags::from_template(&parts);
        assert!(!flags.contains(LineFlags::STARTS_WITH_WS));
        assert!(!flags.contains(LineFlags::ENDS_WITH_WS));
    }

    #[test]
    fn from_content_matches_from_template_for_templates() {
        let parts = alloc::vec![LinePart::Slot(0), LinePart::Literal(" world".to_string())];
        let content = LineContent::Template(parts.clone());
        assert_eq!(
            LineFlags::from_content(&content),
            LineFlags::from_template(&parts)
        );
    }
}
