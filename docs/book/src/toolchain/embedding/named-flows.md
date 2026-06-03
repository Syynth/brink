# Named Flows

A single `Story` can run several independent execution contexts at once —
**named flows**. Each flow has its own position, call stack, and output, while
sharing the story's globals and visit counts. They're how you model a background
conversation, a parallel subplot, or a side channel that advances on its own.

```rust,ignore
story.spawn_flow("background", entry_point_id)?;     // start a flow at an address
let lines = story.continue_flow_maximally("background")?;  // -> Vec<Line>
story.choose_flow("background", index)?;             // pick a choice in that flow
story.destroy_flow("background")?;                   // tear it down
```

`flow_names()` lists the currently active named flows.

The default, unnamed flow is the one driven by `continue_single` /
`continue_maximally` / `choose`. The `*_flow` variants target a flow by name and
otherwise behave identically — same `Line` results, same choice protocol.

## Errors

- **`UnknownFlow`** — referenced a flow name that isn't active.
- **`FlowAlreadyExists`** — `spawn_flow` with a name that's already in use.

See [Reference › Errors](../reference/errors.md) for the full list.

> For engine integration, `bevy-brink` exposes each flow as an entity with its
> own components rather than name-keyed lookups — see
> [Bevy › Spawning & Driving Flows](../../integrations/bevy/flows.md).
