# Loading & Linking

Before running a story, you need to produce `StoryData` and link it into a `Program`.

## Producing StoryData

There are two paths:

**From `.ink` source** (native compiler): `compile_path` returns a
`CompileOutput`; its `.data` field is the `StoryData`.

```rust,ignore
use std::path::Path;
let output = brink_compiler::compile_path(Path::new("story.ink"))?;
let story_data = output.data;
```

**From `.inkb` bytes** (pre-compiled binary):

```rust,ignore
let bytes = std::fs::read("story.inkb")?;
let story_data = brink_format::read_inkb(&bytes)?;
```

## Linking

```rust,ignore
let (program, line_tables) = brink_runtime::link(&story_data)?;
```

The linker resolves all `DefinitionId` references to compact runtime indices, validates the container graph, and initializes global variable defaults. It returns the immutable `Program` together with the story's line tables (`Vec<Vec<LineEntry>>`) — the localizable rendering data, kept separate so it can be swapped for a locale overlay or hot-reloaded without rebuilding the program.

## Creating stories

```rust,ignore
let mut story = Story::new(&program, line_tables);
```

`Story` borrows from `Program` and owns the line tables it renders with. You can create multiple stories from the same program for parallel execution or replaying with different choices.

## Error cases

- **`Decode`** — corrupt or incompatible `.inkb` file (wrong magic, bad checksum, truncated data)
- **`UnresolvedDefinition`** — a container references a `DefinitionId` that doesn't exist in the story data
- **`NoRootContainer`** — the story has no entry point container
