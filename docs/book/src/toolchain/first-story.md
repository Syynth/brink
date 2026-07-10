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
use std::path::Path;
use std::sync::Arc;
use brink_compiler::compile_path;
use brink_runtime::{Line, Story};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compile .ink source. `compile_path` returns a `CompileOutput`;
    // its `.data` field is the `StoryData`.
    let output = compile_path(Path::new("story.ink"))?;

    // Link into an immutable `Program` plus its line tables.
    let (program, line_tables) = brink_runtime::link(&output.data)?;

    // Create a story instance and run it. `continue_single` returns the
    // next `Line`; the variant tells you what to do.
    let mut story = Story::new(Arc::new(program), line_tables);

    loop {
        match story.continue_single()? {
            // Mid-stream content — keep going.
            Line::Text { text, .. } | Line::Done { text, .. } => print!("{text}"),
            Line::Choices { text, choices, .. } => {
                print!("{text}");
                for choice in &choices {
                    println!("  {}. {}", choice.index + 1, choice.text);
                }
                // Select the first choice (replace with real input).
                story.choose(choices[0].index)?;
            }
            Line::End { text, .. } => {
                print!("{text}");
                break;
            }
        }
    }

    Ok(())
}
```

If you already have a compiled `.inkb` file, decode it directly instead of
compiling:

```rust,ignore
use std::sync::Arc;
use brink_runtime::{Line, Story};

let bytes = std::fs::read("story.inkb")?;
let story_data = brink_format::read_inkb(&bytes)?;
let (program, line_tables) = brink_runtime::link(&story_data)?;
let mut story = Story::new(Arc::new(program), line_tables);
// ... step loop as above
```
