# The Step Loop

The core execution model is a synchronous step function that runs until a yield
point and returns a `Line`. The variant tells you what to do next.

```rust,ignore
use brink_runtime::{Line, Story};

let mut story = Story::new(&program, line_tables);

loop {
    match story.continue_single()? {
        // Mid-stream content; more may follow this turn.
        Line::Text { text, tags } => print!("{text}"),
        // This turn's output is complete (`-> DONE`); keep stepping.
        Line::Done { text, tags } => print!("{text}"),
        Line::Choices { text, tags, choices } => {
            print!("{text}");
            // Present `choices`, get the player's selection...
            story.choose(chosen_index)?;
        }
        Line::End { text, tags } => {
            print!("{text}");
            break;
        }
    }
}
```

## `Line` variants

| Variant | Meaning | Next action |
|---------|---------|-------------|
| `Text { text, tags }` | One line of content. More may follow this turn. | Call `continue_single()` again. |
| `Done { text, tags }` | The turn's output is complete (ink `done`). The story is **not** over. | Call `continue_single()` again for the next turn. |
| `Choices { text, tags, choices }` | The story is waiting for a choice. | Call `story.choose(index)`, then continue. |
| `End { text, tags }` | The story reached `-> END`. Permanently finished. | Stop stepping. |

Every variant carries the `text` produced since the last yield point and any ink
tags (`# tag`) attached to it. The helpers `line.text()`, `line.tags()`, and
`line.is_terminal()` work across variants (`is_terminal()` is true for anything
but `Text`).

## `continue_single` vs `continue_maximally`

- `continue_single() -> Line` produces **one** line — ideal for typewriter UIs
  that reveal content a line at a time.
- `continue_maximally() -> Vec<Line>` runs until a terminal line and returns
  every line produced along the way; the **last** element is always a terminal
  variant (`Done`, `Choices`, or `End`). Ideal for click-to-continue UIs that
  show a whole passage at once.

```rust,ignore
loop {
    let lines = story.continue_maximally()?;
    for line in &lines {
        print!("{}", line.text());
    }
    match lines.last() {
        Some(Line::Choices { choices, .. }) => story.choose(choices[0].index)?,
        Some(Line::End { .. }) | None => break,
        _ => {} // Done — loop again for the next turn.
    }
}
```

Both have `_with(&handler)` variants (`continue_single_with`,
`continue_maximally_with`) that take a custom `ExternalFnHandler` for
[external functions](./runtime.md#external-functions).

## StoryStatus

You can query `story.status()` at any time:

| Status | Meaning |
|--------|---------|
| `Active` | Ready to step. |
| `WaitingForChoice` | Must call `choose()` before stepping. |
| `Done` | Hit a `done` opcode. Can resume with `continue_single()`. |
| `Ended` | Hit `-> END`. Cannot step further. |

## Text accumulation

A story may produce several `Text`/`Done` lines before reaching `Choices` or
`End`. Each `continue_single()` carries only the text since the previous yield.
If your application needs the full passage, accumulate text across lines until a
`Choices` or `End` arrives — or use `continue_maximally()`, which batches a whole
passage for you.
