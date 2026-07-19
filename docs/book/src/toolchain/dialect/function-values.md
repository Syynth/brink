# Function values

Ink has no lexical free variables — a knot or stitch never closes over
anything in its surrounding scope — so brink's function values are **partial
application over named functions**, not closures. The author-facing name is
deliberate: "function value," never "closure" or "lambda." There are no
anonymous functions in brink; every function value starts from a
statically-named `=== function name ===`.

A function value is three things: the target's identity, a prefix of its
declared parameters bound at creation, and an effect row (reserved for a
future milestone, always present, currently the conservative "may do
anything" placeholder). That's the whole model — everything below is
consequences of it.

## Creating one: `#fn(name, args…)`

`#fn` joins the `#[…]` / `#{…}` sigil family. It takes a statically-named
function and binds a **prefix** of its declared parameters:

```ink
VAR player_hp = 10

~ temp healer = #fn(heal, player_hp)
~ temp healed = healer(5)
Healed to {healed}.
HP cell is now {player_hp}.
-> END

=== function heal(ref hp: int, amount: int): int ===
~ hp = hp + amount
~ return hp
```

```text
Healed to 15.
HP cell is now 15.
```

Capture mode isn't something you choose at the creation site — it comes from
the target's own signature. `heal`'s first parameter is `ref hp`, so binding
it captures the durable cell `player_hp` itself: every call through
`healer` reads and writes the *same* `player_hp`, exactly like calling
`heal(player_hp, …)` directly would. A `val` parameter, bound the same way,
snapshots the argument's value at creation instead.

**Every `ref` parameter must be bound at creation**, and only to a durable
cell — a `VAR` (including a flow-private `#@local` one), never a `temp`, a
`CONST`, or an expression. `temp`s and params die with their frame; a
function value that outlived one holding a dangling ref would be exactly the
kind of silent misbinding brink's value model refuses to produce. The
checker enforces this at the `#fn` site itself: an unbound `ref` parameter,
or one bound to something that isn't a durable cell, is a compile error
before you ever get to call anything. Once every `ref` parameter is bound,
every parameter *after* it is `val`-only by construction — a function value
you can pass around and call dynamically is always values-only from that
point on.

`#fn`'s target is a **name**, never an expression — `#fn(heal, …)`, not
`#fn(some_expr, …)`. Binding more arguments than the target declares, or
pointing `#fn` at something that isn't a statically-named function (a
variable, a plain knot, a builtin), is a compile error too.

## Calling: two forms

A function value can be called directly, or through the `call()` intrinsic
when the callee itself needs to be an expression rather than a bare name in
call position:

```ink
~ temp adder = #fn(add, 10)
~ temp total = call(adder, 7)
Total: {total}.
-> END

=== function add(a: int, b: int): int ===
~ return a + b
```

```text
Total: 17.
```

`adder(7)` would have produced the same `17` — `call(f, args…)` exists for
the shapes where the direct-call form `f(args…)` isn't accepted (a function
value stored behind an index expression, a field, or handed back from
another expression), not as a second calling convention with different
semantics. Direct-call syntax only ever binds a bare variable/temp/param
name; writing `handlers[state](event)` or `obj.field()` in its place is a
compile error (`E104`) naming `call(f, args…)` as the fix, not a silent
no-op — see issue #869.

## `bind()`: currying an existing function value

Where `#fn` creates a function value from a name, `bind()` curries an
*existing* one — it consumes the head of whatever parameters remain unbound
and returns a new function value with those filled in:

```ink
~ temp f = #fn(combine)
~ temp g = bind(f, 1)
~ temp h = bind(g, 2)
~ temp result = h(3)
Result: {result}.
-> END

=== function combine(a: int, b: int, c: int): int ===
~ return a + b + c
```

```text
Result: 6.
```

`bind` chains compose: `g` binds one more parameter onto `f`, `h` binds one
more onto `g`, and by the time every declared parameter is filled the
result is callable with zero further arguments. `bind`'s appended arguments
are always `val` — the remaining parameters after a `#fn` creation site are
val-only by construction, so there's no `ref` capture decision left to make
by the time `bind` runs.

## Display form

`string(f)` — and interpolation, which routes through the same display
machinery — renders a function value as a signature, with bound arguments
shown as defaults:

```ink
VAR world_hp = 10

~ temp healer = #fn(heal, world_hp)
Display: {healer}.
-> END

=== function heal(ref hp: int, amount: int): int ===
~ hp = hp + amount
~ return hp
```

```text
Display: fn heal(ref hp = world_hp, amount).
```

A bound `ref` parameter shows the captured cell's *name* (`hp = world_hp`,
not the cell's current value — the binding is to the cell, not a snapshot);
a bound `val` parameter shows its value; an unbound parameter shows bare.
This form is permanently observable surface, not a debug aid that might
change shape later — treat it the same as any other stable display rule.

## What can go wrong, and when

Two failure classes exist, and they land at different times depending on
whether the calling code is typed:

- **Under `types = strict`**, calling through a function value with a known
  `fn(T…): R` type is checked statically — a wrong argument count or a
  wrong argument type is a compile error, exactly like calling an ordinary
  function. An escape to `Unknown` at a call site is the same strict-mode
  escape error every other call gets.
- **Under `types = gradual`** (the strict-ink dialect's default — since the
  2026-07-19 ruling the brink dialect defaults to `types = strict` — and the
  mode strict static checks fall back to when a type can't be pinned down),
  calling a
  non-function value, calling with the wrong number of arguments, or
  passing a wrong-typed argument is a **turn-terminating runtime fault** —
  never silent garbage, never a partially-applied call that quietly does
  the wrong thing.

  `call()` and `bind()` are strict-mode gradual-typed for now: their result
  type isn't yet threaded through the `fn(T…): R` lattice the way a direct
  `#fn`-created value's type is, so a mistake reaches them as the same
  runtime fault gradual mode always has, even in an otherwise-strict
  project. Direct calls (`f(args…)`) get the full strict-mode static
  treatment already.

One fault is specific to function values: invoking one that `ref`-binds a
flow-private (`#@local`) cell from outside the flow that created it. brink
doesn't yet track which flow created a given function value, so rather than
risk a silent cross-flow misbinding, invoking a closure over a `#@local`
`ref` binding is a defined fault. A `ref`-bound ordinary `VAR` (shared
across every flow on a `World`) has no such restriction — it's only the
flow-private storage class this applies to.

## Persistence

A function value saves like any other value — there's no special case in
`SaveState` for one sitting in a `VAR`, inside an array or map, or live on
the stack mid-turn when a save happens. Loading it back re-validates the
saved parameter names and modes against the *current* compiled signature:
if a recompile has since renamed, reordered, or re-moded a parameter the
saved value referenced, invoking it after load is a defined fault rather
than a silent misbinding against the wrong slot. This is a best-effort
check, not a cross-version compatibility guarantee — a function value is as
long-lived as the story build it was saved against.
