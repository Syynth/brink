# Effects

Every knot, stitch, and function in a brink-dialect project has an **effect
row** — a static summary of what it touches when it runs: the world cells it
reads, the world cells it writes, and the engine functions it calls. You never
write a row; the compiler infers it by looking at the body. Rows exist for one
reason: a host engine has to make decisions *before* it runs your story —
which flows can advance in parallel, what to prefetch, when a sleeping flow
should wake — and the only thing it can know before running is what a row
tells it.

This chapter is about the *authoring* side of that: what a row is, how it's
inferred, where its boundaries are, and the one place you can pin one down with
an assertion. The host side — how bevy-brink actually schedules on these rows —
lives in the [Bevy integration](../../integrations/bevy/index.md).

## The three layers

Effects are described at three layers, and they are never conflated:

- **Atomic effects** are what *expressions* emit when they run: *read this
  cell*, *write this cell*, *call this external*. Data never has effects — code
  does. `gold` is just a number; `~ gold = gold - 1` is a read and a write.
- **Rows** are the static summary: `{reads, writes, calls}` as **unordered
  sets**. Ordering is the journal's concern, not the row's. Every atom an
  expression emits is absorbed into the *enclosing* definition's row.
- **Types** carry rows only in one place: a function *value*. `fn(int): int`
  can carry `⟨reads: gold⟩`, meaning *calling it* reads `gold`. A collection of
  function values carries rows through its element type. Reading the collection
  is still pure — holding a piece of pending computation is not the same as
  performing it (see [Function Values](function-values.md)).

## Rows are inferred

A definition's row is built exactly the way its type is: walk the body, collect
the atoms, take the union. A **pure** function touches nothing, and its row is
empty:

```ink
-> start

=== function double(n) ===
~ return n * 2

=== start ===
Twice three is {double(3)}.
-> END
```

```text
Twice three is 6.
```

Reading and writing a global `VAR` puts that cell in the `reads` and `writes`
sets:

```ink
VAR gold = 10
-> shop

=== shop ===
~ gold = gold - 3
You have {gold} gold.
-> END
```

```text
You have 7 gold.
```

A **direct call** pulls in the callee's whole row — transitively. If `outer`
calls `inner`, and `inner` writes `gold`, then `outer` writes `gold` too, even
though `outer`'s own body never mentions it. Mutual recursion is handled by the
same per-SCC fixpoint the type checker already runs, so a cycle of functions
all converge on the union of everything the cycle touches. Nothing special is
needed from you; it falls out of following the call graph.

There is one place inference has to give up precision: a call *through a
function value*. When you dispatch through a `#fn` value — `~ temp x = f()` — the
compiler generally can't see which concrete function `f` holds at that moment,
so the row becomes **opaque**: the conservative "touches everything" row. That
is always sound (it can never *under*-report), just coarse. Concrete functions
called directly stay fully precise; only the indirect hop widens.

## Boundaries: what ships in a row

A shipped row contains only what a *host* can act on:

- **World cells** — global `VAR`/`CONST` — one entry per cell.
- **External call kinds** — the engine functions the definition calls.

Everything else is deliberately excluded. A `~ temp` dies inside the frame and
a `#@local` cell is flow-private by construction — neither can matter to a
scheduler, so neither appears in a shipped row. (Internal inference keeps full
per-cell precision regardless; the trimming is only at the boundary.)

Every knot and stitch ships a row — there is no `#@entry` marker, because
"play from here" already makes any knot a possible host entry point. If you
want a definition to *not* be a host entry, `#@private` opts it out: its row
stays internal and a host lookup for it fails at load. (The full visibility
story is the [Modules](modules.md) round; effects just ride on it.)

## Soundness: over-report, never under-report

The one rule a row must obey is directional: it may claim **more** than the
code does, never less. An over-report costs a missed parallelization or a
spurious wakeup — wasteful, but safe. An under-report would let the host run two
flows concurrently that actually race on the same cell — a real bug. So when
inference is unsure, it widens (that opaque row above), and "no answer" is never
an option: the pessimal touches-everything row is always available and always
sound.

A practical corollary: **strict mode buys scheduler precision.** The more your
types resolve, the tighter your rows, the more the host can overlap. Gradual
`Unknown`s widen rows the same way they defer type checks.

## The `@[effects]` assertion

Rows are inferred, deterministic, and shipped in the compiled `.inkb` — so
there is nothing for a lockfile to pin, and brink has none. The *only* contract
you can write is an optional inline upper bound: an `@[effects(…)]` annotation
line at the top of a knot or stitch body.

> The older `#@effects(reads: …)` tag spelling is a frozen, deprecated alias —
> it still compiles but warns (`E110`). New code writes the annotation form
> below; clauses are parenthesized (`reads(gold)`, never `reads: gold`).

```ink
VAR gold = 10
-> shop

=== shop ===
@[effects(reads(gold), writes(gold))]
~ gold = gold - 3
You have {gold} gold.
-> END
```

```text
You have 7 gold.
```

The clauses name world cells (`reads(…)`, `writes(…)`) and externals
(`calls(…)`). `@[effects(pure)]` is sugar for the empty row — the "this stays pure, hold me to
it" case:

```ink
-> greet

=== greet ===
@[effects(pure)]
Hello, traveler.
-> END
```

```text
Hello, traveler.
```

An assertion is an **upper bound**, and the only thing it can do is fail: if the
inferred row is *not covered by* what you declared — the body reads, writes, or
calls something the assertion didn't list — that's a compile error (`E103`,
"inferred effects exceed the declared bound"):

```ink,error(E103)
VAR gold = 0
EXTERNAL play_sfx(x)
-> shop

=== shop ===
@[effects(pure)]
~ gold = gold + 1
~ play_sfx(1)
Spent.
-> END
```

Declaring a bound *wider* than the inferred row is silent — there is no drift
policy, because there is nothing to drift against. Over-declaring never warns;
only exceedance errors. The assertion is a tripwire you set deliberately (a
function you promise to keep pure, a knot whose world footprint you want frozen),
not a running commentary on your code.

The directive is **runtime-inert** — advisory metadata, checked at compile time
and then invisible. A program with a satisfied `@[effects]` bound produces the
exact same output as the same program without it.

## Seeing your rows

Two tools surface inferred rows so you don't have to guess.

**Hover** over a knot or stitch (in the editor, or `brink ide hover NAME -e
main.ink`) shows its effect row on a stable line — `reads: …; writes: …; calls:
…`, or `pure`, or `opaque` for a definition that dispatches through a function
value.

**`brink ide effects-diff`** compares every row against a baseline — a git
revision (`--rev HEAD` for working-tree-vs-HEAD) or a second entry file
(`--base`) — and prints a CI-comment-friendly Markdown summary of what moved:

```sh
brink ide effects-diff --rev HEAD -e main.ink
```

This is drift *visibility*, not a gate: it shows what your edit did to the
shipped rows. Add `--exit-code` to make it fail a CI check when any row changed.
See [The CLI › `effects-diff`](../cli/ide.md#effects-diff--how-a-change-moved-the-inferred-effect-rows)
for the full flag set and JSON shape.

## What effects are *not*

Interior effects are always inferred, never spelled. There is no monad, no
effect handler, no function coloring, no `async`/`await`-style annotation
creeping through your call sites — the entry-point row and the optional
`@[effects]` bound are the whole author-facing surface. Everything else about
effects — parallel scheduling, prefetch, reactive sleep, the capability
manifest that maps `calls` to engine components — is the host's job, and lives
in the [Bevy integration](../../integrations/bevy/index.md).
