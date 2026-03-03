# Runtime API

The `brink-runtime` crate is the primary library interface for embedding ink stories in Rust applications. It provides a bytecode VM that executes compiled stories.

## Public API

| Item | Kind | Description |
|------|------|-------------|
| `link()` | Function | Link `StoryData` into an immutable `Program` |
| `Program` | Struct | Immutable, shareable compiled story (one per story file) |
| `Story` | Struct | Per-instance mutable execution state |
| `StepResult` | Enum | Result of `Story::step()` — `Done`, `Choices`, or `Ended` |
| `Choice` | Struct | A single choice with `text` and `index` |
| `StoryStatus` | Enum | Current status: `Active`, `WaitingForChoice`, `Done`, `Ended` |
| `RuntimeError` | Enum | All possible runtime errors |

## Design

<!-- TODO: explain the two-object model (Program + Story)
  - Program is immutable and shareable (Arc-friendly)
  - Story holds all mutable state (stacks, globals, output buffer, visit counts)
  - One Program can drive many Story instances
-->

<!-- TODO: link to the architecture chapter for VM internals -->
