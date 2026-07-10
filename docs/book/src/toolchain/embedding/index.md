# Embedding the Runtime

`brink-runtime` is the bytecode VM. Embed it to drive ink stories from a Rust
program — a game, a tool, a custom engine. It depends only on `brink-format`, so
pulling it in doesn't drag the compiler along.

This section is the hands-on path. For the mental model behind it, read
[The Execution Model](../concepts/execution-model.md); for the exhaustive API
surface, see [Reference › Runtime API](../reference/runtime-api.md).

## The two-object model

The runtime keeps compiled data and execution state in separate objects — this
is the one structural idea to internalize:

- **`Program`** — the immutable bytecode, variable defaults, and metadata. Built
  once via `link()`, shareable across threads.
- **`Story`** — all the mutable state: operand stack, call stack, globals, visit
  counts, output buffer, and the line tables it renders with. It holds an
  `Arc<Program>`.

Because `Program` is immutable, **many `Story` instances can run concurrently
against one `Program`** — parallel playthroughs, or replaying with different
choices, share the compiled data for free.

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

`Story` owns a refcount, not a borrow, so it carries no lifetime — it can be
moved into a thread, stored in a struct, or held in an ECS component without
threading a `'p` parameter through your types. To fan out playthroughs, clone
the `Arc` (cheap) and give each `Story` its own line tables:

```rust
# extern crate brink_format;
# extern crate brink_runtime;
# use std::sync::Arc;
# use brink_format::LineEntry;
# use brink_runtime::{Program, Story};
# fn demo(program: Program, line_tables: Vec<Vec<LineEntry>>) {
let program = Arc::new(program);
let mut a: Story = Story::new(Arc::clone(&program), line_tables.clone());
let mut b: Story = Story::new(Arc::clone(&program), line_tables);
# let _ = (&mut a, &mut b);
# }
```

## The shape of embedding

1. **[Loading & Linking](./loading.md)** — produce `StoryData` (compile `.ink`
   or read `.inkb`) and `link()` it into a `Program` + line tables.
2. **Drive it** — step the story and react to each `Line`. The loop, the `Line`
   variants, and choice handling all live in
   [The Execution Model](../concepts/execution-model.md).
3. **[External Functions](./external-functions.md)** — let the story call back
   into your code (`EXTERNAL` functions), synchronously or deferred.
4. **[Named Flows](./named-flows.md)** — run parallel execution contexts within
   one story.
5. **[Sessions & Replay](./sessions.md)** — journal a playthrough for a save
   file, deterministic replay, and state snapshots/diffs.
6. **[Speculation](./speculation.md)** — run the story forward from its current
   state without committing to it, then discard the run.

A minimal driver looks like this — see the execution-model page for what each
arm means:

```rust
# extern crate brink_runtime;
# use brink_runtime::{Line, RuntimeError, Story};
# fn demo(story: &mut Story) -> Result<(), RuntimeError> {
loop {
    match story.continue_single()? {
        Line::Text { text, .. } | Line::Done { text, .. } => print!("{text}"),
        Line::Choices { text, choices, .. } => {
            print!("{text}");
            story.choose(/* player's pick */ choices[0].index)?;
        }
        Line::End { text, .. } => { print!("{text}"); break; }
    }
}
# Ok(())
# }
```
