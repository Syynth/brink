# The Step Loop

The core execution model is a synchronous step function that runs until a yield point.

```rust,ignore
let mut story = Story::new(&program);

loop {
    match story.step(&program)? {
        StepResult::Done { text } => {
            // Story paused — more content may follow.
            // Call step() again to continue.
            print!("{text}");
        }
        StepResult::Choices { text, choices } => {
            // Story is waiting for player input.
            print!("{text}");
            // ... present choices, get player's selection ...
            story.choose(chosen_index)?;
        }
        StepResult::Ended { text } => {
            // Story is permanently finished.
            print!("{text}");
            break;
        }
    }
}
```

## `StepResult` variants

<!-- TODO: explain each variant in detail:
  - Done: yielded text, can resume with another step(). Story may produce more
    Done results before reaching Choices or Ended.
  - Choices: yielded text AND choices. Must call choose() before next step().
  - Ended: story hit an `-> END`. Cannot step further.
-->

## `StoryStatus`

<!-- TODO: explain the status enum and when each state is active:
  - Active — ready to step
  - WaitingForChoice — must call choose() next
  - Done — paused, can resume
  - Ended — permanently finished
-->

## Text accumulation

<!-- TODO: explain that text may come in pieces across multiple Done results
     before a Choices or Ended. The step loop often needs to accumulate. -->
