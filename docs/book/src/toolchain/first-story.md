# Quick Start

## Playing a story from the command line

```sh
# Compile an ink story to binary
brink compile story.ink -o story.inkb

# Play it interactively
brink play story.inkb
```

## Embedding the runtime in Rust

```rust,no_run
# extern crate brink_compiler;
# extern crate brink_runtime;
use std::path::Path;
use std::sync::Arc;
use brink_compiler::compile_path;
use brink_runtime::{Step, Story};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compile .ink source. `compile_path` returns a `CompileOutput`;
    // its `.data` field is the `StoryData`.
    let output = compile_path(Path::new("story.ink"))?;

    // Link into an immutable `Program` plus its line tables.
    let (program, line_tables) = brink_runtime::link(&output.data)?;

    // Create a story instance and run it. `continue_single` returns the
    // next `Step`; the variant tells you what to do.
    let mut story: Story = Story::new(Arc::new(program), line_tables);

    loop {
        match story.continue_single()? {
            // Mid-stream content — keep going.
            Step::Line(line) => print!("{}", line.text),
            // This turn's output is complete; the story isn't over.
            Step::Done => {}
            Step::Choices(choices) => {
                for choice in &choices {
                    println!("  {}. {}", choice.index + 1, choice.text);
                }
                // Select the first choice (replace with real input).
                story.choose(choices[0].index)?;
            }
            Step::End => break,
            // Reserved for flow suspension; not yet emitted.
            Step::Suspended => break,
        }
    }

    Ok(())
}
```

If you already have a compiled `.inkb` file, decode it directly instead of
compiling:

```rust,no_run
# extern crate brink_format;
# extern crate brink_runtime;
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use std::sync::Arc;
use brink_runtime::Story;

let bytes = std::fs::read("story.inkb")?;
let story_data = brink_format::read_inkb(&bytes)?;
let (program, line_tables) = brink_runtime::link(&story_data)?;
let mut story: Story = Story::new(Arc::new(program), line_tables);
// ... step loop as above
# let _ = &mut story;
# Ok(())
# }
```
