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
  counts, output buffer, and the line tables it renders with. It borrows from a
  `Program`.

Because `Program` is immutable, **many `Story` instances can run concurrently
against one `Program`** — parallel playthroughs, or replaying with different
choices, share the compiled data for free.

```rust,ignore
let (program, line_tables) = brink_runtime::link(&story_data)?;
let mut story = Story::new(&program, line_tables);
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

A minimal driver looks like this — see the execution-model page for what each
arm means:

```rust,ignore
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
```
