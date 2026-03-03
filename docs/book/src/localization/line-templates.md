# Line Templates

Lines in brink can be plain strings or templates with interpolation slots and plural/gender selects.

## `LineContent`

```rust,ignore
enum LineContent {
    Plain(String),
    Template(LineTemplate),
}

type LineTemplate = Vec<LinePart>;
```

## Template parts

```rust,ignore
enum LinePart {
    Literal(String),
    Slot(u8),              // value interpolation (index into evaluation stack snapshot)
    Select {
        slot: u8,
        variants: Vec<(SelectKey, String)>,
        default: String,
    },
}
```

<!-- TODO: explain each LinePart variant with examples:
  - Literal — static text fragments between dynamic parts
  - Slot — runtime value injection, e.g. "You have {0} gold"
  - Select — plural/keyword branching, e.g. "{0} {0:cardinal:one=apple|other=apples}"
-->

## Select keys

```rust,ignore
enum SelectKey {
    Cardinal(PluralCategory),
    Ordinal(PluralCategory),
    Exact(i32),
    Keyword(String),
}
```

<!-- TODO: explain each SelectKey variant:
  - Cardinal — CLDR cardinal plural categories (zero, one, two, few, many, other)
  - Ordinal — CLDR ordinal plural categories (1st, 2nd, 3rd, etc.)
  - Exact — matches a specific integer value
  - Keyword — matches a named keyword (for gender, custom categories)
-->

## Choice text decomposition

Ink choices have up to three text parts: start content (before `[`), choice-only content (inside `[]`), and output-only content (after `]`). The compiler decomposes each choice into two independent lines:

- **Display line** = start + choice-only
- **Output line** = start + output-only

<!-- TODO: explain why this matters for localization:
  - Translators localize each line independently
  - Target language can use completely different grammatical constructions
  - No structural coupling between prompt and narrative output
-->
