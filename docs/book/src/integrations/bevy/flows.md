# Spawning & Driving Flows

A *flow* is one live conversation — a `FlowInstance` and its `Context`, attached
to an entity. You spawn flows with a request component and advance them from
your own systems.

## The request-component pattern

You don't construct a flow directly. You spawn an entity carrying a
`BrinkFlowRequest<M>` (a `bon`-built builder) pointing at a story handle, and
the plugin's `fulfill_flow_requests` system materializes the flow once the
assets finish loading — no polling, no readiness latch:

```rust,ignore
commands.spawn(
    BrinkFlowRequest::<()>::builder()
        .story(assets.load("dialogue.inkb"))
        .start(FlowStart::Address("intro_scene".into()))  // optional
        .seed(ContextSeed::FromGlobals)                    // optional
        .build(),
);
```

On fulfillment the request component is **removed** and replaced with the live
flow components (below). Re-inserting the request afterward is a no-op (a debug
build warns); to restart, despawn the entity and spawn a fresh request.

### `FlowStart` — where execution begins

| Variant | Meaning |
|---------|---------|
| `Root` (default) | The file's root container. Fine for demos/tests; does **not** auto-enter a named knot. |
| `Address(String)` | Start at a knot/stitch by name. If the name is unknown, the request is dropped at fulfillment. |

### `ContextSeed` — how the flow's state is seeded

| Variant | Meaning |
|---------|---------|
| `FromGlobals` (default) | Clone the shared `BrinkGlobals<M>` "save data". |
| `FromInitial` | Use the program's fresh starting state — an independent flow that ignores the shared save. |
| `Custom(Context)` | A caller-supplied `Context` (e.g. a mid-game branch from a snapshot). |

## Flow components & resources

After fulfillment the entity carries:

| Type | Kind | Holds |
|------|------|-------|
| `BrinkFlow<M>` | Component | the `FlowInstance` (`.inner`) — call stacks, output buffer, pending choices, transcript |
| `BrinkContext<M>` | Component | this flow's in-flight `Context` (`.inner`) — globals, visit/turn counts, RNG |
| `BrinkProgram<M>` | Component | `Handle<ProgramAsset>` the flow runs against |
| `BrinkLocale<M>` | Component | `Handle<LineTablesAsset>` the flow renders with |
| `BrinkGlobals<M>` | Resource | the shared "save data" `Context`; auto-inserted on first fulfillment |

Each flow has its **own** `Context` — globals are not auto-shared between
concurrent flows. When a side conversation should contribute its changes back
to the shared save, commit explicitly:

```rust,ignore
globals.commit_from(&flow_ctx);          // wholesale "save everything"
globals.commit_progress(&flow_ctx);      // globals replace; visit/turn counts take the max
globals.commit_globals_only(&flow_ctx);  // just variables; leave counts/RNG alone
// "new game" reset:
globals.commit_from(&program.initial_context);
```

## Driving a flow

Two ways to advance, depending on whether you have `&mut World`.

### From a normal system — `step_one` / `advance_until_terminal`

These take the program + line tables (looked up from the assets via the
entity's handles), the flow's `&mut Context`, an `ExternalFnHandler`, the
entity, and `Commands`. They return `Advance`:

| `Advance` | Meaning |
|-----------|---------|
| `Line(Line)` | a line was produced and its observer event fired |
| `AwaitingQuery` | the flow paused on a world-access binding; the plugin resolver handles it — skip this flow and resume next frame |

```rust,ignore
fn drive(
    mut flows: Query<(Entity, &mut BrinkFlow<()>, &mut BrinkContext<()>,
                      &BrinkProgram<()>, &BrinkLocale<()>)>,
    programs: Res<Assets<ProgramAsset>>,
    tables: Res<Assets<LineTablesAsset>>,
    bindings: Res<BrinkBindings<()>>,
    mut commands: Commands,
) {
    for (entity, mut flow, mut ctx, prog, loc) in &mut flows {
        if flow.inner.has_pending_external() { continue; } // paused; resolver will resume it
        let (Some(p), Some(t)) = (programs.get(&prog.handle), tables.get(&loc.handle))
            else { continue; };
        let handler = bindings.handler();
        let _ = flow.advance_until_terminal(
            &p.program, &t.tables, &mut ctx.inner, &handler, entity, &mut commands,
        );
        handler.flush(&mut commands); // emit any buffered command events
    }
}
```

- `step_one` produces **one** line — for typewriter UIs that animate fragments.
- `advance_until_terminal` runs until a terminal line (`Done` / `Choices` /
  `End`), firing events for every line along the way — for click-to-continue
  dialogue. It's bounded by a 10,000-line safety cap.

If you have no bindings, pass `&brink_runtime::FallbackHandler` instead of
building one from `BrinkBindings`.

### From an exclusive system — `advance_flow`

`advance_flow::<M>(&mut World, entity) -> Result<Line, BrinkCallError>` is the
counterpart for `&mut World` contexts. It resolves world-access query bindings
*inline* (so a line like `Enemies near: {enemy_count()}.` works in one frame)
and never yields `AwaitingQuery`. See [External Functions](./bindings.md).

## Choices

A `Line::Choices` (or a `BrinkChoicesPresented` event) means the flow is waiting
for a pick. Select with `choose`:

```rust,ignore
flow.choose(&mut ctx.inner, index)?;
```

For keyboard UIs, `digit_key_to_choice_index(&keys, choices.len())` maps
`Digit1..=Digit9` to a 0-based choice index:

```rust,ignore
if let Some(idx) = digit_key_to_choice_index(&keys, choices.len()) {
    flow.choose(&mut ctx.inner, idx)?;
}
```

## Observer events

`step_one`/`advance_until_terminal` fire one `EntityEvent` per produced line,
targeted at the flow entity, so observers react to exactly the situation they
care about (no `match` on a `Line`):

| Event | Fires for | Carries |
|-------|-----------|---------|
| `BrinkLineDelivered<M>` | `Line::Text` (mid-stream) | `text`, `tags` |
| `BrinkChoicesPresented<M>` | `Line::Choices` | `text`, `tags`, `choices: Vec<Choice>` |
| `BrinkTurnDone<M>` | `Line::Done` (turn complete, `-> DONE`) | `text`, `tags` |
| `BrinkStoryEnded<M>` | `Line::End` (`-> END`) | `text`, `tags` |
| `BrinkFlowReset<M>` (dev) | a hot-reload is about to rebuild the flow | `entity` |

```rust,ignore
app.add_observer(|on: On<BrinkChoicesPresented<()>>| {
    for (i, choice) in on.event().choices.iter().enumerate() {
        println!("  [{}] {}", i + 1, choice.text);
    }
});
```

Terminal lines bundle their accumulated text in their own `text` field — a
`Choices`/`Done`/`End` event already contains the passage text leading up to
it, so a click-to-continue UI can render from terminal events alone.

## Transcripts

For a "show the whole conversation so far" view rather than per-event reaction,
add a `BrinkTranscript<M>` component (opt-in) to a flow entity. The plugin
re-renders it whenever the flow grows, the locale changes, or line tables
hot-reload:

```rust,ignore
commands.entity(flow).insert(BrinkTranscript::<()>::default());
// later:
let text = transcript.text();              // all lines joined with '\n'
let lines = &transcript.lines;             // Vec<(String, Vec<String>)> — (text, tags)
```

## Hot-reload (dev)

With the `dev` feature, flows fulfilled from a `.ink` source carry a
`BrinkReplayLog<M>`. When the source changes, the plugin rebuilds the flow
against the new program, fires `BrinkFlowReset<M>` (clear your UI), and replays
recorded choices to restore position. Record choices with `choose_recording`
instead of `choose` to feed that log.
