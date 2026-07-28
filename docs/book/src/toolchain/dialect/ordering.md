# Ordering, Sorting, and Comparing

Closing the ledger at the Last Light, three questions get asked of the
same night's numbers — put them in order, put them in *my* order, and
name the extremes:

```ink
~ temp tab = #[4, 7, 2, 5]
The night's takings, in the order they landed: {tab}.
For the magistrate's fair copy: {sorted(tab)}.
~ sort_by(tab, #fn(largest_first))
Calling in debts from the top: {tab}.
Lightest night: {min(tab)}. Heaviest: {max(tab)}.
-> END

=== function largest_first(a: int, b: int): int ===
~ return b - a
```

```text
The night's takings, in the order they landed: [4, 7, 2, 5].
For the magistrate's fair copy: [2, 4, 5, 7].
Calling in debts from the top: [7, 5, 4, 2].
Lightest night: some(2). Heaviest: some(7).
```

The [Collections](literals.md) chapter introduced these verbs as part of
the working surface; this chapter is the doctrine underneath them. One
pinned order serves every ordering verb in the language — the sort
family, `min`/`max`, and the heap verbs — and one line of doctrine
governs its only hard case: **dev mode changes where execution stops,
never what values appear.** What follows is the roster of what orders,
the contract your comparators owe, the NaN story in full, the priority
queue in the corner, and the deliberate gap between ordering and
equality.

> **Current spelling** — examples in this chapter compile in today's
> brink dialect: collection literals carry the `#[…]` sigil, the verbs
> are free calls (`sort(tab)`, `heap_push(open, 3)`), and a comparator is
> a function value spelled `#fn(name)` over a declared function (see
> [Function Values](function-values.md)). The ruled native spellings —
> method position (`tab.sort()`, with UFCS auto-`ref` on the receiver)
> and lambda comparators (`tab.sort_by(|a, b| b - a)`) — arrive with the
> native frontend; the semantics taught here do not change with them.

## What orders

The ordering verbs share one **doctrine order**, a single total order
over a closed roster of types:

| Type | Order |
|---|---|
| `int`, `float` | numeric, cross-comparing — `1 < 1.5 < 2` in one array |
| `bool` | `false < true` |
| `string` | lexicographic by Unicode scalar value |
| arrays | lexicographic element-wise, recursively — first differing element decides; a full prefix ties to the shorter array |

```ink
~ temp mixed = #[2, 1.5, 1]
~ sort(mixed)
mixed numerics: {mixed}.
~ temp words = #["pear", "apple", "fig"]
words: {sorted(words)}.
~ temp nested = #[#[2], #[1, 5], #[1]]
lex: {sorted(nested)}.
-> END
```

```text
mixed numerics: [1, 1.5, 2].
words: [apple, fig, pear].
lex: [[1], [1, 5], [2]].
```

Two of those rows carry footnotes worth knowing. String order is by
Unicode scalar value — a deterministic, locale-free order that puts
`"Zebra"` before `"apple"`; proper locale-aware collation is the intl
pipeline's business, like casing. And the array row is recursive with a
depth cap: an array nested past 64 levels faults rather than chase a
pathological self-referential structure down the stack.

Everything else is **not orderable**, and asking is a malformed question
in the [Option chapter's](option.md) sense — a turn-terminating fault
("`sort` cannot order element of type map"), never a shrug and never a
guessed order:

- **maps** — no defined order between entries;
- **`Option<T>`** — `some`/`none` don't order (ch. 14 drew this line);
- **flags subsets** — a partial order, deliberately not forced total;
- **divert targets**, function values, ranges, `Weighted<T>` tables;
- **the numeric tower** (`vec2`…`mat4`) — vectors have no one honest
  lexicographic order, so tower kinds are not orderable, and the
  compiler refuses even a `compare` protocol registration for them
  (`E118`);
- **structs and enums — today.** The ruled path for records is an
  explicit `compare` protocol impl (`fn(T, T): int`, and nothing else:
  field declaration order is never silently promoted to ordering
  semantics). The impl-block spelling hasn't reached the dialect yet, so
  until it lands, sorting records is simply a fault.

A cross-type pair of individually-orderable elements (`#[1, "x"]`) is
just as malformed — the doctrine order crosses `int`/`float` and stops
there. The fault names the offending element's type, and in an extremum
or sort walk it fires the moment the pair is compared.

The doctrine order's smallest consumers are `min` and `max`: least and
greatest element by exactly this order, `none` on an empty array (the
[Option chapter's](option.md) absence doctrine — an empty extremum is
an honest answer, not a bug), and on ties they keep the *first*
occurrence, deterministically. `min(a)` always agrees with
`first(sorted(a))`, by construction — one order, every verb.

## The four sort verbs

The sort family is four verbs on a two-way grid, and the grid is the
[naming law](literals.md#the-verb-surface) doing its job — the verb
carries the mutation signal:

| | doctrine order | your comparator |
|---|---|---|
| **imperative, in-place** | `sort(a)` | `sort_by(a, cmp)` |
| **past-participle, functional** | `sorted(a): [T]` | `sorted_by(a, cmp): [T]` |

The imperative pair mutates in place and returns nothing, so it is
**statement-only** and its receiver must be an lvalue — the confusion
lattice from Collections closes over the whole family. Using one as an
expression fails with `E056`, "`sort` mutates its first argument and
returns nothing — it can only be used as a statement, not an
expression":

```ink,error(E056)
~ temp tab = #[3, 1, 2]
~ temp tidy = sort(tab)
{tidy}
-> END
```

The fix is one edit in either direction: `sorted(tab)` if you wanted a
copy, or `sort(tab)` on its own line if you wanted the ledger itself
reordered. Handing the imperative form a temporary (`sort(#[3, 1])`) is
the familiar `E055` lost-write refusal, and a wrong argument count is
`E058`, naming the expected signature (`sort(array)`,
`sort_by(array, comparator)`).

Two properties hold across all four verbs, in every mode, forever:

**Sorting is stable.** Equal elements keep their input order. Sort the
tab by amount and two 4-coin nights stay in chronological order — which
is what makes sorting by one *aspect* of a value trustworthy:

```ink
~ temp stock = #["cider", "ale", "bread", "gin"]
~ sort_by(stock, #fn(shortest_first))
Shelved by label width: {stock}.
-> END

=== function shortest_first(a: string, b: string): int ===
~ return len(a) - len(b)
```

```text
Shelved by label width: [ale, gin, cider, bread].
```

`ale` and `gin` tie at three characters and keep their shelf order;
`cider` and `bread` tie at five and do the same. A second sort by a
second aspect refines the first instead of scrambling it.

**Sort never implies dedup.** A sort with ties drops nothing — ordering
and equality are separate questions (the last section returns to this),
and no ordering verb ever removes, merges, or invents an element. The
guarantee floor, straight from the ruling: the result is *some
permutation of the input, never worse* — even a comparator that faults
mid-sort leaves nothing fabricated behind it.

## Comparators are a contract

`sort_by`/`sorted_by` hand ordering to your function, and the shape of
that function is ruled: **`fn(T, T): int`** — negative means "a first",
zero means "tie", positive means "b first". Any int does; `b - a` is the
classic descending one-liner. A comparator must also be:

- **Pure and silent.** No writes, no content output, no tags, no
  randomness — and **no reads of story state either**. The order must be
  a function of the two comparands and nothing else, because how many
  times the sort calls your comparator, and in what order, is an
  implementation detail you can't observe on a clean comparator and
  must never be able to observe on a dirty one.
- **A consistent total order.** If `cmp(a, b)` says `a` first, `cmp(b,
  a)` must say `b` first, and ties must behave like ties. The
  implementation is permitted to fault on a detected inconsistency; what
  it will never do is loop or lose data (the permutation floor above).
- **Allowed to fault.** Purity here is deliberately not totality — a
  comparator that faults on bad data is honest, and its fault ends the
  turn like any other.

Where the compiler can *prove* a violation — the comparator is a named
`#fn(target)` whose inferred effect row shows a read, write, emission,
or external call — it refuses at compile time with `E119`. This fails
with "`sort_by`'s comparator `luckiest_first` reads lucky — a comparator
must be a pure, silent `fn(T, T): int` (stdlib-spec §4b: the order must
depend only on the two comparands)":

```ink,error(E119)
VAR lucky = 4

~ temp dice = #[3, 6, 2]
~ sort_by(dice, #fn(luckiest_first))
{dice}
-> END

=== function luckiest_first(a: int, b: int): int ===
~ return (a - lucky) * (a - lucky) - (b - lucky) * (b - lucky)
```

Sorting by distance-from-a-favorite is a perfectly good idea — but this
spelling makes the order depend on `lucky`, which is exactly what the
contract bans. Today's honest fix is to restructure so the sort key is
in the data (build the array of distances and sort that); when lambdas
land, capturing `lucky` *by value* into the comparator will be the
one-line spelling of the same fix.

The check is exceedance-only: it fires on what it can prove, never on
what it can't. A comparator that arrives as an opaque value — a
variable, a parameter, a `bind(…)` result — passes the gate, and the
**runtime** holds the residual line instead. During the sort, each
comparator call runs isolated: anything it prints is captured and
discarded, and misbehavior the VM can observe is a turn-terminating
fault with a taxonomy of its own:

| Fault | Fires when |
|---|---|
| `ComparatorNotAFunction` | the second argument isn't a function value — "`sort_by` comparator must be a function value `fn(T, T): int`, got int" |
| `ComparatorReturnType` | the comparator returned a non-int — "got string", never a silent coercion |
| `ComparatorEscaped` | the comparator presented a choice, reached `-> DONE`/`-> END`, called an external function, exceeded the nested evaluation step budget, or recursed past the nesting depth limit — "comparators must be pure, silent functions" |

The budgets behind that last row exist so a divergent comparator can
never hang the story: each comparator call runs under its own
million-step budget inside the sort's single VM step, and
comparator-inside-comparator nesting (a comparator that itself sorts)
is capped at depth 8. Two honest footnotes on the current machinery:
calling a comparator counts a visit on that function, exactly like any
other in-story function call; and a contract violation that slips both
the static gate and the VM's observation — an opaque comparator that
writes a global, say — is not currently intercepted, so its writes
really happen, at an unspecified number of invocations in an
unspecified order. That is a contract violation, not a technique;
whether the runtime should grow its own write-guard is an open design
question, on the docket, not ruled.

One more thing `sort_by` deliberately does *not* do — see the next
section for why that's interesting.

## NaN, dev, and prod

Floats bring one lawless guest to every ordering table: `NaN`, which
IEEE comparison refuses to place. The language's arithmetic is
**NaN-total** — `0.0 / 0.0` is `NaN`, and it flows through `+`, `*`,
and every math verb without a fault, by ruling. Ordering contexts are
where the flow stops, and what happens there is the doctrine this
chapter is named for.

**Dev mode — the default — faults, loudly.** A NaN comparand reaching
`sort`/`sorted`/`min`/`max`/`heap_push` is a turn-terminating fault:

> `` `sort` reached a NaN comparand — NaN cannot be ordered (dev-mode
> fault; prod mode places NaN by the pinned total order)``

The point is bug archaeology: the NaN was *born* upstream, in some
arithmetic that went wrong, and dev mode surfaces it at its first
ordering consumption — while you can still find the cause. The check is
about the operand, not the comparison: `min` of the one-element array
`#[0.0 / 0.0]` faults in dev even though there is nothing to compare
it with, because the NaN in an ordering context *is* the bug. The scan
is recursive — a NaN inside a nested array is found the same way.

**Prod mode keeps moving.** The same NaN is *placed* by the pinned
total order: ordinary IEEE order where IEEE has an answer, `-0` tying
with `+0`, NaN greater than everything, NaN tying with NaN. Sorting
`#[1.0, 0.0 / 0.0, -1.0]` in prod yields `[-1, 1, NaN]` — every
element preserved, deterministically placed, save- and replay-safe.
(This is deliberately *not* IEEE's `totalOrder`, which splits `-0` from
`+0` and would make sorting disagree with `==` on perfectly clean
data.)

On NaN-free data the two modes agree exactly, and both cohere with `<`
and `==` — flipping a clean story to prod changes nothing. That's the
author-level line to carry: **the mode changes where execution stops,
never what values appear.** No mode fabricates, coerces, or drops
anything; dev halts at the bug, prod files it at the end of the array.

And the split is **fenced** to exactly this kind of case. The dev/prod
knob exists only where the prod behavior is defined, total, and
fabricates no data. Placement qualifies. Fabrication never does:
`int("potato")`, an out-of-bounds index, a malformed question — those
fault in every mode, forever, and no future knob will soften them.
There will never be a "prod mode" that invents a value to keep the
story moving.

Three practical corollaries:

- **`sort_by` is not on the dev fault list** — deliberately. Your
  comparator owns the element semantics; a NaN never reaches the
  ordering machinery as a comparison result, so `sort_by` faults only
  on its own terms (dispatch, return type, whatever the comparator's
  body does). If your comparator wants NaN hygiene, it implements it.
- **Effect rows don't know modes exist.** An ordering verb over
  `[float]` carries the fault possibility in its row unconditionally —
  the conservative union — while `[int]`/`[string]`/`[bool]` orderings
  are provably total and their fault charge is discharged. A
  totality-gated position (a wake condition, say) therefore rejects a
  float ordering in *both* modes, which is correct: a NaN-able wake
  condition is a landmine regardless of build profile.
- **The knob is a host/build setting, not story state.** The runtime
  API is `Story::set_exec_mode` / `FlowInstance::set_exec_mode` with
  `ExecMode::Dev` as the default (see
  [Runtime API](../reference/runtime-api.md)); a shipping host flips to
  `ExecMode::Prod` for release builds. The ruled home for the setting
  is project configuration (a `brink.toml` profile) with the host API
  as override — the config side isn't wired yet, so today the host API
  is the whole surface. The mode is never embedded in compiled
  `.inkb`, never persisted in a save, and never consulted by the
  checker. (What the *default* should be for engine integrations like
  bevy is a pending question on the docket — not ruled, so not taught.)

## The heap in the corner

The dice corner keeps a queue: whoever's owed the smallest debt gets
paid first. The language's priority queue is deliberately humble —
**three verbs over an ordinary array**, not a new type:

| Verb | Signature | Notes |
|---|---|---|
| `heap_push` | `heap_push(a: [T], x: T)` | statement-only mutator; add `x`, restore the invariant |
| `heap_pop` | `heap_pop(a: [T]): Option<T>` | remove and return the minimum; `none` on empty |
| `heap_peek` | `heap_peek(a: [T]): Option<T>` | read the minimum without removing; `none` on empty |

The heap is a **min-heap over the doctrine order** — the same
comparison core as `sort` and `min`, one order for the whole language —
and its contract is the invariant: an array built through `heap_push`
always pops in ascending order, no matter how the pushes interleaved:

```ink
~ temp open = #[5, 9, 8]
~ heap_push(open, 3)
~ heap_push(open, 7)
peek: {heap_peek(open)}
~ temp first = heap_pop(open)
~ temp second = heap_pop(open)
popped: {first} then {second}
drain: {heap_pop(open)} {heap_pop(open)} {heap_pop(open)} {string(heap_pop(open))}
-> END
```

```text
peek: some(3)
popped: some(3) then some(5)
drain: some(7) some(8) some(9) none
```

Everything in that transcript is doctrine you've already met. The pops
come back as `Option<T>` because an empty heap is *absence*, not a bug
— the [Option chapter's](option.md) line verbatim, and the final `none`
is the drain loop's natural stopping signal (spelled `string(…)` on that
last pop only, so the signal stays *visible* on the page — a bare
`{heap_pop(open)}` on the exhausted heap would print nothing at all,
per [How Option prints](option.md#how-option-prints)). `heap_push` is a
statement-only, lvalue-first mutator (`E055`/`E056`/`E058`, exactly the
`sort` family's manners); `heap_pop` is a hybrid like `pop` — it
mutates *and* answers, so it's legal in expression position, with the
same bare-variable receiver rule. And the §4b NaN doctrine applies **at
the door**: `heap_push` checks the entering element (dev faults on a
NaN anywhere inside it; prod places it by the pinned order), and
because every element arrived through that check, a clean heap stays
clean by induction — `heap_pop` and `heap_peek` never re-scan.

The bounded-priority-queue loop, today's spelling:

```ink
~ temp bell: Array<int> = #[]
~ heap_push(bell, 12)
~ heap_push(bell, 3)
~ heap_push(bell, 7)
~ heap_push(bell, 1)
~ temp calls = ""
~ {
    while len(bell) > 0 {
        temp room = heap_pop(bell)
        calls = calls + " " + string(room)
    }
}
Rooms answered in order:{calls}.
-> END
```

```text
Rooms answered in order: some(1) some(3) some(7) some(12).
```

(The `some(…)` in the prose is the total Option render from ch. 14,
kept honest on purpose; the ruled `as`-binding will make
`while heap_pop(…) as room { … }` the clean drain loop when it lands.)

The humility of the design has one sharp edge to respect: **the array
is just an array.** Nothing marks it as a heap, nothing stops you
indexing it, printing it, or sorting it — and nothing *verifies* it.
The verbs maintain the invariant over arrays built through them;
feed `heap_pop` an arbitrary array and it will treat element `0` as
the minimum and dutifully re-sift, garbage in, garbage out. The middle
of a heap array is *not* sorted — only the root is special — so read
it only through `heap_peek`/`heap_pop`, and build it only through
`heap_push` (starting from empty, or from an array you know satisfies
the invariant). If real projects show this shape-confusion biting, a
sealed `Heap<T>` type is the recorded upgrade path — designed, not
built, waiting on evidence.

One neighbor deliberately *not* in this chapter: the dice corner's
weighted table. `Weighted<T>` and `roll` live with the randomness
story — a draw writes the RNG cell, and the table's
evidence-by-construction contract belongs beside seeds and replay in
the Randomness chapter. The heap lives here because ordering is its
engine; `Weighted` merely lives *near* it in `std::collections`.

## Ordering is not equality

The last piece of the doctrine is a deliberate separation. The
language has two comparison surfaces, and they answer different
questions:

**The operators stay frozen IEEE.** `<`, `<=`, `>`, `>=`, `==`, `!=`
on floats behave exactly as ink and IEEE defined them: `NaN < x` is
false, `NaN > x` is false, and `NaN == NaN` is false — NaN is not
equal to itself, and no operator ever consults the pinned total order:

```ink
~ temp broken = 0.0 / 0.0
The chalk reads {broken}. Equal to itself: {broken == broken}. Less than one: {broken < 1.0}.
-> END
```

```text
The chalk reads NaN. Equal to itself: false. Less than one: false.
```

Only the *verbs* carry the ordering doctrine — the third application
of the language's standing two-surface pattern (operators keep their
inherited, oracle-guarded meaning; verbs carry the ruled semantics).
The practical reading: `x != x` remains the honest NaN test, an
`if a < b` in your own code never faults and never uses the pinned
order, and the doctrine only engages when you hand data to an
ordering verb.

**Compare and equality are allowed to disagree.** When the `compare`
protocol opens to user types, it is ruled as *ordering only*: equality
stays structural, always, and a `compare` impl that calls two values a
tie does not make them `==`. Sort the guest ledger by family name and
two Coopers tie for placement while remaining distinct guests — which
is also why sort never dedups: ties are an ordering fact, not an
identity claim. (The protocol's contract is pure·silent·**total** —
stricter than a `sort_by` comparator, which may fault — because a
registered order speaks for the type everywhere, sight unseen.)

## Reference: the diagnostics in this chapter

| Code | Fires when | Policy |
|---|---|---|
| `E055` | an ordering mutator's first argument isn't an lvalue (`sort(#[2, 1])`, `heap_push(sorted(a), x)`) | both |
| `E056` | `sort`/`sort_by`/`heap_push` used in expression position — they return nothing | both |
| `E058` | wrong argument count on `sort`/`sort_by`/`heap_push` — names the expected signature | both |
| `E118` | a protocol impl registration names a numeric-tower kind — tower kinds are compiler-known and not orderable (registration is a programmatic surface today; no source spelling reaches it) | both |
| `E119` | a provably impure/unsilent `#fn` comparator on `sort_by`/`sorted_by` — exceedance-only | both |

And the runtime's side of the line, all turn-terminating faults:
`NotOrderable` (an element outside the roster, or a cross-type pair);
`UnorderedComparand` (dev mode only — a NaN comparand at
`sort`/`sorted`/`min`/`max`/`heap_push`); the comparator taxonomy
(`ComparatorNotAFunction`, `ComparatorReturnType`,
`ComparatorEscaped`); and the ordinary wrong-container fault (`sort`
on a map, `heap_pop` on an int). `min`/`max`/`heap_pop`/`heap_peek` on
an *empty array* are none of these — that's absence, and it answers
`none`.

## Where this is ruled

- **The ordering doctrine** (the pinned total order and its roster; the
  dev-fault/prod-placement split; the fence — "placement qualifies;
  fabrication never does"; mode-independent rows; operators frozen
  IEEE; the comparator law) — `docs/stdlib-spec.md` §4b; decision log
  2026-07-18 ("The ordering doctrine: NaN faults at ordering contexts
  in dev, pinned placement in prod"). Implementation NS-A4 (#1110,
  PR #1149).
- **F0: the four-verb grid** — `sort_by` in-place, `sorted_by` the
  functional twin, per the naming law — decision log 2026-07-19
  ("Findings batch 1 ruled").
- **F14: `sort_by` off the dev NaN-fault list** (the comparator owns
  the element semantics) — `docs/stdlib-spec.md` §4b, as-built
  amendment dated 2026-07-19 with NS-A4.
- **The knob's home** (project config profile + host-API override;
  today the host API is the implemented leg) and **compare/equality
  coherence** (compare is ordering only; sort never implies dedup) —
  decision log 2026-07-19 ("Tower mini-spec ruled (T1-T5)…; knob home;
  compare coherence"). Tower kinds not orderable: tower-mini-spec T4;
  E118 with NS-A8 (#1114).
- **The comparator contract** (pure·silent — reads included — plus the
  consistent-total-order law; E119 exceedance-only; the runtime
  residual taxonomy) — `docs/stdlib-spec.md` §4b; NS-A4 (#1110). The
  possible runtime write-guard is an open docket question (F34) — not
  ruled, not taught.
- **F29(a): refined fault discharge** (provably NaN-free orderings
  don't carry the conservative faults bit) — `docs/stdlib-spec.md` §4b,
  ruled by delegation 2026-07-19 (not fully reviewed).
- **The humble heap** (verbs over plain arrays in `std::collections`;
  min-heap on the doctrine order; entry check at `heap_push`;
  `Option` on empty; sealed `Heap<T>` recorded as the upgrade path) —
  `docs/stdlib-spec.md` §8; decision log 2026-07-18 ("Collections+
  ruled"). Implementation NS-A7 (#1113, PR #1156). The bevy-facing
  `ExecMode` default is the docket's F35 — pending.
- **The absence returns** (`min`/`max`/`heap_pop`/`heap_peek` →
  `Option` on empty) — the 2026-07-18 absence doctrine;
  [Option and Absence](option.md).
