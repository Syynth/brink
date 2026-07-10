# Loading & Linking

Before running a story, you need to produce `StoryData` and link it into a `Program`.

## Producing StoryData

There are two paths:

**From `.ink` source** (native compiler): `compile_path` returns a
`CompileOutput`; its `.data` field is the `StoryData`.

```rust
# extern crate brink_compiler;
# fn demo() -> Result<(), Box<dyn std::error::Error>> {
use std::path::Path;
let output = brink_compiler::compile_path(Path::new("story.ink"))?;
let story_data = output.data;
# let _ = story_data;
# Ok(())
# }
```

**From `.inkb` bytes** (pre-compiled binary):

```rust
# extern crate brink_format;
# fn demo() -> Result<(), Box<dyn std::error::Error>> {
let bytes = std::fs::read("story.inkb")?;
let story_data = brink_format::read_inkb(&bytes)?;
# let _ = story_data;
# Ok(())
# }
```

## Linking

```rust
# extern crate brink_format;
# extern crate brink_runtime;
# use brink_format::StoryData;
# fn demo(story_data: StoryData) -> Result<(), Box<dyn std::error::Error>> {
let (program, line_tables) = brink_runtime::link(&story_data)?;
# let _ = (program, line_tables);
# Ok(())
# }
```

The linker resolves all `DefinitionId` references to compact runtime indices, validates the container graph, and initializes global variable defaults. It returns the immutable `Program` together with the story's line tables (`Vec<Vec<LineEntry>>`) — the localizable rendering data, kept separate so it can be swapped for a locale overlay or hot-reloaded without rebuilding the program.

## Creating stories

```rust
# extern crate brink_format;
# extern crate brink_runtime;
# use brink_format::{LineEntry, StoryData};
# use brink_runtime::Program;
# fn demo(program: Program, line_tables: Vec<Vec<LineEntry>>) {
use std::sync::Arc;
use brink_runtime::Story;

let mut story: Story = Story::new(Arc::new(program), line_tables);
# let _ = &mut story;
# }
```

`Story` holds an `Arc<Program>` and owns the line tables it renders with. Because the handle is refcounted rather than borrowed, a `Story` has no lifetime parameter and can be moved or stored freely. You can create multiple stories from the same program — clone the `Arc` — for parallel execution or replaying with different choices.

## Error cases

- **`Decode`** — corrupt or incompatible `.inkb` file (wrong magic, bad checksum, truncated data)
- **`UnresolvedDefinition`** — a container references a `DefinitionId` that doesn't exist in the story data
- **`NoRootContainer`** — the story has no entry point container
