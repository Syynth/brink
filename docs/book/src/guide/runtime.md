# Runtime API

The `brink-runtime` crate provides the bytecode VM for executing compiled ink stories.

## Core API

| Item | Kind | Description |
|------|------|-------------|
| `link()` | Function | Link `StoryData` into `(Program, line_tables)` |
| `Program` | Struct | Immutable, shareable compiled story |
| `Story` | Struct | Per-instance mutable execution state |
| `Line` | Enum | Returned by `continue_single()` / `continue_maximally()`: `Text`, `Done`, `Choices`, or `End` |
| `Choice` | Struct | A single choice with `text`, `index`, and `tags` |
| `StoryStatus` | Enum | `Active`, `WaitingForChoice`, `Done`, `Ended` |
| `RuntimeError` | Enum | All possible runtime errors |

`link()` returns the immutable `Program` *and* the story's line tables
(`Vec<Vec<LineEntry>>`) — the swappable rendering data. Hand both to
`Story::new(&program, line_tables)`.

## Two-object model

The runtime separates compiled data from execution state:

- **`Program`** holds the immutable bytecode, variable defaults, and metadata. It is created once via `link()` and can be shared across threads.
- **`Story`** holds all mutable state: operand stack, call stack, global variables, visit counts, output buffer, and the line tables it renders with. It borrows from a `Program`.

Multiple `Story` instances can execute concurrently against the same `Program`.

## External functions

Ink stories can call external functions defined by the host. Implement the `ExternalFnHandler` trait:

```rust,ignore
trait ExternalFnHandler {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult;
}

enum ExternalResult {
    Resolved(Value),  // Return a value immediately
    Fallback,         // Use the ink-defined fallback body
    Pending,          // Async resolution (call resolve_external() later)
}
```

Use `story.continue_single_with(&handler)` (or `continue_maximally_with`) to step with external-function support. `ExternalResult::Pending` pauses the story on a deferred external; supply the result later with `story.resolve_external(value)` and resume. (The `bevy-brink` integration builds a richer binding facility on top of this — see the Bevy Integration section.)

## Named flows

Named flows allow parallel execution contexts within a single story:

```rust,ignore
story.spawn_flow("background", entry_point_id)?;
let lines = story.continue_flow_maximally("background")?; // -> Vec<Line>
story.choose_flow("background", index)?;
story.destroy_flow("background")?;
```

`flow_names()` lists the active named flows.

## Statistics

`story.stats()` returns execution counters: opcodes executed, steps, threads created/completed, frames pushed/popped, choices presented/selected, snapshot cache hits/misses, and materializations.

## RNG

The VM uses `FastRng` (a simple LCG) by default. `DotNetRng` matches the C# reference implementation's random behavior. Implement the `StoryRng` trait for custom RNG.
