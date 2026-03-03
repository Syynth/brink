# Choices

When a story yields `StepResult::Choices`, the player must select one before execution can continue.

```rust,ignore
StepResult::Choices { text, choices } => {
    for choice in &choices {
        println!("{}: {}", choice.index + 1, choice.text);
    }
    // Get player's selection (0-indexed into the choices vec)
    story.choose(choices[selected].index)?;
}
```

## The `Choice` struct

<!-- TODO: explain Choice fields:
  - text: the display text for this choice
  - index: the internal index to pass to story.choose()
  - Note: index may not be contiguous (invisible defaults are filtered out)
-->

## Choice semantics

<!-- TODO: explain ink's choice types and how they surface in the API:
  - Once-only choices (*, default) — disappear after being selected
  - Sticky choices (+) — remain available
  - Fallback / invisible default choices — auto-selected, never shown to player
  - Conditional choices — only appear when their condition is true
-->

## Errors

<!-- TODO: explain choice-related errors:
  - NotWaitingForChoice — called choose() when not in WaitingForChoice status
  - InvalidChoiceIndex — index out of range
-->
