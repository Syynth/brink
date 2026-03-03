# Plural Resolution

Brink uses CLDR plural categories for locale-aware text. The runtime itself ships no locale data — consumers provide a resolver via the `PluralResolver` trait.

## `PluralCategory`

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

## The `PluralResolver` trait

```rust,ignore
trait PluralResolver {
    fn cardinal(&self, n: i64, locale_override: Option<&str>) -> PluralCategory;
    fn ordinal(&self, n: i64) -> PluralCategory;
}
```

<!-- TODO: explain the trait:
  - cardinal() — "1 apple" vs "2 apples" (most common)
  - ordinal() — "1st", "2nd", "3rd" (English), locale-specific rules
  - locale_override — allows per-call locale switching (e.g. mixed-language stories)
-->

## `brink-intl`

The `brink-intl` crate provides a batteries-included `PluralResolver` backed by ICU4X baked data, pruned at build time to only the locales the consumer specifies.

<!-- TODO: usage example once brink-intl is implemented:
  - Adding brink-intl as a dependency
  - Constructing a resolver with specific locales
  - Passing it to the runtime
-->

## No resolver (fallback)

Stories without localization don't need a resolver. When no resolver is provided, all plural lookups fall back to `PluralCategory::Other`.
