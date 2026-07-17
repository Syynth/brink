# The Execution Model

A compiled story runs as a synchronous **step function**: it executes bytecode
until it reaches a yield point, then hands back a `Line`. The variant of that
`Line` tells you what just happened and what to do next. This loop is the shared
foundation under every client — raw Rust, Bevy, the web runner all express the
same model.

```rust
# extern crate brink_format;
# extern crate brink_runtime;
# use brink_format::{LineEntry, StoryData};
# use brink_runtime::{Program, RuntimeError};
# fn demo(program: Program, line_tables: Vec<Vec<LineEntry>>) -> Result<(), RuntimeError> {
# let chosen_index = 0usize;
use std::sync::Arc;
use brink_runtime::{Line, Story};

let mut story: Story = Story::new(Arc::new(program), line_tables);

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
        // The flow parked on a wake condition (flow suspension). Reserved:
        // not emitted by the runtime until the suspension milestone lands.
        Line::Suspended { text, tags } => {
            print!("{text}");
            break;
        }
    }
}
# Ok(())
# }
```

> `Story` is the mutable half of the [two-object model](../embedding/index.md#the-two-object-model):
> it holds an `Arc<Program>` and carries all the execution state. How that state
> is partitioned — and how it can be shared across flows or kept private — is
> the subject of [The State Model](./state-model.md).

## `Line` variants

| Variant | Meaning | Next action |
|---------|---------|-------------|
| `Text { text, tags }` | One line of content. More may follow this turn. | Call `continue_single()` again. |
| `Done { text, tags }` | The turn's output is complete (ink `done`). The story is **not** over. | Call `continue_single()` again for the next turn. |
| `Choices { text, tags, choices }` | The story is waiting for a choice. | Call `story.choose(index)`, then continue. |
| `End { text, tags }` | The story reached `-> END`. Permanently finished. | Stop stepping. |
| `Suspended { text, tags }` | The flow parked on a wake condition (brink flow suspension). **Reserved** — not yet emitted by the runtime. | Stop driving; resume when the host's wake surface reports the flow runnable. |

Every variant carries the `text` produced since the last yield point and any ink
tags (`# tag`) attached to it. The helpers `line.text()`, `line.tags()`, and
`line.is_terminal()` work across variants (`is_terminal()` is true for anything
but `Text`).

## `continue_single` vs `continue_maximally`

- `continue_single() -> Line` produces **one** line — ideal for typewriter UIs
  that reveal content a line at a time.
- `continue_maximally() -> Vec<Line>` runs until a terminal line and returns
  every line produced along the way; the **last** element is always a terminal
  variant (`Done`, `Choices`, `End`, or — once flow suspension lands — `Suspended`). Ideal for click-to-continue UIs that
  show a whole passage at once.

```rust
# extern crate brink_runtime;
# use brink_runtime::{Line, RuntimeError, Story};
# fn demo(story: &mut Story) -> Result<(), RuntimeError> {
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
# Ok(())
# }
```

Both have `_with(&handler)` variants (`continue_single_with`,
`continue_maximally_with`) that take a custom `ExternalFnHandler` for
[external functions](../embedding/external-functions.md).

## Choices

When the story yields `Line::Choices`, execution is blocked until you select one
with `story.choose(index)`:

```rust
# extern crate brink_runtime;
# use brink_runtime::{Line, RuntimeError, Story};
# fn demo(story: &mut Story, line: Line, selected: usize) -> Result<(), RuntimeError> {
# match line {
Line::Choices { text, choices, .. } => {
    for choice in &choices {
        println!("{}: {}", choice.index + 1, choice.text);
    }
    story.choose(choices[selected].index)?;
}
# _ => {}
# }
# Ok(())
# }
```

Each `Choice` carries:

| Field | Type | Description |
|-------|------|-------------|
| `text` | `String` | display text for this choice |
| `index` | `usize` | the value to pass to `story.choose()` |
| `tags` | `Vec<String>` | tags attached to this choice |

Ink defines several choice *kinds*, but they're resolved by the compiler and VM
— the runtime always hands you a flat `Vec<Choice>` of the ones currently
selectable:

- **Once-only** (`*`) — the default; disappears after it's taken.
- **Sticky** (`+`) — stays available on later visits.
- **Fallback** — has no display text; auto-selected when nothing else is
  available, and never appears in the `choices` vec.
- **Conditional** — guarded by a condition; only present when the guard is true.

Choice-related errors (`InvalidChoiceIndex`, `NotWaitingForChoice`) are listed in
[Reference › Errors](../reference/errors.md).

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
