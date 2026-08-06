# Handles (T1d)

An ink `Value::Handle` is an opaque `{kind, id}` token — a *name* for a host
resource (an entity, a timer, an audio instance), never a live pointer. The
script world holds only the token; dereferencing happens host-side, inside a
binding, against a registry `bevy-brink` owns. See `docs/t1d-spec.md` §4 for
the full design; this page covers the `bevy-brink` side of it.

## `HandleKind` — the two-halved trait

Each kind of host resource a story can hold a handle to implements one
trait, split by which side of the save boundary the knowledge lives on:

```rust
# extern crate bevy_ecs;
# extern crate bevy_brink;
# extern crate serde;
# use bevy_ecs::world::World;
# use bevy_brink::HandleKind;
# use serde::{Serialize, Deserialize};
#[derive(Serialize, Deserialize, Clone)]
struct TimerSaveKey { remaining_secs: f32 }

struct TimerState { remaining_secs: f32 }

struct Timer;

impl HandleKind for Timer {
    const KIND: &'static str = "Timer";
    type Resource = TimerState;
    type SaveKey = TimerSaveKey;

    fn save_key(&self, _world: &World, res: &TimerState) -> Option<TimerSaveKey> {
        Some(TimerSaveKey { remaining_secs: res.remaining_secs })
    }

    fn resolve(&self, _world: &mut World, key: &TimerSaveKey) -> Option<TimerState> {
        // Timers are resumable: `resolve` doesn't look one up, it rebuilds
        // one from the recipe `save_key` captured.
        Some(TimerState { remaining_secs: key.remaining_secs })
    }
}
```

`SaveKey` is a **reconstruction recipe, not just a foreign key** — pick a
point on the spectrum per kind: identity lookup (an NPC GUID, an asset
path), reconstruction (the timer above — resumable), or deliberate
ephemerality. Returning `None` from `save_key` for a particular live
resource means "this one is meaningless across sessions" — an implementor
choice, never a category the spec assigns. `resolve` returning `None` means
the recipe no longer names anything live (e.g. the NPC despawned) — a
normal, expected outcome, never a fault.

## Registering a kind

```rust
# extern crate bevy_app;
# extern crate bevy_ecs;
# extern crate bevy_brink;
# extern crate serde;
# use bevy_app::App;
# use bevy_ecs::world::World;
# use bevy_brink::{HandleKind, BrinkHandleAppExt};
# use serde::{Serialize, Deserialize};
# #[derive(Serialize, Deserialize, Clone)]
# struct TimerSaveKey { remaining_secs: f32 }
# struct TimerState { remaining_secs: f32 }
# struct Timer;
# impl HandleKind for Timer {
#     const KIND: &'static str = "Timer";
#     type Resource = TimerState;
#     type SaveKey = TimerSaveKey;
#     fn save_key(&self, _world: &World, res: &TimerState) -> Option<TimerSaveKey> {
#         Some(TimerSaveKey { remaining_secs: res.remaining_secs })
#     }
#     fn resolve(&self, _world: &mut World, key: &TimerSaveKey) -> Option<TimerState> {
#         Some(TimerState { remaining_secs: key.remaining_secs })
#     }
# }
# fn demo(app: &mut App) {
app.register_handle_kind::<(), Timer>(Timer);
# }
```

This inserts `Timer`'s `HandleRegistry<Timer>` resource (opaque `u64` token
ids, live-resource storage) and indexes it under `"Timer"` in the app's
type-erased `HandleKinds<()>` — the index everything below (`is_valid`,
save/load, GC) dispatches through when it only has a runtime `Value::Handle`
to work from, not a static `K`.

Mint a token from inside any binding that has the resource to hand and the
loaded `Program` (to resolve `Timer`'s name id):

```rust,ignore
let value = registry.mint_value(program, TimerState { remaining_secs: 30.0 });
```

`mint_value` returns `None` if this compile never interned `"Timer"` — a
kind name only exists in a program's name table if the source graph
actually references `Handle<Timer>` somewhere (a typed binding signature or
annotation).

## `is_valid` — the standard world-query binding

`bevy-brink` registers `is_valid(h)` automatically on every `BrinkPlugin<M>`
— no wiring needed. It's an ordinary `bind_brink_query` binding (per spec,
not a language intrinsic): call it from ink like any other `EXTERNAL`.
Dead, unregistered-kind, and non-handle arguments all just return `false` —
`is_valid` never faults.

## Dead handles and declared failure values

Dereferencing a dead handle is **never UB and never a turn fault**. A
binding looks its token up in the typed `HandleRegistry<K>` and, on a miss,
returns whatever failure value it has chosen to declare — `Value::Null`, a
sentinel int, whatever fits the binding's contract:

```rust,ignore
fn npc_name(In((flow, args)): In<BrinkQueryInput>, registry: Res<HandleRegistry<Npc>>, mut commands: Commands) -> Value {
    let Some((_, id)) = args.first().and_then(Value::as_handle) else { return Value::Null };
    match registry.get_or_dead::<()>(id, &mut commands, flow) {
        Some(npc) => Value::String(npc.name.clone().into()),
        None => Value::Null, // the declared failure value for this binding
    }
}
```

`get_or_dead` is opt-in telemetry: a dead lookup fires `BrinkDeadHandleDeref<M>`
(entity, kind, id) at the flow entity, so a host can log/count misses without
that changing what the binding returns. Use plain `HandleRegistry::get` for a
silent lookup.

## Save/load and the rehydration report

`bevy-brink` persists the `token → SaveKey` table beside the ink `SaveState`,
and **keeps token ids stable across a load** — the ink state (which only
ever holds `{kind, id}`) is untouched; only the registry's right-hand side
(the live resources) rebinds:

```rust,ignore
// Alongside BrinkGlobals::save_state:
let handle_save = save_handles::<()>(app.world());

// Alongside BrinkGlobals::load_state, after the ink SaveState is loaded:
let report = load_handles::<()>(
    app.world_mut(),
    &program,
    &script_save_state,
    &handle_save,
    RehydrationPolicy::Lenient,
)?;
```

`load_handles` walks every `Value::Handle` reachable from `script_save_state`
(including tokens nested in arrays/maps/records/closures) and buckets each
one into the returned `RehydrationReport`:

| Bucket | Meaning |
|--------|---------|
| `rebound` | resolved to a live resource, same token id |
| `dead_by_resolve` | a registered kind, a persisted recipe, but `resolve` returned `None` — normal |
| `dead_ephemeral` | a registered kind with no persisted entry — the kind chose ephemerality for this token |
| `dead_by_unregistered_kind` | the kind isn't registered at all — suspicious (integration drift) |

**Never-fail-load holds** — `load_handles` always returns `Ok` under the
production-default [`RehydrationPolicy::Lenient`], even with unregistered
kinds present (they land in `dead_by_unregistered_kind` for you to log).
`RehydrationPolicy::StrictKinds` is the dev/CI knob: an unregistered kind
fails the whole call loudly instead, so a registration that drifted out of
sync with a save file surfaces immediately rather than silently dropping
state.

## Registry GC at `-> DONE`

Script state is fully enumerable, so `bevy-brink` computes the live
handle-token set — every token reachable from the shared `World`'s globals
plus every flow's own local state — at every `-> DONE` a flow reaches, and
drops each registered kind's unreachable registry entries. No script-side
destructors exist or are needed; this is wired automatically by
`BrinkPlugin<M>` (see `gc_on_turn_done`). A dev-only
`HandleRetentionMetrics<M>` resource tracks each kind's live count and
last-sweep drop count — a diagnostic, not a semantic.

## `EntityMapper` integration

For a kind whose `Resource` is `Entity`, cross-references inside a
`SaveKey` may point at entities by their *old* session's id. `resolve` can
consult and populate the app-wide `HandleEntityRemap` resource (implements
`bevy_ecs::entity::EntityMapper`, reset at the start of every
`load_handles` call) to translate between old and new `Entity` ids as it
reconstructs scene-based resources.
