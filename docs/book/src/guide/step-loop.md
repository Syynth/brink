# The Step Loop

The core execution model is a synchronous step function that runs until a yield point.

```rust,ignore
let mut story = Story::new(&program);

loop {
    match story.continue_maximally()? {
        StepResult::Done { text, tags, .. } => {
            print!("{text}");
        }
        StepResult::Choices { text, choices, tags, .. } => {
            print!("{text}");
            // Present choices, get player's selection...
            story.choose(chosen_index)?;
        }
        StepResult::Ended { text, tags, .. } => {
            print!("{text}");
            break;
        }
    }
}
```

## StepResult variants

| Variant | Meaning | Next action |
|---------|---------|-------------|
| `Done { text, tags }` | Story yielded text at a `done` point. More content may follow. | Call `continue_maximally()` again. |
| `Choices { text, choices, tags }` | Story yielded text and is waiting for a choice. | Call `story.choose(index)`, then `continue_maximally()`. |
| `Ended { text, tags }` | Story reached `-> END`. Permanently finished. | Stop stepping. |

Each variant carries the text produced since the last yield point. The `tags` field contains any ink tags (`# tag`) attached to the current output.

## StoryStatus

You can also query `story.status()` at any time:

| Status | Meaning |
|--------|---------|
| `Active` | Ready to step. |
| `WaitingForChoice` | Must call `choose()` before stepping. |
| `Done` | Hit a `done` opcode. Can resume with `continue_maximally()`. |
| `Ended` | Hit `-> END`. Cannot step further. |

## Text accumulation

A story may produce multiple `Done` results in sequence before reaching `Choices` or `Ended`. Each `StepResult` carries only the text produced since the previous yield. If your application needs the full passage text, accumulate across `Done` results until a `Choices` or `Ended` arrives.
