# External Functions (ink ↔ engine)

The binding facility connects ink `EXTERNAL` functions to engine code, and lets
engine code call ink functions. The boundary is split by **World access**, not
by sync/async return shape: bindings that need no World resolve inline; those
that do (or that take time) pause the flow and resume out-of-band.

Register bindings at app-build time via `BrinkBindingsAppExt` (they live in a
`BrinkBindings<M>` resource). Each verb takes the marker `M` as its first
explicit type parameter — use `()` for the default single-story case.

## ink → engine: the five kinds

| Verb | For | Resolution |
|------|-----|------------|
| `bind_brink_fn` | pure compute, no World | inline while the VM steps |
| `bind_brink_command` | fire-and-forget event (with optional return) | buffered during the step, flushed after |
| `bind_brink_query` | read live World state | flow pauses; a resolver runs the binding system, then resumes |
| `bind_brink_async` | multi-frame World interaction (UI, input) | flow parks; `BrinkExternalAwaited` fires; you resolve when ready |
| `bind_brink_task` | off-thread compute / IO | flow parks; the future runs on the task pool; resolved on completion |

### `bind_brink_fn` — pure functions

A side-effect-free function of the ink args, resolved inline (no World, no
latency). The return type is anything `Into<Value>`:

```rust,ignore
app.bind_brink_fn::<(), _, _>("clamp01", |args| {
    args.first().and_then(Value::as_float).unwrap_or(0.0).clamp(0.0, 1.0)
});
```

### `bind_brink_command` — fire-and-forget events

Parse the ink args into a Bevy `Event` and trigger it. Derive `BrinkCommand`
for structs whose fields are `i32`/`f32`/`bool`/`String`; react with a normal
observer:

```rust,ignore
#[derive(Event, BrinkCommand)]
struct PlaySound { name: String }

app.bind_brink_command::<(), PlaySound>("play_sound")
   .add_observer(|on: On<PlaySound>| { /* play on.event().name */ });
```

The event is buffered while the VM steps (the handler can't touch the World
mid-step) and emitted when the flow's handler is flushed. To return a value to
ink, hand-implement `BrinkCommand` and override `reply()`.

### `bind_brink_query` — read the World

A Bevy system with arbitrary `SystemParam`s that reads the World and returns a
`Value`. It takes `In<BrinkQueryInput>` — `(Entity, Vec<Value>)`, the calling
flow plus the ink args — so a binding can query *anything*, with no upfront
declaration:

```rust,ignore
fn enemy_count(In((_flow, _args)): In<BrinkQueryInput>, q: Query<&Enemy>) -> Value {
    Value::Int(q.iter().count() as i32)
}
app.bind_brink_query::<(), _, _>("enemy_count", enemy_count);
```

Resolving a query needs World access, so it can't run inline. The flow pauses
(`ExternalResult::Pending`); from a normal system it yields `Advance::AwaitingQuery`
and the plugin's `resolve_pending_externals` system (gated on
`any_flow_awaiting_external`) runs the binding via `run_system_with` and resumes
it. From an exclusive `&mut World` context, `advance_flow` resolves it inline in
one frame.

## ink → engine: async (defer-across-frames) bindings

Some externals can't resolve in one pass — a targeting UI that waits for a
click, or a network round-trip. The flow *parks* on the pending external and is
**frozen until resolved**, so the flow entity itself is the correlation key (no
per-call id needed).

Async bindings are a step-loop feature only: the one-pass exclusive drivers
(`advance_flow`, `call_ink_function`) return
`BrinkCallError::AsyncExternalUnsupported` on them.

### `bind_brink_async` — the event primitive (World interaction)

When ink calls the external, the flow parks and `BrinkExternalAwaited<M>` (an
`EntityEvent` carrying `name` + `args`) fires *once* at the flow entity. Do your
multi-frame work and resolve via `resolve_brink_external` whenever ready:

```rust,ignore
app.bind_brink_async::<()>("pick_target");

app.add_observer(|on: On<BrinkExternalAwaited<()>>, mut commands: Commands| {
    if on.event().name == "pick_target" {
        // … open a targeting UI; many frames later, when the player clicks:
        commands.resolve_brink_external::<()>(on.event().entity, Value::Int(7));
    }
});
```

`resolve_brink_external` is guarded by `has_pending_external`, so a stale or
double resolve is a safe no-op. Runnable demo:
`cargo run --example async_external`.

### `bind_brink_task` — off-thread compute

Sugar over `AsyncComputeTaskPool`. You hand it an `async` closure; bevy-brink
spawns the future, parks a `BrinkPendingTask<M>`, and resolves the flow with the
output once it completes (polled each frame by `poll_brink_tasks`):

```rust,ignore
app.bind_brink_task::<(), _, _>("expensive_roll", |args: Vec<Value>| async move {
    let sides = args.first().and_then(Value::as_int).unwrap_or(6);
    Value::Int(compute_roll(sides).await)
});
```

The future is `Send + 'static` and runs off the main thread, so it **cannot
access the World** — it computes from the ink args only. For World-dependent
async, use `bind_brink_async`. Runnable demo:
`cargo run --example async_task`.

## engine → ink: calling ink functions

Evaluate an ink function from engine code, out-of-band (output isolated,
transcript untouched, visit counts not bumped). The function may itself call
world-access query bindings — they resolve as part of the call.

### From an exclusive system — `call_ink_function`

```rust,ignore
let can_advance = call_ink_function::<()>(world, flow_entity, "can_advance", &[])?;
```

Synchronous; resolves world-access query bindings inline because it holds
`&mut World`.

### From a normal system — `commands.brink_call(...).observe(...)`

A normal (non-exclusive) system can't call `call_ink_function` directly, so it
*requests* a deferred call. The result is delivered to an observer scoped to a
unique per-call entity — it can never be mis-correlated with another call:

```rust,ignore
commands
    .brink_call::<()>(flow_entity, "can_advance", (in_combat,))
    .observe(|on: On<BrinkCallResolved<()>>| {
        let result = on.event().value.as_bool();
        // …
    });
```

`brink_call` accepts `()`, tuples of `Into<Value>` (up to 4), `Vec<Value>`, or
`&[Value]` as args. The plugin's resolver fires `BrinkCallResolved<M>` (with
`value`) or `BrinkCallFailed<M>` (with an error string) at the call entity, then
despawns it. Runnable demo: `cargo run --example engine_bindings`.

## Wiring summary

```rust,ignore
app.bind_brink_fn::<(), _, _>("clamp01", clamp01)
   .bind_brink_command::<(), PlaySound>("play_sound")
   .bind_brink_query::<(), _, _>("enemy_count", enemy_count)
   .bind_brink_async::<()>("pick_target")
   .bind_brink_task::<(), _, _>("expensive_roll", expensive_roll);
```

Unknown `EXTERNAL` names fall through to `ExternalResult::Fallback`, so the
story's in-ink fallback body (if any) runs.
