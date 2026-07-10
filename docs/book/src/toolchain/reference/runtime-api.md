# Runtime API

The public surface of `brink-runtime` — the crate that executes compiled
stories. For how these pieces fit together conceptually, see
[The Execution Model](../concepts/execution-model.md); for a worked walkthrough,
see [Embedding the Runtime](../embedding/index.md).

## Core API

| Item | Kind | Description |
|------|------|-------------|
| `link()` | Function | Link `StoryData` into `(Program, line_tables)` |
| `Program` | Struct | Immutable, shareable compiled story |
| `Story` | Struct | Per-instance mutable execution state |
| `Line` | Enum | Yielded by `continue_single()` / `continue_maximally()`: `Text`, `Done`, `Choices`, `End` |
| `Choice` | Struct | A single choice — `text`, `index`, `tags` |
| `StoryStatus` | Enum | `Active`, `WaitingForChoice`, `Done`, `Ended` |
| `RuntimeError` | Enum | All runtime errors (see [Errors](./errors.md)) |

`link()` returns the immutable `Program` **and** the story's line tables
(`Vec<Vec<LineEntry>>`) — the swappable rendering data. Hand both to
`Story::new(Arc::new(program), line_tables)`.

```rust
# extern crate brink_format;
# extern crate brink_runtime;
# use brink_format::StoryData;
# use brink_runtime::{RuntimeError, Story};
# fn demo(story_data: StoryData) -> Result<(), RuntimeError> {
use std::sync::Arc;

let (program, line_tables) = brink_runtime::link(&story_data)?;
let mut story: Story = Story::new(Arc::new(program), line_tables);
# let _ = &mut story;
# Ok(())
# }
```

`Story` takes the program by `Arc`, not by reference, so it carries no lifetime
parameter — clone the `Arc` to run many stories against one `Program`.

## Stepping

| Method | Returns | Use |
|--------|---------|-----|
| `continue_single()` | `Line` | one line at a time (typewriter UIs) |
| `continue_maximally()` | `Vec<Line>` | a whole passage, last element terminal |
| `continue_single_with(&h)` / `continue_maximally_with(&h)` | as above | same, with an `ExternalFnHandler` |
| `choose(index)` | `()` | select a choice when `WaitingForChoice` |
| `status()` | `StoryStatus` | query state at any time |

See [The Execution Model](../concepts/execution-model.md) for the step loop, the
`Line` variants, and `StoryStatus`, and
[External Functions](../embedding/external-functions.md) for the `_with` forms.

## Named flows

`spawn_flow`, `continue_flow_maximally`, `choose_flow`, `destroy_flow`,
`flow_names` — parallel execution contexts within one story. See
[Named Flows](../embedding/named-flows.md).

## Statistics

`story.stats()` returns execution counters: opcodes executed, steps, threads
created/completed, frames pushed/popped, choices presented/selected, snapshot
cache hits/misses, and materializations. Useful for profiling and for asserting
on VM behavior in tests.

## RNG

The VM uses `FastRng` (a simple LCG) by default. `DotNetRng` reproduces the C#
reference implementation's random behavior — use it when you need bit-for-bit
parity with inklecate. Implement the `StoryRng` trait for a custom source.
