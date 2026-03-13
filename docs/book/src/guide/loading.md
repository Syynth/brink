# Loading & Linking

Before running a story, you need to produce `StoryData` and link it into a `Program`.

## Producing StoryData

There are two paths:

**From `.ink` source** (native compiler):

```rust,ignore
let story_data = brink_compiler::compile_path("story.ink")?;
```

**From `.inkb` bytes** (pre-compiled binary):

```rust,ignore
let bytes = std::fs::read("story.inkb")?;
let story_data = brink_format::inkb::decode(&bytes)?;
```

## Linking

```rust,ignore
let program = brink_runtime::link(&story_data)?;
```

The linker resolves all `DefinitionId` references to compact runtime indices, validates the container graph, and initializes global variable defaults. The result is an immutable `Program`.

## Creating stories

```rust,ignore
let mut story = Story::new(&program);
```

`Story` borrows from `Program`. You can create multiple stories from the same program for parallel execution or replaying with different choices.

## Error cases

- **`Decode`** -- corrupt or incompatible `.inkb` file (wrong magic, bad checksum, truncated data)
- **`UnresolvedDefinition`** -- a container references a `DefinitionId` that doesn't exist in the story data
- **`NoRootContainer`** -- the story has no entry point container
