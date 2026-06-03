# Plural Resolution

brink uses CLDR plural categories for locale-aware text. The runtime itself ships no locale data — consumers provide a resolver via the `PluralResolver` trait.

## PluralCategory

```rust,ignore
enum PluralCategory {
    Zero,
    One,
    Two,
    Few,
    Many,
    Other,
}
```

These correspond to the six CLDR plural categories. Different languages use different subsets — English uses `One` and `Other`, Arabic uses all six, Japanese uses only `Other`.

## The PluralResolver trait

```rust,ignore
trait PluralResolver {
    fn cardinal(&self, n: i64, locale_override: Option<&str>) -> PluralCategory;
    fn ordinal(&self, n: i64) -> PluralCategory;
}
```

- **`cardinal()`** — determines the plural form for cardinal numbers. "1 apple" vs "2 apples" in English; more complex rules in other languages.
- **`ordinal()`** — determines the plural form for ordinal numbers. "1st", "2nd", "3rd", "4th" in English.
- **`locale_override`** — allows per-call locale switching for mixed-language stories.

## No resolver (fallback)

Stories without localization don't need a resolver. When no resolver is provided, all plural selects fall back to `PluralCategory::Other`, and `Select` parts in line templates use their `default` variant.

## Custom implementation

Implement `PluralResolver` for your own type to provide locale-aware plural handling:

```rust,ignore
struct EnglishPlurals;

impl PluralResolver for EnglishPlurals {
    fn cardinal(&self, n: i64, _locale: Option<&str>) -> PluralCategory {
        if n == 1 { PluralCategory::One } else { PluralCategory::Other }
    }

    fn ordinal(&self, n: i64) -> PluralCategory {
        match n % 10 {
            1 if n % 100 != 11 => PluralCategory::One,
            2 if n % 100 != 12 => PluralCategory::Two,
            3 if n % 100 != 13 => PluralCategory::Few,
            _ => PluralCategory::Other,
        }
    }
}
```

## Batteries-included resolvers

The `brink-intl` crate ships two ready-made resolvers so you don't have to hand-write CLDR rules:

- **`IcuPluralResolver`** — backed by ICU4X with CLDR baked data (~50 KB), correct for every CLDR locale.
- **`DefaultPluralResolver`** — a minimal English-only resolver for stories that don't localize.

```rust,ignore
use brink_intl::IcuPluralResolver;
let resolver = IcuPluralResolver::new();
// pass `Some(&resolver)` to render_transcript / apply_locale rendering
```
