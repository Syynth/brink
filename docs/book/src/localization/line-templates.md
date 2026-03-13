# Line Templates

Lines in brink can be plain strings or templates with interpolation slots and plural/gender selects.

## LineContent

```rust,ignore
enum LineContent {
    Plain(String),
    Template(LineTemplate),
}

struct LineTemplate {
    parts: Vec<LinePart>,
}
```

## Template parts

```rust,ignore
enum LinePart {
    Literal(String),
    Slot(u8),
    Select {
        slot: u8,
        variants: Vec<(SelectKey, String)>,
        default: String,
    },
}
```

- **Literal** -- static text fragments between dynamic parts.
- **Slot** -- runtime value interpolation. The `u8` is an index into the evaluation stack snapshot captured when the line is emitted. For example, `"You have {0} gold"` becomes `[Literal("You have "), Slot(0), Literal(" gold")]`.
- **Select** -- plural/keyword branching. Selects a variant string based on the runtime value at the given slot, using a `SelectKey` to match. Falls back to `default` if no variant matches.

## Select keys

```rust,ignore
enum SelectKey {
    Cardinal(PluralCategory),
    Ordinal(PluralCategory),
    Exact(i32),
    Keyword(String),
}
```

- **Cardinal** -- CLDR cardinal plural categories (zero, one, two, few, many, other). Used for "1 apple" vs "2 apples".
- **Ordinal** -- CLDR ordinal categories. Used for "1st", "2nd", "3rd".
- **Exact** -- matches a specific integer value. Useful for special-casing "0 items" or "exactly 1".
- **Keyword** -- matches a named string key. Used for gender or custom grammatical categories.

## Line tables

Line tables are stored per-container in the `.inkb` format. Each container has a sequence of `LineEntry` values referenced by index from `EmitLine` opcodes. The `EvalLine` opcode handles templates with interpolation, evaluating slots from the current stack state.

## Choice text decomposition

Ink choices have up to three text parts: start content (before `[`), choice-only content (inside `[]`), and output-only content (after `]`). The compiler decomposes each choice into two independent lines:

- **Display line** = start + choice-only (what the player sees in the choice list)
- **Output line** = start + output-only (what appears in the narrative after selection)

This decomposition allows translators to localize each line independently -- the target language can use completely different grammatical constructions for the prompt and the narrative output.
