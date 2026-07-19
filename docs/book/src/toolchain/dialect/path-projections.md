# Path projections

`ref npc.hp`, `ref inventory[idx]`, `ref party[leader].hp` — a **path
projection** is a `ref` argument that names a path into a durable cell
instead of the whole cell. It's the same `ref` you already know (a
function's `ref` parameter binds the caller's cell, not a copy), extended
so the bound path can reach *inside* a struct field or a collection
element instead of stopping at the cell's name.

A path projection is a value with an identity — `(root cell, path
segments)` — never a pointer into memory. Reads walk the path against the
root's *current* value; writes are a read-modify-write on the *root cell*.
That framing explains everything below: why the index in `ref arr[i]` is
fixed the moment the `ref` is created, why a path that stops resolving is a
defined error rather than undefined behavior, and why two overlapping
projections into the same cell never need an aliasing check to stay
deterministic.

## Creating one: `ref` in argument position

A path projection is created only where `ref` already exists — a direct
argument of a call, `#fn(…)`, or `bind(…)`. There's no standalone
projection expression (`temp r = ref a[0]` is a compile error): projections
exist only at the boundary where a `ref` parameter is being bound.

```ink
STRUCT NPC = #{hp: int, name: string}
VAR npc = 0

~ npc = NPC#{hp: 10, name: "Aeris"}
~ heal(ref npc.hp, 5)
{npc.name} has {npc.hp} HP.
-> END

=== function heal(ref hp: int, amount: int) ===
~ hp = hp + amount
```

```text
Aeris has 15 HP.
```

`heal`'s `ref hp` parameter is unchanged from ordinary `ref` — it binds
whatever cell the caller names. What's new is that the caller can name a
*path* into a cell (`npc.hp`) rather than only the bare cell itself. Inside
`heal`, `hp` behaves exactly like it always has: reading it walks the path
to get the current field value, writing it walks the path and stores back
into `npc`.

The root of a projection's path must always be a durable cell — a `VAR`
(including a flow-private `#@local`) — the same rule ordinary `ref`
arguments already follow. A `temp`, a `CONST`, or an expression result
can't anchor a projection, for the same reason they can't anchor a plain
`ref`: they don't outlive the call.

## Index expressions snapshot at creation

The segments of a path projection — including any `[index]` subexpression
— are evaluated once, when the `ref` argument is created. Reassigning the
variable that produced an index afterward never retargets an
already-created projection:

```ink
VAR inventory = 0

~ inventory = #[10, 20, 30]
~ temp idx = 0
~ bump(ref inventory[idx], 100)
~ idx = 2
{inventory[0]} {inventory[1]} {inventory[2]}
-> END

=== function bump(ref x: int, k: int) ===
~ x = x + k
```

```text
110 20 30
```

`ref inventory[idx]` captured `idx == 0` at the moment `bump` was called.
Reassigning `idx` to `2` right after has no effect on the projection
`bump` is already holding — only `inventory[0]` moved.

## Overlapping projections write through immediately

Two separate projections into the same root cell never need reconciling
against each other — every write lands on the root cell the instant it
happens, so a read through one projection always sees whatever the most
recent write through any projection left behind:

```ink
STRUCT NPC = #{hp: int, name: string}
VAR npc = 0

~ npc = NPC#{hp: 0, name: "Aeris"}
~ heal(ref npc.hp, 5)
~ heal(ref npc.hp, 7)
{npc.hp}
-> END

=== function heal(ref hp: int, k: int) ===
~ hp = hp + k
```

```text
12
```

Nothing about this needs aliasing analysis: `heal(ref npc.hp, 5)` reads
`npc.hp`, adds `5`, and stores back into `npc` before `heal(ref npc.hp,
7)` ever creates its own projection — the second call's read already
sees `5`.

## When a path stops resolving

A projection's path is only guaranteed to resolve against the root's value
*at the moment it created* — the value can change shape before the
projection is read or written (the array shrinks below the snapshotted
index, a map loses the snapshotted key, a struct field the projection
names gets removed). When that happens, the read or write that discovers
it is a defined, turn-terminating runtime fault — never a silent clamp,
never undefined behavior. There's nothing to catch in-story; treat it the
same as any other fault your host-integration error handling already
covers.

## Through `#fn`

A path projection can be the bound `ref` argument of `#fn`, exactly like a
bare-cell `ref` can (see [Function Values](./function-values.md)):

```ink
STRUCT NPC = #{hp: int, name: string}
VAR npc = 0

~ npc = NPC#{hp: 5, name: "Aeris"}
~ temp healer = #fn(heal, ref npc.hp)
~ temp result = healer(9)
{result}
-> END

=== function heal(ref hp: int, amount: int): int ===
~ hp = hp + amount
~ return hp
```

```text
14
```

`healer` closes over the path `npc.hp`, not a snapshot of its value at
creation — calling `healer` later still reads and writes through to
whatever `npc.hp` holds at call time.

## Display form

`string(p)` — and interpolation, which routes through the same display
machinery — renders a path projection as `ref` followed by the root cell's
name and its path segments: a dotted field renders `.field`, an index
renders `[value]`. This is the same display convention `#fn`'s bound-`ref`
rendering already uses for a bare cell (`ref hp = player_hp`) — a
projection-bound parameter shows its *path* in that same slot instead of a
bare name:

```ink
STRUCT NPC = #{hp: int, name: string}
VAR npc = 0

~ npc = NPC#{hp: 5, name: "Aeris"}
~ temp healer = #fn(heal, ref npc.hp)
{healer}
-> END

=== function heal(ref hp: int, amount: int) ===
~ hp = hp + amount
```

```text
fn heal(ref hp = npc.hp, amount)
```

This form is deliberately boring and stable — it names the root and the
path, never the current value at the root (the binding is to the path, not
a snapshot of what it currently holds).

## What can't happen: a path never crosses to the host

An `EXTERNAL` function's declared parameters have no `ref` grammar at
all — only a knot or function header can mark a parameter `ref`. That
means a path projection can never be the argument bound to an `EXTERNAL`
call: `heal(ref npc.hp, 5)` only ever type-checks against a target whose
own signature declares a `ref` parameter, and no `EXTERNAL` declaration
can. Passing `ref npc.hp` to an `EXTERNAL` function is a compile error,
not a runtime concern.

That's a deliberate consequence of the value model, not an incidental
gap: the whole point of "a projection is `(root cell, path)`, never an
interior pointer" is that only ink bytecode — which knows how to walk a
path back to the root cell — ever handles the unresolved form. Reading a
projection-bound parameter inside an ordinary ink function (the only way
to ever observe one) always resolves it to a plain value first, so
whatever your engine's external-function bindings receive is always an
ordinary snapshot — an `int`, a `string`, a struct, whatever the path
resolved to — never the path itself. The host side of a binding facility
never needs to know path projections exist.

## Persistence

A path projection saves like a plain `ref` binding always has — there's no
special case in `SaveState` for one sitting mid-call. Loading it back
re-validates the root cell the same way an ordinary `ref` parameter's
saved binding already does; a recompile that renamed or removed the root
is the same defined fault a dangling ordinary `ref` binding would produce,
not a silent misbind.
