//! ICU4X-backed plural resolution.

use brink_format::{PluralCategory, PluralResolver};
use icu_plurals::PluralRules;

use crate::IntlError;

/// Plural resolver backed by ICU4X baked CLDR data.
///
/// Ships all locale plural rules — the compiled data is ~50KB total.
pub struct IcuPluralResolver {
    locale: icu_locale_core::Locale,
}

impl IcuPluralResolver {
    /// Create a resolver for the given BCP-47 locale tag (e.g. `"en"`, `"ar"`, `"ja"`).
    pub fn new(locale_tag: &str) -> Result<Self, IntlError> {
        let locale: icu_locale_core::Locale = locale_tag
            .parse()
            .map_err(|_| IntlError::InvalidLocaleTag(locale_tag.to_owned()))?;
        Ok(Self { locale })
    }

    fn prefs_for(locale: &icu_locale_core::Locale) -> icu_plurals::PluralRulesPreferences {
        locale.clone().into()
    }
}

impl PluralResolver for IcuPluralResolver {
    fn cardinal(&self, n: i64, locale_override: Option<&str>) -> PluralCategory {
        let locale = if let Some(tag) = locale_override {
            match tag.parse::<icu_locale_core::Locale>() {
                Ok(l) => l,
                Err(_) => return PluralCategory::Other,
            }
        } else {
            self.locale.clone()
        };

        let prefs = Self::prefs_for(&locale);
        let Ok(rules) = PluralRules::try_new_cardinal(prefs) else {
            return PluralCategory::Other;
        };

        from_icu(rules.category_for(n))
    }

    fn ordinal(&self, n: i64) -> PluralCategory {
        let prefs = Self::prefs_for(&self.locale);
        let Ok(rules) = PluralRules::try_new_ordinal(prefs) else {
            return PluralCategory::Other;
        };

        from_icu(rules.category_for(n))
    }
}

/// No-op resolver that always returns [`PluralCategory::Other`].
///
/// Used as the fallback when no locale-aware resolver is configured.
pub struct DefaultPluralResolver;

impl PluralResolver for DefaultPluralResolver {
    fn cardinal(&self, _n: i64, _locale_override: Option<&str>) -> PluralCategory {
        PluralCategory::Other
    }

    fn ordinal(&self, _n: i64) -> PluralCategory {
        PluralCategory::Other
    }
}

fn from_icu(cat: icu_plurals::PluralCategory) -> PluralCategory {
    match cat {
        icu_plurals::PluralCategory::Zero => PluralCategory::Zero,
        icu_plurals::PluralCategory::One => PluralCategory::One,
        icu_plurals::PluralCategory::Two => PluralCategory::Two,
        icu_plurals::PluralCategory::Few => PluralCategory::Few,
        icu_plurals::PluralCategory::Many => PluralCategory::Many,
        icu_plurals::PluralCategory::Other => PluralCategory::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_cardinal() {
        let r = IcuPluralResolver::new("en").unwrap();
        assert_eq!(r.cardinal(1, None), PluralCategory::One);
        assert_eq!(r.cardinal(0, None), PluralCategory::Other);
        assert_eq!(r.cardinal(2, None), PluralCategory::Other);
        assert_eq!(r.cardinal(42, None), PluralCategory::Other);
    }

    #[test]
    fn arabic_cardinal() {
        let r = IcuPluralResolver::new("ar").unwrap();
        assert_eq!(r.cardinal(0, None), PluralCategory::Zero);
        assert_eq!(r.cardinal(1, None), PluralCategory::One);
        assert_eq!(r.cardinal(2, None), PluralCategory::Two);
        assert_eq!(r.cardinal(3, None), PluralCategory::Few);
        assert_eq!(r.cardinal(11, None), PluralCategory::Many);
        assert_eq!(r.cardinal(100, None), PluralCategory::Other);
    }

    #[test]
    fn japanese_cardinal_always_other() {
        let r = IcuPluralResolver::new("ja").unwrap();
        assert_eq!(r.cardinal(0, None), PluralCategory::Other);
        assert_eq!(r.cardinal(1, None), PluralCategory::Other);
        assert_eq!(r.cardinal(1000, None), PluralCategory::Other);
    }

    #[test]
    fn english_ordinal() {
        let r = IcuPluralResolver::new("en").unwrap();
        assert_eq!(r.ordinal(1), PluralCategory::One);
        assert_eq!(r.ordinal(2), PluralCategory::Two);
        assert_eq!(r.ordinal(3), PluralCategory::Few);
        assert_eq!(r.ordinal(4), PluralCategory::Other);
        assert_eq!(r.ordinal(11), PluralCategory::Other);
        assert_eq!(r.ordinal(21), PluralCategory::One);
    }

    #[test]
    fn invalid_locale_tag() {
        // BCP-47 tags must start with a 2-3 letter language subtag;
        // a bare digit string is always invalid.
        let result = IcuPluralResolver::new("12345");
        assert!(result.is_err());
    }

    #[test]
    fn locale_override() {
        let r = IcuPluralResolver::new("en").unwrap();
        // Arabic: 0 → Zero
        assert_eq!(r.cardinal(0, Some("ar")), PluralCategory::Zero);
        // English: 0 → Other
        assert_eq!(r.cardinal(0, None), PluralCategory::Other);
    }

    #[test]
    fn default_resolver_always_other() {
        let r = DefaultPluralResolver;
        assert_eq!(r.cardinal(0, None), PluralCategory::Other);
        assert_eq!(r.cardinal(1, None), PluralCategory::Other);
        assert_eq!(r.ordinal(1), PluralCategory::Other);
        assert_eq!(r.ordinal(42), PluralCategory::Other);
    }
}
