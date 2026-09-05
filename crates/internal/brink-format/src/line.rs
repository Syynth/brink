use alloc::string::String;
use alloc::vec::Vec;

/// The content of a single output line — either a plain string or a template
/// with interpolation slots and plural selects.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LineContent {
    Plain(String),
    Template(LineTemplate),
}

bitflags::bitflags! {
    /// Whitespace characteristics of a line, precomputed at compile time.
    ///
    /// Used by the output buffer to make filtering decisions (suppress
    /// whitespace-only/empty content when there's no content yet) without
    /// eagerly resolving deferred `LineRef` parts.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct LineFlags: u8 {
        /// The resolved content is entirely whitespace (but not empty).
        const ALL_WS = 0b0100;
        /// The resolved content is empty.
        const EMPTY  = 0b1000;
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

        // ALL_WS: only true if every part is whitespace-only literals.
        // Any Slot/Select means we can't guarantee all-whitespace. A Span
        // is conservative too, for the same reason Slot/Select are — its
        // `children` could resolve to anything once a `Slot` inside it
        // does — even though a *literal-only* span's whitespace-ness
        // could in principle be computed recursively, that refinement
        // isn't needed for the runtime's current use of this flag
        // (suppressing empty/whitespace-only output before real content
        // has started) and conservative-false is always a safe answer.
        let all_ws = parts.iter().all(|p| match p {
            LinePart::Literal(s) => s.trim().is_empty(),
            LinePart::Slot(_) | LinePart::Select { .. } | LinePart::Span { .. } => false,
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    /// `<name attr="v">…</name>` — an inline markup span
    /// (`docs/prose-dialect-spec.md` §4.4, issue #1716). Genuinely nested,
    /// mirroring `hir::ContentPart::Span`: the decoder enforces balance
    /// structurally, so a mangled translation (unbalanced inline codes, a
    /// classic TMS failure) becomes a decode error, not silent rendering
    /// corruption. `children` is empty for a self-closing / point-marker
    /// span (`<pause/>`, `<sfx name="bell"/>`, §8b.11).
    ///
    /// **Hash-transparent** (§4.4, RULED before any markup ships): `name`/
    /// `attrs` never contribute to `source_hash` — only `children`'s own
    /// text/slots do, recursively, the same way an `Interpolation`
    /// contributes a `"{…}"` placeholder rather than its resolved value.
    /// `Hello <wave>world</wave>` hashes identically to `Hello world`. See
    /// `brink-ir`'s `lir::lower::recognize` — the one place that builds
    /// this variant, and the one place hash-transparency is enforced.
    Span {
        name: String,
        attrs: Vec<(String, String)>,
        children: Vec<LinePart>,
    },
}

/// The key for matching a branch in a [`LinePart::Select`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    fn bit_values_are_stable_for_persisted_brkt_compatibility() {
        // `LineFlags` is persisted on the wire in the `.brkt` transcript
        // format (`transcript.rs`'s `encode_part`/`decode_part`), unlike
        // `.inkb` where it's recomputed at decode time. Removing
        // `STARTS_WITH_WS`/`ENDS_WITH_WS` must not renumber the surviving
        // bits, or a `.brkt` file written before this change would decode
        // its old `ALL_WS`/`EMPTY` bits (0b0100/0b1000) as different flags
        // under a newer reader. Pin the values so a future edit here has to
        // consciously break this guarantee.
        assert_eq!(LineFlags::ALL_WS.bits(), 0b0100);
        assert_eq!(LineFlags::EMPTY.bits(), 0b1000);
    }

    #[test]
    fn empty_plain_string_is_empty() {
        assert_eq!(LineFlags::from_plain(""), LineFlags::EMPTY);
    }

    #[test]
    fn whitespace_only_plain_string_is_all_ws() {
        let flags = LineFlags::from_plain("   \t\n  ");
        assert!(flags.contains(LineFlags::ALL_WS));
        assert!(!flags.contains(LineFlags::EMPTY));
    }

    #[test]
    fn mixed_content_plain_string_has_no_flags() {
        // Leading/trailing whitespace on otherwise non-whitespace content
        // used to set STARTS_WITH_WS/ENDS_WITH_WS; those flags were removed
        // (no production consumer — see #1444's follow-up scope note) so
        // this case now carries no flags at all.
        let flags = LineFlags::from_plain("  Hello world  ");
        assert!(flags.is_empty());
    }

    #[test]
    fn empty_template_is_empty() {
        assert_eq!(LineFlags::from_template(&[]), LineFlags::EMPTY);
    }

    #[test]
    fn all_whitespace_literal_parts_are_all_ws() {
        let parts = alloc::vec![
            LinePart::Literal("  ".to_string()),
            LinePart::Literal("\t".to_string()),
        ];
        let flags = LineFlags::from_template(&parts);
        assert!(flags.contains(LineFlags::ALL_WS));
    }

    #[test]
    fn a_slot_defeats_all_ws_even_if_every_literal_is_whitespace() {
        // A Slot's resolved content is unknown at compile time, so ALL_WS
        // must stay conservative (unset) even when every literal part
        // present is whitespace-only.
        let parts = alloc::vec![LinePart::Literal("  ".to_string()), LinePart::Slot(0)];
        let flags = LineFlags::from_template(&parts);
        assert!(!flags.contains(LineFlags::ALL_WS));
    }

    #[test]
    fn a_select_defeats_all_ws() {
        let parts = alloc::vec![select_part(), LinePart::Literal("  ".to_string())];
        let flags = LineFlags::from_template(&parts);
        assert!(!flags.contains(LineFlags::ALL_WS));
    }

    #[test]
    fn mixed_content_template_has_no_flags() {
        let parts = alloc::vec![
            LinePart::Literal("Hello ".to_string()),
            LinePart::Slot(0),
            LinePart::Literal(" world".to_string()),
        ];
        let flags = LineFlags::from_template(&parts);
        assert!(flags.is_empty());
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

    #[test]
    fn from_content_matches_from_plain_for_plain() {
        let content = LineContent::Plain("   ".to_string());
        assert_eq!(
            LineFlags::from_content(&content),
            LineFlags::from_plain("   ")
        );
    }
}
