# Quick Start

## Playing a story from the command line

```sh
# Compile an ink story to binary
brink compile story.ink -o story.inkb

# Play it interactively
brink play story.inkb
```

## Embedding the runtime in Rust

```rust,ignore
use brink_compiler::compile_path;
use brink_runtime::{link, Story, StepResult};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compile .ink source to StoryData
    let story_data = compile_path("story.ink")?;

    // Link into an executable program
    let program = link(&story_data)?;

    // Create a story instance and run it
    let mut story = Story::new(&program);

    loop {
        match story.continue_maximally()? {
            StepResult::Done { text, .. } => {
                print!("{text}");
            }
            StepResult::Choices { text, choices, .. } => {
                print!("{text}");
                for choice in &choices {
                    println!("  {}. {}", choice.index + 1, choice.text);
                }
                // Select the first choice (replace with user input)
                story.choose(choices[0].index)?;
            }
            StepResult::Ended { text, .. } => {
                print!("{text}");
                break;
            }
        }
    }

    Ok(())
}
```

If you already have a compiled `.inkb` file, load it directly instead of compiling:

```rust,ignore
use brink_format::inkb;
use brink_runtime::{link, Story, StepResult};

let bytes = std::fs::read("story.inkb")?;
let story_data = inkb::decode(&bytes)?;
let program = link(&story_data)?;
let mut story = Story::new(&program);
// ... step loop as above
```
