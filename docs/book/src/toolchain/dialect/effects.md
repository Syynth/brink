# Effects

Every knot and stitch carries an **inferred effect row**: a static,
conservative summary of what it — and everything it transitively calls —
may touch. Three sets: which global cells it may **read**, which it may
**write**, and which `EXTERNAL` functions it may **call**. Nothing you write
produces this. It falls out of the same call-graph walk that infers your
unannotated types, and it's always there, on every def, whether or not you
ever look at it.

```ink
VAR gold = 100

Starting gold: {gold}.
~ temp remaining = spend(30)
Remaining gold: {remaining}.
-> END

=== function spend(cost) ===
~ gold = gold - cost
~ return gold
```

```text
Starting gold: 100.
Remaining gold: 70.
```

`spend` reads `gold` and writes `gold`. That's its row — you never wrote
anything to produce it, and nothing above prints it (rows aren't ink-visible
data; they're read through [hover and `brink ide`](#editor--ci-tooling)).
This chapter is about a row's shape, the one assertion you can write against
it, and the tooling that surfaces it.

## Reading a row

A row has exactly three atom kinds:

- **reads** — global `VAR`/`CONST` cells the def's body (or anything it
  calls) may read.
- **writes** — global cells it may write.
- **calls** — `EXTERNAL` binding *names* it may transitively call. (Ordinary
  knot/stitch calls aren't atoms themselves — see [Rows
  compose](#rows-compose) below.)

A def that touches none of these — no global reads, no writes, no external
calls — is **pure**. `double(x) = x * 2` is pure; there's no cell or
external in sight.

## Rows compose

A row isn't just its own body's atoms — a direct call to another
knot/stitch pulls in *that def's whole row*, transitively, however deep the
call chain runs:

```ink
VAR hp = 10

~ temp result = heal(5)
Result: {result}.
-> END

=== function heal(amount) ===
~ hp = hp + amount
~ return log(hp)

=== function log(value) ===
~ return value
```

```text
Result: 15.
```

`log` itself touches nothing — its row is pure. But `heal` calls `log`, so
`heal`'s row is `{reads: hp, writes: hp}` from its own body, the same as if
`log` weren't there at all. Recursive and mutually-recursive call cycles
fold the same way: every def in the cycle ends up with the join of every
atom any of them touches, computed by a fixpoint over the finite universe of
cells and call-kinds (it always terminates — there's nothing to grow into
forever).

## Conservative-total: rows never under-report

A row is an upper bound. It may claim more than a def's body strictly needs
on some execution paths (the `int → float` join in the [Types
chapter](./types.md) has a cousin here: over-approximation isn't a bug, it's
the contract), but it may **never** claim less than what could actually
happen. Two consequences follow directly from that one rule:

- **A call through a function value is unbounded.** `#fn`/`bind` — see [Function
  Values](./function-values.md) — let a call's target vary at runtime; a row
  can't see through that indirection statically, so a def that calls through
  one gets the pessimal **opaque** row: "may touch anything." This is sound
  (an opaque row can never under-claim), just imprecise — a future
  refinement may narrow it by reading a row back off the function value's
  own type, but nothing about today's compiler ever reports *less* than
  opaque for an indirect call.
- **An unresolved callee is opaque too**, for the same reason — no row to
  join in means no floor to trust below "anything."

```ink
~ temp adder = #fn(add, 10)
~ temp total = apply(adder, 7)
Total: {total}.
-> END

=== function add(a, b) ===
~ return a + b

=== function apply(f, x) ===
~ return f(x)
```

```text
Total: 17.
```

`apply` calls through its `f` parameter — a function value, resolved only at
runtime — so `apply`'s row is opaque, not `add`'s actual (empty) row. The
program runs exactly the same either way; only the *row* pays the
imprecision cost, not the behavior.

## Asserting a bound: `#@effects(…)`

An inferred row is advisory by default — nothing enforces it. `#@effects(…)`
turns it into a checked upper bound: a compile-time assertion that a
def's *inferred* row is covered by what you declared. Placement mirrors
`#@local` — the top of the knot/stitch body, right after the header:

```ink
VAR gold = 100

Starting gold: {gold}.
~ temp remaining = spend(30)
Remaining gold: {remaining}.
-> END

=== function spend(cost) ===
#@effects(reads: gold, writes: gold)
~ gold = gold - cost
~ return gold
```

```text
Starting gold: 100.
Remaining gold: 70.
```

Clauses are `reads:`, `writes:`, `calls:` — comma-separated identifiers,
any subset, any order. `#@effects(pure)` sugars the empty row for a
genuinely pure def:

```ink
Doubled: {double(21)}.
-> END

=== function double(x) ===
#@effects(pure)
~ return x * 2
```

```text
Doubled: 42.
```

**Over-declaring never warns.** There is no drift policy in either
direction: a bound strictly wider than what's actually inferred stays
completely silent, on purpose (spec §10, sitting 2, 2026-07-14). The *only*
diagnostic this surface produces is **exceedance** (`E103`) — the inferred
row is *not* covered by the declared bound. Declaring `#@effects(pure)` on
`spend` above (which actually reads and writes `gold`) is a compile error
naming exactly what escaped: ``inferred effects exceed the `#@effects`
assertion's declared bound: reads gold; writes gold``. An opaque inferred row
(a call through a function value, or an unresolved callee) can never be
bounded by any concrete assertion — the message says so explicitly rather
than listing atoms that don't exist to list.

A clause naming something that isn't a declared global cell (`reads`/
`writes`) or a declared `EXTERNAL` (`calls`) anywhere in the project is a
separate, ordinary well-formedness error — the assertion can't even be
built into a row without a real name behind every clause. `#@effects` is
brink-dialect-gated the same way `#@module`/`#@private` are: writing one
under `strict-ink` is rejected outright, not silently ignored.

## Default-public, and the door `#@private` closes

Every knot/stitch ships its row by default — there's no `#@entry` marker to
opt in, because every knot is already a valid [host entry
point](../reference/containers.md) (`choose_path_string`, or an editor's
play-from-here). `#@private` (the same directive
[Modules](./modules.md) already documents) is the opt-out: a private
definition's row is internal, and a host attempting to resolve it as an
entry point fails at load. Its full visibility semantics live in the
Modules chapter — effects rows are just one more thing "public" and
"private" mean something for.

## Editor & CI tooling

Rows are metadata, not ink-visible output, so two tools exist specifically
to surface them:

- **Hover** shows a knot/stitch's inferred row as one line — `reads …;
  writes …; calls …`, or `pure`, or `opaque` — right alongside its
  signature and docs.
- **`brink ide effects-diff`** diffs inferred rows between two git
  revisions (or a revision and the working tree), a CI-comment-friendly
  report of exactly what changed:

  ```sh
  brink ide effects-diff -e main.ink --base origin/main       # working tree vs. a branch
  brink ide effects-diff -e main.ink --base HEAD~1 --head HEAD --format json
  ```

  This is the sitting-2 ruling's answer to "what about drift?" — there is
  no lockfile and no pin artifact (inference is deterministic from source;
  the shipped `.inkb` rows already are the frozen record), so drift has
  nothing to reproduce *against*. What's left is pure **visibility**:
  tooling that shows a human what moved, never a gate that blocks a build
  over it. See [`brink ide`](../cli/ide.md#effects-diff--row-drift-between-two-revisions)
  for the full command reference.
