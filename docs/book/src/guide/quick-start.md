# Quick Start

## Playing a story from the command line

<!-- TODO: example .ink file, compile it, play it with `brink play` -->

```sh
brink play story.ink.json
```

## Embedding the runtime in Rust

<!-- TODO: minimal example showing the full loop — this is the runtime's existing doc example -->

```rust,ignore
let program = brink_runtime::link(&story_data)?;
let mut story = brink_runtime::Story::new(&program);

loop {
    match story.continue_maximally()? {
        StepResult::Done { text } => print!("{text}"),
        StepResult::Choices { text, choices } => {
            print!("{text}");
            // present choices to the player, get their selection...
            story.choose(chosen_index)?;
        }
        StepResult::Ended { text } => {
            print!("{text}");
            break;
        }
    }
}
```

<!-- TODO: explain where story_data comes from (loading .inkb / .inkt / .ink.json) -->
