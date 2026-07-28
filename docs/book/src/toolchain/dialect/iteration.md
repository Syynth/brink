# Iteration

Closing time at the Last Light. The innkeeper works down the register one
name at a time, and the slate settles itself:

```ink
VAR owed = 0
VAR roll = ""

~ {
    temp tab = #{"Mira": 4, "Old Tom": 7, "Edda": 2}
    for guest in tab {
        owed = owed + tab[guest]
        roll = roll + " " + guest
    }
}
Closing time. The innkeeper reads the slate:{roll}.
{owed} coins still owed between them.
-> END
```

```text
Closing time. The innkeeper reads the slate: Mira Old Tom Edda.
13 coins still owed between them.
```

One loop, three guests, two facts computed — and then the prose reads the
results. That shape is the whole chapter in miniature: **loops live in
logic blocks and compute; the narration comes after, and reads what they
computed.** What remains is the contracts — what `for` accepts, what
exactly it iterates when the collection changes under it, what `while`,
`break`, and `continue` add, why a runaway loop fails loudly instead of
hanging your story, and where the ruled-but-unlanded pieces of the
iteration surface currently stand.

> **Current spelling** — examples in this chapter compile in today's brink
> dialect: collection literals carry the `#[…]`/`#{…}` sigils, verbs are
> free calls (`push(tab, 5)`, `len(tab)`), and loops live inside `~ { … }`
> logic blocks. The loop syntax itself — `for name in expr { … }`,
> `while cond { … }`, `break`, `continue` — is already the ruled shape;
> what changes with the native frontend is the spelling around it (bare
> `[…]` literals, method calls, lambdas). The ruled two-binding form
> `for k, v in m` now parses and lowers on the native `.brink` surface;
> this chapter's `~ { … }` brink-dialect examples still have no
> two-binding spelling (see below).

## Loops compute; prose narrates

Narrative iteration is rarer than programming instinct suggests, and the
language is shaped around that honestly. Prose cannot loop: a `for` or
`while` is a *statement*, legal only inside a `~ { … }` logic block, and a
block is fenced to pure computation — no text output, no choices, no
diverts ever appear inside one (see [Logic Blocks](blocks.md)). You will
never write "for each guest, print a paragraph" as a loop around
narration.

What loops are *for* is the step before narration: totalling a ledger,
scanning an inventory, updating every entry of a grid, finding the first
thing that matters. The working idiom is the one the opening scene used —
**compute first, narrate second**: run the loop in a block, accumulate
into a variable (a number, a built-up string, a collection), then
interpolate the result. And for the most common narrative "iteration" of
all — a line that varies each time the story passes through it — the brace
alternations (`{& …}`, `{! …}` and kin) are the right tool, not a loop at
all; they belong to the prose dialect and its own chapter.

## `for` and the closed iterable set

`for name in expr { … }` walks a sequence, binding each element to `name`
and running the body once per element. What can stand after `in` is a
**closed set** — exactly three kinds, each with a defined element:

| Iterable | Elements | Order |
|---|---|---|
| array `[T]` | its values | the array's own order |
| map `[K: V]` | its **keys** | insertion order |
| range | its integers | ascending |

Nothing else iterates — not strings, not `Option`, not a number.
Iterating over anything outside the set is a malformed question in the
[Option chapter's](option.md) sense: a turn-terminating "not indexable"
fault at runtime, never a silent zero-times loop. Under strict types the
mistake usually surfaces earlier and indirectly — a loop variable with no
element type to be escapes inference as `Unknown`, the familiar `E065`
from [Values & Types](types.md).

The iterable is a full expression, evaluated **once**, at loop entry.
Iterating a function's result or a functional verb's copy is ordinary:

```ink
VAR order = ""

~ {
    temp tab = #[4, 7, 2]
    for coins in sorted(tab) {
        order = order + " " + coins
    }
}
Debts called in, smallest first:{order}.
-> END
```

```text
Debts called in, smallest first: 2 4 7.
```

Map iteration order is a language guarantee, not an accident: keys come
back in insertion order, deterministically, on every platform, every run
— the same promise `keys(m)`/`values(m)` make in
[Collections](literals.md), and part of the same replay-stability
contract everything else obeys.

## The loop variable

The loop variable is a fresh, block-scoped binding — it exists only
inside the body, takes the iterable's element type under strict
(`for coins in tab` makes `coins` an `int` when `tab` is an
`Array<int>`), and does not leak. It may share a name with an outer
`temp`; that's shadowing, and it's legal but flagged with the `E054`
warning ("shadows an already-visible temp"), because it is almost always
either deliberate or a bug — never innocuous:

```ink
VAR log = ""

~ {
    temp round = 999
    log = log + "before=" + round
    for round in #[1, 2, 3] {
        log = log + " round=" + round
    }
    log = log + " after=" + round
}

{log}
-> END
```

```text
before=999 round=1 round=2 round=3 after=999
```

The outer `round` is untouched — the loop wrote its own `round`, three
times, and the outer binding was waiting, intact, at the closing brace.
(The example compiles; the compiler just makes you look at it.)

One thing the loop variable is *not*: a handle back into the collection.
Assigning to it changes the binding, never the array element it was
copied from — collections are values, and the loop hands you copies. The
blessed way to mutate elements in place is the range-index idiom below.

## What the loop actually iterates

Here is the contract worth carrying around: **a `for` loop iterates the
value its iterable expression produced at entry — not the variable it
came from.** The sequence is fixed once, before the first pass; nothing
the body does to the source variable changes how many times the loop
runs or what it sees.

For arrays this falls straight out of the value model. The loop reads
`tab` once and holds that value; a `push(tab, …)` in the body writes a
*new* value into the variable, and the loop's copy never sees it:

```ink
VAR seen = 0
VAR grown = 0

~ {
    temp tab = #[4, 7, 2]
    for coins in tab {
        push(tab, coins)
        seen = seen + 1
    }
    grown = len(tab)
}
The loop visited {seen} entries; the tab ended the night at {grown}.
-> END
```

```text
The loop visited 3 entries; the tab ended the night at 6.
```

Three passes, not six, and certainly not forever — appending while
iterating can never turn a loop infinite, by construction.

For maps the same guarantee is a deliberate ruling rather than a free
consequence, and it has a name: **the key set is snapshotted eagerly at
loop entry** (F10). The loop walks the keys the map had when it started;
structural changes mid-loop — inserting, removing — affect the live map
immediately but never the walk:

```ink
VAR knocked = ""
VAR listed = 0

~ {
    temp rooms = #{"Mira": 3, "Old Tom": 1}
    for guest in rooms {
        rooms["Edda"] = 7
        knocked = knocked + " " + guest
    }
    listed = len(rooms)
}
The innkeeper knocked for{knocked} — yet the register now lists {listed} guests.
-> END
```

```text
The innkeeper knocked for Mira Old Tom — yet the register now lists 3 guests.
```

The honest edge of the snapshot: only the *keys* are snapshotted. A
`rooms[guest]` read in the body is a live read of the current map — which
is exactly what you want (you see the values as they now are), with one
sharp corner: if the body `remove`s a key the snapshot still holds, a
later `rooms[that_key]` read is the ordinary faulting missing-key read
from [Collections](literals.md). The snapshot never invents an entry to
hide the removal — a removed key reads as exactly what it is.

Reading keys and values together is the everyday map loop:

```ink
~ temp rooms = #{"Mira": 3, "Old Tom": 1, "Edda": 7}
~ temp doors = ""
~ {
    for guest in rooms {
        doors = doors + guest + " is in room " + rooms[guest] + ". "
    }
}
{doors}
-> END
```

```text
Mira is in room 3. Old Tom is in room 1. Edda is in room 7.
```

> **Landed on the native surface — `for k, v in m` (B2).** The ruled pair
> spelling is the two-binding loop: `for guest, room in rooms { … }`,
> defined as exactly the desugar you just wrote by hand — key iteration
> plus a `room = rooms[guest]` read at the top of each pass, total by
> construction, no pair value ever materializing. The ruling landed
> 2026-07-19 (with F10) and the two-binding form now parses and lowers
> on the native `.brink` parser (#1461). This chapter's `~ { … }`
> brink-dialect surface has no two-binding spelling yet, so `for k in m`
> + `m[k]` is still the pair story for the examples above.

## Counting with ranges

Ranges earned their chapter in [Collections](literals.md) as values;
here is where they earn their keep. `for i in a..b` walks the integers
ascending — and because an empty range is legal and iterates **zero
times**, the bounds never need guarding:

```ink
VAR doubled = ""

~ {
    temp tab = #[4, 7, 2]
    for i in 0..len(tab) {
        tab[i] = tab[i] * 2
    }
    doubled = string(tab)
}
Every debt doubles after midnight: {doubled}.
-> END
```

```text
Every debt doubles after midnight: [8, 14, 4].
```

That `for i in 0..len(a)` shape is doing two jobs worth naming:

- **It's safe on empty.** `len(tab)` of an empty array makes `0..0`,
  which runs zero times. Emptiness is load-bearing, not an edge case to
  guard — the same posture that makes `pick(0..n)` answer `none` rather
  than fault.
- **It's the mutation idiom.** The loop variable is a copy, but an index
  is a place — `tab[i] = …` writes through to the real array, one
  in-bounds claim at a time. (If the body *shrinks* the array while the
  range marches on, the out-of-bounds write faults exactly as an index
  claim should. The range was sized at entry; the array is live.)

Two mechanical facts, so you can plan around them: a range in a `for`
never materializes its integers — `for i in 0..1000000` walks bounds, it
does not allocate a million-element array — and a range only counts
*up*: `5..2` is empty, not a countdown. To walk backwards, count up and
index from the far end.

A dedicated mutating-iteration form (`for ref x in xs { … }`) is under
design in the stdlib spec — proposed, not ruled, not taught here. The
index idiom is the current answer, and an honest one.

## `while`, `break`, and `continue`

`for` is for "each of these"; **`while cond { … }`** is for "as long as
this holds" — re-testing its condition before every pass:

```ink
VAR pot = 0

~ {
    temp round = 1
    while round <= 5 {
        pot = pot + round
        round = round + 1
    }
}
Five rounds at the dice table and the pot holds {pot} coins.
-> END
```

```text
Five rounds at the dice table and the pot holds 15 coins.
```

Inside either loop, two statements steer: **`break`** leaves the
innermost enclosing loop immediately; **`continue`** abandons the rest of
the current pass and goes straight to the next test. The classic
combined shape:

```ink
VAR sum = 0

~ {
    temp i = 0
    while true {
        i = i + 1
        if i > 10 {
            break
        }
        if i mod 2 == 0 {
            continue
        }
        sum = sum + i
    }
}
The dice corner's odd throws come to {sum}.
-> END
```

```text
The dice corner's odd throws come to 25.
```

Both words are meaningful only inside a loop. Outside one there is
nothing to leave, and the compiler refuses rather than guessing — `E057`,
"break/continue outside a loop: `break` used outside any enclosing
while/for loop":

```ink,error(E057)
~ {
    break
}
-> END
```

In nested loops, `break`/`continue` always bind to the *innermost* loop;
there are no loop labels. If you need to leave two loops at once, the
clean spelling is usually a function and a `return` — which is also this
chapter's next idiom.

## The step budget, or why nothing hangs

`while true` with no `break` is a real program, and the runtime has a
ruled answer to it: **every turn runs under a step budget.** The VM
counts opcode steps; a turn that exceeds the limit (one million steps by
default — narrative-scale loops use a tiny fraction of it) stops with a
`StepLimitExceeded` error instead of freezing the player's game. A
sibling cap bounds runaway *output* — a turn that emits thousands of
lines without reaching a choice or an end hits the line limit (10,000
by default) the same way.

Know the category, because it is deliberately harsher than a fault. A
turn-terminating fault — an out-of-bounds index, a malformed question —
is a recorded, deterministic, replayable part of the story's history. A
**safety-limit error** is not: it aborts *mid-step*, the story is left
partway through a turn, and the host is told to treat the instance as
spent and restart from a snapshot (see [Errors](../reference/errors.md)).
The budget is not a semantic boundary you tune your story against; it is
the fence at the cliff edge.

The author's mental model, in two lines: **loops are for bounded,
narrative-scale data** — tabs, inventories, registers, grids — and every
loop should have its bound visible in its shape (`for` over a
collection is bounded by construction; a `while` should wear its exit
condition plainly). If a loop is doing so much work that the budget is
in sight, the story is simulating, not narrating, and the simulation
belongs on the host side of the seam.

## Finding things: loops that answer with Option

The [Option chapter](option.md) taught the fold-your-own idiom: when no
verb answers your exact question, the loop is four honest lines. Its
natural companion is the *find*-shaped loop — walk until something
qualifies, and answer like the stdlib verbs do: `some(hit)` or `none`,
never a sentinel. Wrap it in a function and let `return` be the early
exit:

```ink
First night over 5 coins: {first_over(#[4, 7, 2, 9], 5)}.
First night over 10: {string(first_over(#[4, 7, 2, 9], 10))}.
-> END

=== function first_over(tab: Array<int>, floor: int) ===
~ {
    for coins in tab {
        if coins > floor {
            return some(coins)
        }
    }
}
~ return none
```

```text
First night over 5 coins: some(7).
First night over 10: none.
```

The shape earns its keep three ways: the `return` inside the loop is the
cleanest multi-level exit the language has; the two returns hand
inference everything it needs (`first_over` settles as
`(Array<int>, int) -> Option<int>`, no annotation); and the caller gets
a value in the full absence doctrine — testable with `== none`, equal to
`some(7)` exactly when the world had one. When the pure verb trio lands
(below), many of these hand-rolled loops collapse into one-liners — the
doctrine they answer with will not change.

## Iteration across a pause — the current posture

Today, a loop always runs to completion within a single step of the
story, and nothing can interrupt it from the inside — the logic-block
fence keeps choices and diverts out of loop bodies, and the `await`
suspension surface is fenced off entirely (`E052`, "brink extension not
yet implemented") until the flow-suspension slice lands. There is no
compiling program in which a loop is half-done while the player thinks.
This section is posture, not practice — flagged honestly as such.

The design has already been settled *for* that future, though, and it
explains a choice you can otherwise only take on faith: the iterate
protocol is ruled **pull-shaped** (a `next(ref Self): Option<T>` step,
"every element exactly once; `none` is terminal and sticky") precisely
so that `for` desugars inline and an in-flight iteration can park inside
a suspended flow and resume after it wakes. Ranges became real,
serializable values (F7) for the same reason — a parked loop's cursor
has to survive a save. When suspension ships, loops will cross it
without this chapter changing its contracts.

## Planned — the pure verbs: `map`, `filter`, `fold`

The ruled iteration surface is larger than `for`, and its centerpiece —
the fn-value trio — is **ruled but not yet in the dialect**: no spelling
of `map`/`filter`/`fold` compiles today, which is why this section has
doctrine and no code fences. What is already settled:

- **The trio requires pure·silent callbacks.** `map` (transform each),
  `filter` (keep some), and `fold` (combine into one) accept only
  callbacks whose effect rows are pure and silent — reading story state
  is legal (filtering on state is the bread-and-butter case), writing,
  emitting, and drawing randomness are not. Faulting is permitted; a
  callback that can fault makes the whole call able to fault, rows
  composing as usual ([Effects](effects.md)).
- **"One logical pass, order unobservable."** Because callbacks are
  pure, whether stages interleave or fuse is unobservable *by
  construction* — the eager-versus-lazy question is dissolved, not
  deferred, and the implementation may fuse freely, forever.
- **Effectful iteration is a different concept with different
  spellings**: `each` (do something per element, no result) and
  `map_each` (the effectful transform — sequential, in iteration order,
  element by element, never fused). The standing naming law was ruled
  with them: **the weird thing gets the ugly method.** Convenience is
  spent on the pure spelling; the friction in `map_each` *is* the speed
  bump. The trio's rejection error will name both exits: make it pure,
  or say `map_each`.
- **`filter_map`** — the Option-aware mapper (callback returns
  `Option<U>`; `none`s drop) — is the ruled bridge to the
  [Option chapter](option.md), and the destined one-line form of the
  find-shaped loops above.

Until the verbs land, `for` is not a lesser substitute — it is the same
closed iterable set, the same snapshot contracts, and every trio-shaped
computation in this chapter's examples is a loop you already know how to
write.

## Reference: the diagnostics in this chapter

| Code | Fires when | Policy |
|---|---|---|
| `E051` | loop syntax (a `~ { … }` block) in a `strict-ink` project | dialect gate |
| `E052` | an `await` form (including `while await`) — suspension not yet implemented | both |
| `E054` | a loop variable or block `temp` shadows an already-visible temp | both (warning) |
| `E057` | `break`/`continue` outside any enclosing `while`/`for` | both |
| `E065` | a loop variable with no element type (e.g. iterating a non-iterable) escapes as `Unknown` | strict |

And the runtime's side of the line: iterating a value outside the closed
set is a turn-terminating "not indexable" fault in every mode; a
mid-loop faulting read (`m[k]` on a key removed after the snapshot,
`a[i]` past a shrunk array) is the ordinary indexing fault it always
was; and a loop that exhausts the step or line budget is a
safety-limit error — mid-step, instance spent, restart from a snapshot.

## Where this is ruled

- **The loop surface** (`while`/`for`/`break`/`continue` inside
  `~ { … }` blocks; the pure-logic fence; block scoping and the `E054`
  shadow warning; `E057`) — `docs/t1b-surface-spec.md` §2; the T1b
  landing (#577).
- **One closed iterable set; sequences & iteration as one design** —
  `docs/stdlib-spec.md` §4; decision log 2026-07-18.
- **The iterate protocol: pull-shaped, laws attached** ("every element
  exactly once; `none` terminal and sticky"; pull chosen so iterators
  park across suspensions) — `docs/stdlib-spec.md` §9.6; decision log
  2026-07-18 (protocol registry); NS-A3 (#1109).
- **F10: the map key set snapshots eagerly at loop entry** (maps' `for`
  is a deliberate exception to live pull; removed-key reads fault
  honestly) — decision log 2026-07-19 (Phase C findings ruling);
  `docs/stdlib-spec.md` §5. The `for k, v in m` desugar ruled with it;
  the native `.brink` surface landed with Track B2 (#1461); the
  `~ { … }` brink dialect this chapter teaches has no two-binding
  spelling yet.
- **F7/F8: ranges as a real Value kind; empty ranges iterate zero
  times; refinements inert under gradual** — decision log 2026-07-19;
  `docs/stdlib-spec.md` §7; NS-A5 (#1136).
- **The trio: pure-required; eager/lazy dissolved; `each`/`map_each`;
  "the weird thing gets the ugly method"** — decision log 2026-07-18;
  `docs/stdlib-spec.md` §4. Surface not yet landed — no compiling
  spelling today.
- **Mutating iteration `for ref x`** — `docs/stdlib-spec.md` §4,
  marked proposed (🔶): under design, not ruled, not taught.
- **The step budget** ("guard against unbounded growth"; safety-limit
  errors abort mid-step, restart from snapshot) — the runtime's standing
  posture; see [Errors](../reference/errors.md) and
  [Speculation](../embedding/speculation.md) for the host-facing view.
