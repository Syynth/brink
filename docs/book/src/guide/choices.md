# Choices

When a story yields `StepResult::Choices`, the player must select one before execution can continue.

```rust,ignore
StepResult::Choices { text, choices, .. } => {
    for choice in &choices {
        println!("{}: {}", choice.index + 1, choice.text);
    }
    story.choose(choices[selected].index)?;
}
```

## The Choice struct

| Field | Type | Description |
|-------|------|-------------|
| `text` | `String` | Display text for this choice |
| `index` | `usize` | Index to pass to `story.choose()` |
| `tags` | `Vec<String>` | Tags attached to this choice |

## Choice semantics in ink

Ink defines several choice types. These are handled by the compiler and VM -- the runtime API always presents them as a `Vec<Choice>`:

- **Once-only** (`*`) -- the default. Disappears after being selected.
- **Sticky** (`+`) -- remains available on subsequent visits.
- **Fallback** -- a choice with no display text, auto-selected when no other choices are available. Never appears in the `choices` vec.
- **Conditional** -- a choice guarded by a condition. Only appears when its condition is true.

## Errors

- **`InvalidChoiceIndex`** -- the index passed to `choose()` is not in the valid range. Check `choices.len()` before selecting.
- **`NotWaitingForChoice`** -- `choose()` was called when the story was not in `WaitingForChoice` status.
