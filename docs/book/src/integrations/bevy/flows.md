# Spawning & Driving Flows

A *flow* is one live conversation — a `FlowInstance` and its private
`FlowLocal` override layer, attached to an entity. You spawn flows with a
request component and advance them from your own systems.

## The request-component pattern

You don't construct a flow directly. You spawn an entity carrying a
`BrinkFlowRequest<M>` (a `bon`-built builder) pointing at a story handle, and
the plugin's `fulfill_flow_requests` system materializes the flow once the
assets finish loading — no polling, no readiness latch:

```rust,ignore
{{#include ../../../../../crates/bevy-brink/examples/book_flows.rs:spawn_request}}
```

On fulfillment the request component is **removed** and replaced with the live
flow components (below). Re-inserting the request afterward is a no-op (a debug
build warns); to restart, despawn the entity and spawn a fresh request.

### `FlowStart` — where execution begins

| Variant | Meaning |
|---------|---------|
| `Root` (default) | The file's root container. Fine for demos/tests; does **not** auto-enter a named knot. |
| `Address(String)` | Start at a knot/stitch by name. If the name is unknown, the request is dropped at fulfillment. |

Spawning a flow takes no seed/policy parameter: its `FlowLocal` always starts
fresh and empty. What's shared vs. private is a property of the *world*, set
up once — see below.

## Flow components & resources

After fulfillment the entity carries:

| Type | Kind | Holds |
|------|------|-------|
| `BrinkFlow<M>` | Component | the `FlowInstance` (`.inner`) — call stacks, output buffer, pending choices, transcript |
| `BrinkContext<M>` | Component | this flow's private `FlowLocal` (`.inner`) — overrides for whatever units the policy homes to `Local` |
| `BrinkProgram<M>` | Component | `Handle<ProgramAsset>` the flow runs against |
| `BrinkLocale<M>` | Component | `Handle<LineTablesAsset>` the flow renders with |
| `BrinkGlobals<M>` | Resource | the **one shared** `World` for marker `M` — globals, visit/turn counts, RNG; auto-inserted on first fulfillment |

## World vs. Local: one shared `World`, opt-in private state

Every flow spawned under a marker advances against the **same**
`BrinkGlobals<M>` `World`. By default every unit of story-state (`VAR`s,
visit/turn counts, RNG) is `World`-scoped — reads and writes are immediately
visible to every flow sharing it, with no "commit" step, because nothing was
ever forked. This is byte-identical to plain ink and is almost certainly what
you want for a single-flow game or for genuinely shared globals (inventory,
quest flags) across concurrent NPC conversations.

For **per-entity private state** (an NPC's own mood, its own "have I greeted
them before" history), install a policy at plugin setup naming exactly the
`VAR`s and knots that should be private — everything else stays shared:

```rust,ignore
{{#include ../../../../../crates/bevy-brink/examples/book_flows.rs:policy}}
```

A knot override covers its own visit/turn count and everything nested under
it, so sequence/cycle/stopping content (`{ Hello | Welcome back }`) inside a
`Local`-scoped knot varies per flow too. There is no "commit private state
back to shared" verb — if a private counter should eventually raise a shared
flag, write that promotion **in ink**, where it's visible (`~ if mood > 10:
~ reputation += 1`), not as a Bevy-side merge helper.

## Driving a flow

Two ways to advance, depending on whether you have `&mut World`.

### From a normal system — `step_one` / `advance_until_terminal`

These take the program + line tables (looked up from the assets via the
entity's handles), a `&mut` routing view built with
[`flow_context_view`](https://docs.rs/bevy-brink) over the entity's
`BrinkContext` and the marker's shared `BrinkGlobals`, an `ExternalFnHandler`,
the entity, and `Commands`. They return `Advance`:

| `Advance` | Meaning |
|-----------|---------|
| `Step(Step)` | a step was produced and its observer event fired |
| `AwaitingQuery` | the flow paused on a world-access binding; the plugin resolver handles it — skip this flow and resume next frame |

```rust,ignore
{{#include ../../../../../crates/bevy-brink/examples/book_flows.rs:drive}}
```

- `step_one` produces **one** line — for typewriter UIs that animate fragments.
- `advance_until_terminal` runs until a terminal line (`Done` / `Choices` /
  `End`), firing events for every line along the way — for click-to-continue
  dialogue. It's bounded by a 10,000-line safety cap per call
  (`FlowInstance::LINE_LIMIT`).

If you have no bindings, pass `&bevy_brink::FallbackHandler` instead of
building one from `BrinkBindings`.

### From an exclusive system — `advance_flow`

`advance_flow::<M>(&mut World, entity) -> Result<Step, BrinkCallError>` is the
counterpart for `&mut World` contexts. It resolves world-access query bindings
*inline* (so a line like `Enemies near: {enemy_count()}.` works in one frame)
and never yields `AwaitingQuery`. See [External Functions](./bindings.md).

## Choices

A `Step::Choices` (or a `BrinkChoicesPresented` event) means the flow is waiting
for a pick. Select with `choose`:

```rust,ignore
{{#include ../../../../../crates/bevy-brink/examples/book_flows.rs:choose}}
```

For keyboard UIs, `digit_key_to_choice_index(&keys, choices.len())` maps
`Digit1..=Digit9` to a 0-based choice index:

```rust,ignore
{{#include ../../../../../crates/bevy-brink/examples/book_flows.rs:digit_choose}}
```

## Observer events

`step_one`/`advance_until_terminal` fire one `EntityEvent` per produced step,
targeted at the flow entity, so observers react to exactly the situation they
care about (no `match` on a `Step`):

| Event | Fires for | Carries |
|-------|-----------|---------|
| `BrinkLineDelivered<M>` | `Step::Line` (mid-stream) | `text`, `tags` |
| `BrinkChoicesPresented<M>` | `Step::Choices` | `text`, `tags` (always empty), `choices: Vec<Choice>` |
| `BrinkTurnDone<M>` | `Step::Done` (turn complete, `-> DONE`) | `text`, `tags` (always empty) |
| `BrinkStoryEnded<M>` | `Step::End` (`-> END`) | `text`, `tags` (always empty) |
| `BrinkFlowReset<M>` (dev) | a hot-reload is about to rebuild the flow | `entity` |

```rust,ignore
{{#include ../../../../../crates/bevy-brink/examples/book_flows.rs:observer}}
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
{{#include ../../../../../crates/bevy-brink/examples/book_flows.rs:transcript}}
```

## Hot-reload (dev)

With the `dev` feature, flows fulfilled from a `.ink` source carry a
`BrinkReplayLog<M>`. When the source changes, the plugin rebuilds the flow
against the new program, fires `BrinkFlowReset<M>` (clear your UI), and replays
recorded choices to restore position. Record choices with `choose_recording`
instead of `choose` to feed that log.
